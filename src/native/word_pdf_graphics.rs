//! Bounded, opt-in PDF graphics for editable Word. This module never saves the
//! source PDF or renders its original page into an exported image.

use std::collections::{BTreeMap, HashMap};
use std::ffi::{c_int, c_ulong, c_void};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::ptr::null_mut;
use std::sync::Mutex;

use image::{DynamicImage, ImageFormat, RgbaImage};
use pdfium_render::prelude::*;

use super::pdfium_render_ffi::pdfium_dll_path;
use crate::api::{OcrOptions, OcrResult};
use crate::export_word::{WordGraphic, WordPageGraphics, WordRasterFormat, WordRect};
use crate::input_format::InputLimits;
use crate::page_analysis::ProcessingMode;

const MAX_OBJECTS_PER_PAGE: usize = 30_000;
const MAX_OBJECTS_PER_DOCUMENT: usize = 200_000;
const MAX_GRAPHICS_PER_PAGE: usize = 512;
const MAX_GRAPHICS_PER_DOCUMENT: usize = 4_096;
const MAX_IMAGE_SIDE: u32 = 8_192;
const MAX_IMAGE_PIXELS: u64 = 16_000_000;
const MAX_PATH_SEGMENTS: usize = 384;
const MAX_VECTOR_SIDE: u32 = 2_048;
const MAX_VECTOR_PIXELS: u64 = 2_000_000;
const MAX_GRAPHICS_BYTES: usize = 96 * 1024 * 1024;
const MAX_GRAPHICS_WORK_PIXELS: u64 = 128_000_000;
static WORD_PDFIUM: Mutex<()> = Mutex::new(());

type Point = (f32, f32);

#[derive(Clone, Copy)]
struct Transform {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Transform {
    fn from_pdf(matrix: FS_MATRIX) -> Self {
        let FS_MATRIX { a, b, c, d, e, f } = matrix;
        Self { a, b, c, d, e, f }
    }

    fn point(self, (x, y): Point) -> Point {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// `outer(self(point))`.
    fn then(self, outer: Self) -> Self {
        let (a, b) = outer.vector((self.a, self.b));
        let (c, d) = outer.vector((self.c, self.d));
        let (e, f) = outer.point((self.e, self.f));
        Self { a, b, c, d, e, f }
    }

    fn vector(self, (x, y): Point) -> Point {
        (self.a * x + self.c * y, self.b * x + self.d * y)
    }

    fn inverse_point(self, (x, y): Point) -> Point {
        let x = x - self.e;
        let y = y - self.f;
        let det = self.det();
        (
            (self.d * x - self.c * y) / det,
            (self.a * y - self.b * x) / det,
        )
    }

    fn det(self) -> f32 {
        self.a * self.d - self.b * self.c
    }

    fn valid(self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .into_iter()
            .all(|n| n.is_finite() && n.abs() <= 1.0e7)
            && self.det().is_finite()
            && self.det().abs() > 1.0e-10
    }
}

#[derive(Clone, Copy)]
struct Rect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Rect {
    fn around(points: impl IntoIterator<Item = Point>) -> Option<Self> {
        let mut result = Self {
            left: f32::INFINITY,
            top: f32::INFINITY,
            right: f32::NEG_INFINITY,
            bottom: f32::NEG_INFINITY,
        };
        for (x, y) in points {
            if !x.is_finite() || !y.is_finite() || x.abs() > 1.0e7 || y.abs() > 1.0e7 {
                return None;
            }
            result.left = result.left.min(x);
            result.top = result.top.min(y);
            result.right = result.right.max(x);
            result.bottom = result.bottom.max(y);
        }
        result.left.is_finite().then_some(result)
    }

    fn width(self) -> f32 {
        self.right - self.left
    }
    fn height(self) -> f32 {
        self.bottom - self.top
    }
    fn area(self) -> f32 {
        self.width().max(0.0) * self.height().max(0.0)
    }

    fn expanded(self, delta: f32) -> Self {
        Self {
            left: self.left - delta,
            top: self.top - delta,
            right: self.right + delta,
            bottom: self.bottom + delta,
        }
    }

    fn intersect(self, other: Self) -> Option<Self> {
        let clipped = Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        (clipped.area() > 0.0).then_some(clipped)
    }

    fn contains(self, other: Self, epsilon: f32) -> bool {
        other.left >= self.left - epsilon
            && other.top >= self.top - epsilon
            && other.right <= self.right + epsilon
            && other.bottom <= self.bottom + epsilon
    }

    fn word(self) -> WordRect {
        WordRect {
            x: self.left,
            y: self.top,
            width: self.width(),
            height: self.height(),
        }
    }
}

struct Geometry {
    display: Rect,
    to_display: Transform,
}

struct Limits {
    graphics: usize,
    bytes: usize,
    work_pixels: u64,
}

impl Limits {
    fn allows(&self) -> bool {
        self.graphics < MAX_GRAPHICS_PER_DOCUMENT
            && self.bytes < MAX_GRAPHICS_BYTES
            && self.work_pixels < MAX_GRAPHICS_WORK_PIXELS
    }

    fn take_pixels(&mut self, width: u32, height: u32) -> Result<(), &'static str> {
        let sum = self.work_pixels + u64::from(width) * u64::from(height);
        if sum > MAX_GRAPHICS_WORK_PIXELS {
            return Err("document pixel budget reached");
        }
        self.work_pixels = sum;
        Ok(())
    }

    fn add(&mut self, graphic: &WordGraphic) -> Result<(), &'static str> {
        let total = self.bytes + graphic.raster.len() + graphic.svg.as_ref().map_or(0, Vec::len);
        if total > MAX_GRAPHICS_BYTES {
            return Err("document media byte budget reached");
        }
        self.bytes = total;
        self.graphics += 1;
        Ok(())
    }
}

pub(crate) fn collect_pdf_graphics(
    input: &Path,
    options: &OcrOptions,
    result: &OcrResult,
) -> Result<(Vec<WordPageGraphics>, Vec<String>), String> {
    let _guard = WORD_PDFIUM
        .lock()
        .map_err(|_| "Word graphics lock is poisoned")?;
    let input_limits = InputLimits::default();
    if result.document.pages.len() > input_limits.max_frames as usize {
        return Err("too many processed pages for Word graphics".into());
    }
    let mut file = BufReader::new(
        File::open(input).map_err(|e| format!("cannot open PDF for Word graphics: {e}"))?,
    );
    let len = file.get_ref().metadata().map_err(|e| e.to_string())?.len();
    if len == 0 || len > input_limits.max_file_bytes {
        return Err("PDF input file size exceeds Word graphics bounds".into());
    }
    let dll = options
        .pdfium_dll_path
        .clone()
        .map(Ok)
        .unwrap_or_else(pdfium_dll_path)
        .map_err(|e| e.to_string())?;
    let pdfium = match Pdfium::bind_to_library(&dll) {
        Ok(bindings) => Pdfium::new(bindings),
        // The opt-in CLI calls this after OCR. In 0.9.3 `default()` reuses the
        // already initialized bindings on this exact branch; no PATH search.
        Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => Pdfium::default(),
        Err(error) => return Err(format!("cannot open PDFium for Word graphics: {error}")),
    };
    let api = pdfium.bindings();
    let mut access = FPDF_FILEACCESS {
        m_FileLen: c_ulong::try_from(len).map_err(|_| "PDF file too large for Word graphics")?,
        m_GetBlock: Some(read_pdf_block),
        m_Param: (&mut file as *mut BufReader<File>).cast(),
    };
    // The custom accessor avoids Windows code-page conversion. The file and
    // accessor remain live until after `doc` closes, with serialized access.
    let doc = Document::new(api, unsafe {
        api.FPDF_LoadCustomDocument(&mut access, None)
    })
    .ok_or_else(|| {
        format!(
            "PDFium could not reopen PDF for Word graphics (code {})",
            unsafe { api.FPDF_GetLastError() }
        )
    })?;
    let processing: HashMap<_, _> = result
        .page_processing
        .iter()
        .map(|info| (info.page_number, info))
        .collect();
    let table_pages: HashMap<_, _> = result
        .tables
        .iter()
        .map(|tables| (tables.page_number, tables))
        .collect();
    let mut pages = Vec::new();
    let mut warnings = Vec::new();
    let mut inspected = 0;
    let mut limits = Limits {
        graphics: 0,
        bytes: 0,
        work_pixels: 0,
    };
    for processed in &result.document.pages {
        let number = processed.page_number;
        let index = number
            .checked_sub(1)
            .and_then(|index| c_int::try_from(index).ok())
            .ok_or("invalid processed PDF page number")?;
        let page = Page::new(api, unsafe { api.FPDF_LoadPage(doc.raw, index) })
            .ok_or_else(|| format!("PDFium could not reopen page {number} for Word graphics"))?;
        let info = processing.get(&number).copied();
        // NativeText orientation is already the PDF /Rotate; OCR orientation
        // is an additional correction applied to the PDFium-rendered bitmap.
        let extra = info
            .filter(|p| {
                matches!(
                    p.processing_mode,
                    ProcessingMode::Ocr | ProcessingMode::OcrWithGrid
                )
            })
            .map_or(0, |p| p.orientation.angle);
        let geometry = geometry(api, page.raw, extra)
            .ok_or_else(|| format!("invalid Word graphics page geometry on page {number}"))?;
        let mut out = WordPageGraphics {
            page_index: index as usize,
            width_pt: geometry.display.width(),
            height_pt: geometry.display.height(),
            images: Vec::new(),
        };
        let tables: Vec<_> = info
            .and_then(|p| p.render_size)
            .filter(|size| size.width > 0 && size.height > 0)
            .zip(table_pages.get(&number))
            .into_iter()
            .flat_map(|(size, tables)| {
                tables.grids.iter().filter_map(move |grid| {
                    let b = grid.bbox;
                    if b.width == 0 || b.height == 0 {
                        return None;
                    }
                    let fx = geometry.display.width() / size.width as f32;
                    let fy = geometry.display.height() / size.height as f32;
                    Some(Rect {
                        left: b.x as f32 * fx,
                        top: b.y as f32 * fy,
                        right: b.x.saturating_add(b.width) as f32 * fx,
                        bottom: b.y.saturating_add(b.height) as f32 * fy,
                    })
                })
            })
            .collect();
        let mut skipped = BTreeMap::new();
        let count = usize::try_from(unsafe { api.FPDFPage_CountObjects(page.raw) })
            .map_err(|_| format!("cannot count PDF objects on page {number}"))?;
        let allowed = count
            .min(MAX_OBJECTS_PER_PAGE)
            .min(MAX_OBJECTS_PER_DOCUMENT - inspected);
        if count > allowed {
            skipped.insert(
                "page/document object inspection limit reached",
                count - allowed,
            );
        }
        let annotations = unsafe { api.FPDFPage_GetAnnotCount(page.raw) };
        if annotations > 0 {
            skipped.insert("PDF annotations not projected", annotations as usize);
        }
        for object_index in 0..allowed {
            if out.images.len() >= MAX_GRAPHICS_PER_PAGE || !limits.allows() {
                *skipped
                    .entry("page/document graphics budget reached")
                    .or_default() += allowed - object_index;
                break;
            }
            inspected += 1;
            let object = unsafe { api.FPDFPage_GetObject(page.raw, object_index as c_int) };
            let outcome = if object.is_null() {
                Err("PDF object unavailable")
            } else {
                inspect_object(
                    api,
                    doc.raw,
                    page.raw,
                    object,
                    &geometry,
                    &tables,
                    &mut limits,
                )
            };
            match outcome {
                Ok(Some(graphic)) => match limits.add(&graphic) {
                    Ok(()) => out.images.push(graphic),
                    Err(reason) => {
                        *skipped.entry(reason).or_default() += 1;
                        break;
                    }
                },
                Ok(None) => {}
                Err(reason) => *skipped.entry(reason).or_default() += 1,
            }
        }
        warnings.extend(
            skipped
                .into_iter()
                .map(|(reason, count)| format!("PDF page {number}: {reason} ({count})")),
        );
        pages.push(out);
    }
    Ok((pages, warnings))
}

unsafe extern "C" fn read_pdf_block(
    param: *mut c_void,
    offset: c_ulong,
    buffer: *mut u8,
    size: c_ulong,
) -> c_int {
    if param.is_null() || buffer.is_null() || size == 0 || u64::from(size) > isize::MAX as u64 {
        return 0;
    }
    // PDFium supplies a writable buffer of this length and the caller-owned
    // reader pointer. There are no aliases while this synchronous call runs.
    let reader = unsafe { &mut *param.cast::<BufReader<File>>() };
    let data = unsafe { std::slice::from_raw_parts_mut(buffer, size as usize) };
    i32::from(
        reader
            .seek(SeekFrom::Start(u64::from(offset)))
            .and_then(|_| reader.read_exact(data))
            .is_ok(),
    )
}

fn geometry(api: &dyn PdfiumLibraryBindings, page: FPDF_PAGE, extra: u16) -> Option<Geometry> {
    let w = unsafe { api.FPDF_GetPageWidthF(page) };
    let h = unsafe { api.FPDF_GetPageHeightF(page) };
    if ![w, h]
        .into_iter()
        .all(|dim| dim.is_finite() && dim > 0.0 && dim <= 100_000.0)
    {
        return None;
    }
    // Derive PDFium's exact effective CropBox + /Rotate mapping from device
    // corners, including non-zero and inherited boxes, without integer
    // rounding any actual object coordinates.
    let corner = |x, y| {
        let mut px = 0.0;
        let mut py = 0.0;
        let ok =
            unsafe { api.FPDF_DeviceToPage(page, 0, 0, 1024, 1024, 0, x, y, &mut px, &mut py) };
        (ok != 0).then_some((px as f32, py as f32))
    };
    let (e, f) = corner(0, 0)?;
    let (x, y) = corner(1024, 0)?;
    let (u, v) = corner(0, 1024)?;
    let inv = Transform {
        a: (x - e) / w,
        b: (y - f) / w,
        c: (u - e) / h,
        d: (v - f) / h,
        e,
        f,
    };
    if !inv.valid() {
        return None;
    }
    let (e, f) = inv.inverse_point((0.0, 0.0));
    // Use the exact inverse linear terms instead of subtracting large offsets.
    let det = inv.det();
    let base = Transform {
        a: inv.d / det,
        b: -inv.b / det,
        c: -inv.c / det,
        d: inv.a / det,
        e,
        f,
    };
    let (turn, width, height) = match extra {
        0 => (
            Transform {
                a: 1.,
                b: 0.,
                c: 0.,
                d: 1.,
                e: 0.,
                f: 0.,
            },
            w,
            h,
        ),
        90 => (
            Transform {
                a: 0.,
                b: 1.,
                c: -1.,
                d: 0.,
                e: h,
                f: 0.,
            },
            h,
            w,
        ),
        180 => (
            Transform {
                a: -1.,
                b: 0.,
                c: 0.,
                d: -1.,
                e: w,
                f: h,
            },
            w,
            h,
        ),
        270 => (
            Transform {
                a: 0.,
                b: -1.,
                c: 1.,
                d: 0.,
                e: 0.,
                f: w,
            },
            h,
            w,
        ),
        _ => return None,
    };
    Some(Geometry {
        display: Rect {
            left: 0.,
            top: 0.,
            right: width,
            bottom: height,
        },
        to_display: base.then(turn),
    })
}

fn inspect_object(
    api: &dyn PdfiumLibraryBindings,
    doc: FPDF_DOCUMENT,
    page: FPDF_PAGE,
    object: FPDF_PAGEOBJECT,
    geometry: &Geometry,
    tables: &[Rect],
    limits: &mut Limits,
) -> Result<Option<WordGraphic>, &'static str> {
    let kind = unsafe { api.FPDFPageObj_GetType(object) } as u32;
    if kind == FPDF_PAGEOBJ_TEXT {
        return Ok(None);
    }
    let mut active = 0;
    if unsafe { api.FPDFPageObj_GetIsActive(object, &mut active) } == 0 {
        return Err("could not verify PDF object visibility");
    }
    if active == 0 {
        return Ok(None);
    }
    if unsafe { api.FPDFPageObj_CountMarks(object) } > 0 {
        return Err("marked or optional-content graphic not projected");
    }
    match kind {
        FPDF_PAGEOBJ_IMAGE => image_object(api, doc, page, object, geometry, tables, limits),
        FPDF_PAGEOBJ_PATH => path_object(api, object, geometry, tables, limits),
        FPDF_PAGEOBJ_FORM => Err("nested PDF Form XObject not projected"),
        FPDF_PAGEOBJ_SHADING => Err("PDF shading object not projected"),
        _ => Err("unsupported PDF graphic not projected"),
    }
}

fn object_matrix(
    api: &dyn PdfiumLibraryBindings,
    object: FPDF_PAGEOBJECT,
) -> Result<FS_MATRIX, &'static str> {
    let mut matrix = FS_MATRIX {
        a: 0.,
        b: 0.,
        c: 0.,
        d: 0.,
        e: 0.,
        f: 0.,
    };
    if unsafe { api.FPDFPageObj_GetMatrix(object, &mut matrix) } == 0
        || !Transform::from_pdf(matrix).valid()
    {
        return Err("unsupported PDF object matrix");
    }
    Ok(matrix)
}

fn has_clip(api: &dyn PdfiumLibraryBindings, object: FPDF_PAGEOBJECT) -> bool {
    // A nonnegative count includes the zero-path text-only clipping case.
    let clip = unsafe { api.FPDFPageObj_GetClipPath(object) };
    !clip.is_null() && unsafe { api.FPDFClipPath_CountPaths(clip) } >= 0
}

struct Document<'a> {
    api: &'a dyn PdfiumLibraryBindings,
    raw: FPDF_DOCUMENT,
}
impl<'a> Document<'a> {
    fn new(api: &'a dyn PdfiumLibraryBindings, raw: FPDF_DOCUMENT) -> Option<Self> {
        if raw.is_null() {
            None
        } else {
            Some(Self { api, raw })
        }
    }
}
impl Drop for Document<'_> {
    fn drop(&mut self) {
        unsafe { self.api.FPDF_CloseDocument(self.raw) };
    }
}

struct Page<'a> {
    api: &'a dyn PdfiumLibraryBindings,
    raw: FPDF_PAGE,
}
impl<'a> Page<'a> {
    fn new(api: &'a dyn PdfiumLibraryBindings, raw: FPDF_PAGE) -> Option<Self> {
        if raw.is_null() {
            None
        } else {
            Some(Self { api, raw })
        }
    }
}
impl Drop for Page<'_> {
    fn drop(&mut self) {
        unsafe { self.api.FPDF_ClosePage(self.raw) };
    }
}

struct Bitmap<'a> {
    api: &'a dyn PdfiumLibraryBindings,
    raw: FPDF_BITMAP,
}
impl<'a> Bitmap<'a> {
    fn new(api: &'a dyn PdfiumLibraryBindings, raw: FPDF_BITMAP) -> Option<Self> {
        if raw.is_null() {
            None
        } else {
            Some(Self { api, raw })
        }
    }
}
impl Drop for Bitmap<'_> {
    fn drop(&mut self) {
        unsafe { self.api.FPDFBitmap_Destroy(self.raw) };
    }
}

fn bitmap_rgba(bitmap: &Bitmap<'_>, width: u32, height: u32) -> Result<RgbaImage, &'static str> {
    let api = bitmap.api;
    let b = bitmap.raw;
    let (w, h, stride, format, data) = unsafe {
        (
            api.FPDFBitmap_GetWidth(b),
            api.FPDFBitmap_GetHeight(b),
            api.FPDFBitmap_GetStride(b),
            api.FPDFBitmap_GetFormat(b),
            api.FPDFBitmap_GetBuffer(b),
        )
    };
    let channels = match format {
        2 => 3,
        3..=5 => 4,
        _ => return Err("unsupported PDFium bitmap format"),
    };
    if (w, h) != (width as i32, height as i32)
        || stride < w * channels
        || stride > w * channels + 64
        || data.is_null()
    {
        return Err("invalid PDFium graphic bitmap");
    }
    let size = (stride as usize) * (h as usize);
    let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) };
    let mut result = RgbaImage::new(width, height);
    for (dst, src) in result
        .chunks_exact_mut(width as usize * 4)
        .zip(bytes.chunks_exact(stride as usize))
    {
        for (pixel, bgr) in dst
            .chunks_exact_mut(4)
            .zip(src.chunks_exact(channels as usize))
        {
            let a = if format >= 4 { bgr[3] } else { 255 };
            let expand = |v| {
                if format == 5 && a != 0 {
                    ((u32::from(v) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
                } else {
                    v
                }
            };
            pixel.copy_from_slice(&[expand(bgr[2]), expand(bgr[1]), expand(bgr[0]), a]);
        }
    }
    Ok(result)
}

fn png(image: RgbaImage) -> Result<Vec<u8>, &'static str> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(|_| "could not encode PDF graphic PNG")?;
    Ok(bytes.into_inner())
}

fn word_graphic(
    rect: Rect,
    raster: Vec<u8>,
    raster_format: WordRasterFormat,
    svg: Option<Vec<u8>>,
) -> WordGraphic {
    WordGraphic {
        rect: rect.word(),
        raster,
        raster_format,
        svg,
        scan_reference: false,
    }
}

fn image_object(
    api: &dyn PdfiumLibraryBindings,
    doc: FPDF_DOCUMENT,
    page: FPDF_PAGE,
    object: FPDF_PAGEOBJECT,
    geometry: &Geometry,
    tables: &[Rect],
    limits: &mut Limits,
) -> Result<Option<WordGraphic>, &'static str> {
    let source = object_matrix(api, object)?;
    // Convert normalized top-left image coordinates to PDF's bottom-left unit square.
    let normalized = Transform {
        a: 1.,
        b: 0.,
        c: 0.,
        d: -1.,
        e: 0.,
        f: 1.,
    };
    let placed = normalized
        .then(Transform::from_pdf(source))
        .then(geometry.to_display);
    if !placed.valid() {
        return Err("unsupported PDF image placement");
    }
    let full = Rect::around([(0., 0.), (1., 0.), (0., 1.), (1., 1.)].map(|p| placed.point(p)))
        .ok_or("invalid PDF image bounds")?;
    let Some(visible) = geometry.display.intersect(full) else {
        return Ok(None);
    };
    // Applied to any route: a nearly page-sized searchable image can contain
    // a hidden text layer just as a scan does. Do not duplicate the editable
    // page even when OCR was not needed for its text.
    if visible.width() >= geometry.display.width() * 0.90
        && visible.height() >= geometry.display.height() * 0.90
        && placed.det().abs() >= geometry.display.area() * 0.80
    {
        return Err("near-full-page image omitted to avoid duplicating editable content");
    }
    if tables.iter().any(|table| {
        table.area() > 0.0
            && visible.intersect(*table).is_some_and(|shared| {
                shared.area() >= table.area() * 0.90 && shared.area() >= visible.area() * 0.40
            })
    }) {
        return Err("PDF image overlaps a reconstructed table; not duplicated as a picture");
    }
    if has_clip(api, object) {
        return Err("clipped PDF image not projected");
    }
    if unsafe { api.FPDFPageObj_HasTransparency(object) } != 0 {
        return Err("PDF image uses page-level transparency or blend mode");
    }
    let (mut width, mut height) = (0, 0);
    if unsafe { api.FPDFImageObj_GetImagePixelSize(object, &mut width, &mut height) } == 0
        || width == 0
        || height == 0
        || width > MAX_IMAGE_SIDE
        || height > MAX_IMAGE_SIDE
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
    {
        return Err("PDF image pixel size exceeds export bounds");
    }
    limits.take_pixels(width, height)?;
    let canonical = FS_MATRIX {
        a: width as f32,
        b: 0.,
        c: 0.,
        d: height as f32,
        e: 0.,
        f: 0.,
    };
    // Render only this image at its own source dimensions. The normalization
    // avoids PDFium's undocumented rotated/mirrored output geometry; the
    // source-file matrix is restored before any further inspection or return.
    if unsafe { api.FPDFPageObj_SetMatrix(object, &canonical) } == 0 {
        return Err("could not normalize PDF image matrix");
    }
    let bitmap = Bitmap::new(api, unsafe {
        api.FPDFImageObj_GetRenderedBitmap(doc, page, object)
    });
    if unsafe { api.FPDFPageObj_SetMatrix(object, &source) } == 0 {
        return Err("could not restore transient PDF image matrix");
    }
    let pixels = bitmap_rgba(
        &bitmap.ok_or("PDFium could not render embedded image")?,
        width,
        height,
    )?;
    if pixels.pixels().all(|pixel| pixel[3] == 0) {
        return Ok(None);
    }
    let axis_tolerance = visible.width().max(visible.height()) * 0.00001;
    if placed.a > 0.
        && placed.d > 0.
        && placed.b.abs() <= axis_tolerance
        && placed.c.abs() <= axis_tolerance
        && geometry.display.contains(full, 0.00001)
    {
        if let Some(jpeg) = verified_jpeg(api, object, &pixels, limits) {
            return Ok(Some(word_graphic(
                visible,
                jpeg,
                WordRasterFormat::Jpeg,
                None,
            )));
        }
        return Ok(Some(word_graphic(
            visible,
            png(pixels)?,
            WordRasterFormat::Png,
            None,
        )));
    }
    let projected = transform_image(&pixels, placed, visible, limits)?;
    if projected.pixels().all(|pixel| pixel[3] == 0) {
        return Ok(None);
    }
    Ok(Some(word_graphic(
        visible,
        png(projected)?,
        WordRasterFormat::Png,
        None,
    )))
}

fn transform_image(
    image: &RgbaImage,
    placed: Transform,
    rect: Rect,
    limits: &mut Limits,
) -> Result<RgbaImage, &'static str> {
    // Preserve native samples for rotations/reflections; arbitrary affine
    // placement uses a bounded transparent object box, never a page crop.
    let x_density = (placed.a.abs() * image.width() as f32
        + placed.c.abs() * image.height() as f32)
        / (placed.a * placed.a + placed.c * placed.c).max(1.0e-10);
    let y_density = (placed.b.abs() * image.width() as f32
        + placed.d.abs() * image.height() as f32)
        / (placed.b * placed.b + placed.d * placed.d).max(1.0e-10);
    let (width, height) = bounded_dimensions(
        rect.width() * x_density,
        rect.height() * y_density,
        MAX_IMAGE_SIDE,
        MAX_IMAGE_PIXELS,
    );
    limits.take_pixels(width, height)?;
    let mut result = RgbaImage::new(width, height);
    for (x, y, dst) in result.enumerate_pixels_mut() {
        let (u, v) = placed.inverse_point((
            rect.left + (x as f32 + 0.5) / width as f32 * rect.width(),
            rect.top + (y as f32 + 0.5) / height as f32 * rect.height(),
        ));
        if (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v) {
            let ix = ((u * image.width() as f32) as u32).min(image.width() - 1);
            let iy = ((v * image.height() as f32) as u32).min(image.height() - 1);
            *dst = *image.get_pixel(ix, iy);
        }
    }
    Ok(result)
}

fn bounded_dimensions(width: f32, height: f32, side: u32, pixels: u64) -> (u32, u32) {
    let width = width.max(1.);
    let height = height.max(1.);
    let scale = 1.0_f32
        .min(side as f32 / width)
        .min(side as f32 / height)
        .min(((pixels.saturating_sub(u64::from(side) * 2)) as f32 / (width * height)).sqrt());
    (
        (width * scale - 0.0001).ceil().max(1.) as u32,
        (height * scale - 0.0001).ceil().max(1.) as u32,
    )
}

fn verified_jpeg(
    api: &dyn PdfiumLibraryBindings,
    object: FPDF_PAGEOBJECT,
    appearance: &RgbaImage,
    limits: &mut Limits,
) -> Option<Vec<u8>> {
    if unsafe { api.FPDFImageObj_GetImageFilterCount(object) } != 1
        || appearance.pixels().any(|p| p[3] != 255)
    {
        return None;
    }
    let mut filter = [0u8; 32];
    let len = unsafe {
        api.FPDFImageObj_GetImageFilter(
            object,
            0,
            filter.as_mut_ptr().cast(),
            filter.len() as c_ulong,
        )
    } as usize;
    if filter.get(..len)? != b"DCTDecode\0" {
        return None;
    }
    let len = unsafe { api.FPDFImageObj_GetImageDataRaw(object, null_mut(), 0) };
    if len == 0 || len as usize > 12 * 1024 * 1024 {
        return None;
    }
    let mut data = vec![0u8; len as usize];
    if unsafe { api.FPDFImageObj_GetImageDataRaw(object, data.as_mut_ptr().cast(), len) } != len
        || !standalone_jpeg(&data)
    {
        return None;
    }
    let decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(data.as_slice())).ok()?;
    use image::ImageDecoder;
    if decoder.dimensions() != appearance.dimensions() {
        return None;
    }
    limits
        .take_pixels(appearance.width(), appearance.height())
        .ok()?;
    // PDF /Decode, masks, or color profiles can change an otherwise valid
    // JPEG. Compare standalone decoder output to PDFium at source size;
    // retain the original compressed bytes only if appearances agree.
    let standalone = DynamicImage::from_decoder(decoder).ok()?.to_rgb8();
    let mut difference = 0_u64;
    for (a, b) in standalone.pixels().zip(appearance.pixels()) {
        for channel in 0..3 {
            let delta = a[channel].abs_diff(b[channel]);
            if delta > 12 {
                return None;
            }
            difference += u64::from(delta);
        }
    }
    (difference <= u64::from(appearance.width()) * u64::from(appearance.height()) * 6)
        .then_some(data)
}

fn standalone_jpeg(data: &[u8]) -> bool {
    if !data.starts_with(&[0xff, 0xd8]) || !data.ends_with(&[0xff, 0xd9]) {
        return false;
    }
    let mut offset = 2;
    let mut scan = false;
    let mut saw_scan = false;
    while offset < data.len() {
        if scan {
            while data.get(offset).is_some_and(|byte| *byte != 0xff) {
                offset += 1;
            }
        }
        if data.get(offset) != Some(&0xff) {
            return false;
        }
        while data.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let Some(&marker) = data.get(offset) else {
            return false;
        };
        offset += 1;
        if scan && matches!(marker, 0x00 | 0xd0..=0xd7) {
            continue;
        }
        if marker == 0xd9 {
            return saw_scan && offset == data.len();
        }
        let Some(bytes) = data.get(offset..offset + 2) else {
            return false;
        };
        let size = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
        let Some(payload) = (size >= 2)
            .then(|| data.get(offset + 2..offset + size))
            .flatten()
        else {
            return false;
        };
        // Accept ordinary baseline/progressive color structure and standard
        // fixed-size JFIF/Adobe headers only. No EXIF/ICC, comment, thumbnail,
        // source-private metadata, or trailing content after EOI.
        match marker {
            0xc0..=0xc2 | 0xc4 | 0xda..=0xdd => {}
            0xe0 if payload.len() == 14
                && payload.starts_with(b"JFIF\0")
                && payload[12..] == [0, 0] => {}
            0xee if payload.len() == 12 && payload.starts_with(b"Adobe") => {}
            _ => return false,
        }
        scan = marker == 0xda;
        saw_scan |= scan;
        offset += size;
    }
    false
}

#[derive(Clone, Copy)]
enum Command {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}

impl Command {
    fn points(self) -> impl Iterator<Item = Point> {
        match self {
            Self::Move(p) | Self::Line(p) => [Some(p), None, None],
            Self::Cubic(a, b, c) => [Some(a), Some(b), Some(c)],
            Self::Close => [None; 3],
        }
        .into_iter()
        .flatten()
    }
}

type Color = [u32; 4];

struct Style {
    fill_mode: c_int,
    fill: Option<Color>,
    stroke: Option<Color>,
    width: f32,
    cap: c_int,
    join: c_int,
    dash: Vec<f32>,
    dash_phase: f32,
}

fn path_object(
    api: &dyn PdfiumLibraryBindings,
    object: FPDF_PAGEOBJECT,
    geometry: &Geometry,
    tables: &[Rect],
    limits: &mut Limits,
) -> Result<Option<WordGraphic>, &'static str> {
    let transform = Transform::from_pdf(object_matrix(api, object)?).then(geometry.to_display);
    if !transform.valid() {
        return Err("unsupported vector placement");
    }
    let mut style = Style {
        fill_mode: 0,
        fill: None,
        stroke: None,
        width: 0.,
        cap: 0,
        join: 0,
        dash: Vec::new(),
        dash_phase: 0.,
    };
    let mut stroked = 0;
    if unsafe { api.FPDFPath_GetDrawMode(object, &mut style.fill_mode, &mut stroked) } == 0
        || !(0..=2).contains(&style.fill_mode)
    {
        return Err("unknown PDF vector draw mode");
    }
    if style.fill_mode == 0 && stroked == 0 {
        return Ok(None);
    }
    let commands = path_commands(api, object, transform)?;
    let Some(bounds) = Rect::around(commands.iter().flat_map(|c| c.points())) else {
        return Err("invalid PDF vector points");
    };
    // Word regenerates detected table rules. Suppress only a narrow rule
    // geometry, not arbitrary drawings located inside a table's box.
    if (stroked == 0 || unsafe { api.FPDFPageObj_GetDashCount(object) } == 0)
        && tables
            .iter()
            .any(|table| is_table_rule(&commands, bounds, style.fill_mode, *table))
    {
        return Ok(None);
    }
    if has_clip(api, object) {
        return Err("clipped PDF vector not projected");
    }
    if unsafe { api.FPDFPageObj_HasTransparency(object) } != 0 {
        return Err("PDF vector transparency or blend mode not projected");
    }
    if style.fill_mode != 0 {
        style.fill = Some(object_color(api, object, false)?);
    }
    if stroked != 0 {
        let xscale = transform.a.hypot(transform.b);
        let yscale = transform.c.hypot(transform.d);
        let scale2 = xscale * yscale;
        if (xscale - yscale).abs() > xscale.max(yscale) * 0.00001
            || (transform.a * transform.c + transform.b * transform.d).abs() > scale2 * 0.00001
        {
            return Err("anisotropic PDF stroke not projected");
        }
        let (dash, phase) = dash_pattern(api, object, xscale)?;
        style.dash = dash;
        style.dash_phase = phase;
        if unsafe { api.FPDFPageObj_GetStrokeWidth(object, &mut style.width) } == 0 {
            return Err("unknown PDF stroke width");
        }
        style.width *= xscale;
        if !style.width.is_finite() || style.width <= 0. || style.width > 144. {
            return Err("hairline or extreme PDF stroke width not projected");
        }
        style.cap = unsafe { api.FPDFPageObj_GetLineCap(object) };
        style.join = unsafe { api.FPDFPageObj_GetLineJoin(object) };
        if !(0..=2).contains(&style.cap) || !(0..=2).contains(&style.join) {
            return Err("unknown PDF line cap or join");
        }
        style.stroke = Some(object_color(api, object, true)?);
    }
    let padding = if style.stroke.is_some() {
        1. + if style.join == 0 { 5.0 } else { 0.8 } * style.width
    } else {
        0.5
    };
    let Some(rect) = geometry.display.intersect(bounds.expanded(padding)) else {
        return Ok(None);
    };
    if !geometry.display.contains(bounds, 0.001) {
        return Err("PDF vector crosses page crop boundary");
    }
    if style.fill.is_some() && bounds.area() >= geometry.display.area() * 0.86 {
        return Err("near-full-page vector background not projected");
    }
    let (width, height) = bounded_dimensions(
        rect.width() * 3.0,
        rect.height() * 3.0,
        MAX_VECTOR_SIDE,
        MAX_VECTOR_PIXELS,
    );
    limits.take_pixels(width, height)?;
    let fallback = scratch_path_png(api, &commands, &style, rect, width, height)?;
    let Some(fallback) = fallback else {
        return Ok(None);
    };
    // Keep the constrained SVG dialect small. A well-bounded dashed path
    // is retained as a transparent PNG of the isolated path instead.
    let svg = style
        .dash
        .is_empty()
        .then(|| path_svg(&commands, &style, rect));
    Ok(Some(word_graphic(
        rect,
        fallback,
        WordRasterFormat::Png,
        svg,
    )))
}

fn dash_pattern(
    api: &dyn PdfiumLibraryBindings,
    object: FPDF_PAGEOBJECT,
    scale: f32,
) -> Result<(Vec<f32>, f32), &'static str> {
    let count = unsafe { api.FPDFPageObj_GetDashCount(object) };
    if count == 0 {
        return Ok((Vec::new(), 0.));
    }
    if !(1..=16).contains(&count) {
        return Err("PDF dash count exceeds vector export bounds");
    }
    let mut lengths = vec![0.0; count as usize];
    let mut phase = 0.0;
    if unsafe { api.FPDFPageObj_GetDashArray(object, lengths.as_mut_ptr(), lengths.len()) } == 0
        || unsafe { api.FPDFPageObj_GetDashPhase(object, &mut phase) } == 0
    {
        return Err("unreadable PDF dash pattern");
    }
    lengths.iter_mut().for_each(|n| *n *= scale);
    phase *= scale;
    if !lengths.iter().all(|n| (0.5..=10_000.0).contains(n))
        || !(-100_000.0..=100_000.0).contains(&phase)
    {
        return Err("PDF dash dimensions exceed vector export bounds");
    }
    Ok((lengths, phase))
}

fn is_table_rule(commands: &[Command], bounds: Rect, fill_mode: c_int, table: Rect) -> bool {
    const EPS: f32 = 3.0;
    if !table.contains(bounds, EPS) {
        return false;
    }
    let near = |a: f32, b: f32| (a - b).abs() <= EPS;
    let full_width = near(bounds.left, table.left) && near(bounds.right, table.right);
    let full_height = near(bounds.top, table.top) && near(bounds.bottom, table.bottom);
    let axis_aligned =
        |(ax, ay): Point, (bx, by): Point| (ax - bx).abs().min((ay - by).abs()) <= 0.1;
    if let [Command::Move(a), Command::Line(b)] = commands {
        return fill_mode == 0
            && axis_aligned(*a, *b)
            && ((full_width && bounds.height() <= 0.1) || (full_height && bounds.width() <= 0.1));
    }
    let vertices = match commands {
        [Command::Move(a), Command::Line(b), Command::Line(c), Command::Line(d), Command::Close] => {
            [*a, *b, *c, *d]
        }
        [Command::Move(a), Command::Line(b), Command::Line(c), Command::Line(d), Command::Line(end), Command::Close]
            if a == end =>
        {
            [*a, *b, *c, *d]
        }
        _ => return false,
    };
    if !vertices
        .iter()
        .zip(vertices.iter().cycle().skip(1))
        .all(|(a, b)| axis_aligned(*a, *b))
    {
        return false;
    }
    (fill_mode == 0 && full_width && full_height)
        || (full_width && bounds.height() <= 2.0)
        || (full_height && bounds.width() <= 2.0)
}

fn object_color(
    api: &dyn PdfiumLibraryBindings,
    object: FPDF_PAGEOBJECT,
    stroke: bool,
) -> Result<Color, &'static str> {
    let (mut r, mut g, mut b, mut a) = (0, 0, 0, 0);
    let ok = unsafe {
        if stroke {
            api.FPDFPageObj_GetStrokeColor(object, &mut r, &mut g, &mut b, &mut a)
        } else {
            api.FPDFPageObj_GetFillColor(object, &mut r, &mut g, &mut b, &mut a)
        }
    };
    if ok == 0 || r > 255 || g > 255 || b > 255 || a != 255 {
        return Err("unsupported PDF vector paint");
    }
    Ok([r, g, b, a])
}

fn path_commands(
    api: &dyn PdfiumLibraryBindings,
    object: FPDF_PAGEOBJECT,
    transform: Transform,
) -> Result<Vec<Command>, &'static str> {
    let count = unsafe { api.FPDFPath_CountSegments(object) };
    if count <= 0 || count as usize > MAX_PATH_SEGMENTS {
        return Err("PDF vector segment count exceeds export bounds");
    }
    let get = |index| -> Result<_, &'static str> {
        let segment = unsafe { api.FPDFPath_GetPathSegment(object, index) };
        if segment.is_null() {
            return Err("missing PDF vector segment");
        }
        let (mut x, mut y) = (0.0, 0.0);
        if unsafe { api.FPDFPathSegment_GetPoint(segment, &mut x, &mut y) } == 0 {
            return Err("unreadable PDF vector segment");
        }
        Ok((
            unsafe { api.FPDFPathSegment_GetType(segment) },
            transform.point((x, y)),
            unsafe { api.FPDFPathSegment_GetClose(segment) } != 0,
        ))
    };
    let mut commands = Vec::with_capacity(count as usize);
    let mut index = 0;
    while index < count {
        let (kind, p, closed) = get(index)?;
        let (command, closed) = match kind {
            2 if !closed => (Command::Move(p), false),
            0 if !commands.is_empty() => (Command::Line(p), closed),
            1 if !closed && index + 2 < count && !commands.is_empty() => {
                let (k1, p1, closed1) = get(index + 1)?;
                let (k2, p2, closed2) = get(index + 2)?;
                if k1 != 1 || k2 != 1 || closed1 {
                    return Err("invalid PDF cubic segment");
                }
                index += 2;
                (Command::Cubic(p, p1, p2), closed2)
            }
            _ => return Err("unsupported PDF vector segment"),
        };
        commands.push(command);
        if closed {
            commands.push(Command::Close);
        }
        index += 1;
    }
    Ok(commands)
}

fn path_svg(commands: &[Command], style: &Style, rect: Rect) -> Vec<u8> {
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {:.5} {:.5}\"><path d=\"",
        rect.width(),
        rect.height()
    );
    let local = |(x, y): Point| (x - rect.left, y - rect.top);
    for command in commands {
        match *command {
            Command::Move(p) => {
                let (x, y) = local(p);
                let _ = write!(svg, "M {x:.5} {y:.5} ");
            }
            Command::Line(p) => {
                let (x, y) = local(p);
                let _ = write!(svg, "L {x:.5} {y:.5} ");
            }
            Command::Cubic(p, q, r) => {
                let (x, y) = local(p);
                let (u, v) = local(q);
                let (a, b) = local(r);
                let _ = write!(svg, "C {x:.5} {y:.5} {u:.5} {v:.5} {a:.5} {b:.5} ");
            }
            Command::Close => svg.push_str("Z "),
        }
    }
    let paint = |color: Option<Color>| match color {
        Some([r, g, b, _]) => format!("#{r:02x}{g:02x}{b:02x}"),
        None => "none".to_string(),
    };
    let rule = if style.fill_mode == 1 {
        "evenodd"
    } else {
        "nonzero"
    };
    let _ = write!(
        svg,
        "\" fill=\"{}\" fill-rule=\"{rule}\" stroke=\"{}\"",
        paint(style.fill),
        paint(style.stroke)
    );
    if style.stroke.is_some() {
        let cap = ["butt", "round", "square"][style.cap as usize];
        let join = ["miter", "round", "bevel"][style.join as usize];
        let _ = write!(svg, " stroke-width=\"{:.5}\" stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\" stroke-miterlimit=\"10\"", style.width);
    }
    svg.push_str("/></svg>");
    svg.into_bytes()
}

fn scratch_path_png(
    api: &dyn PdfiumLibraryBindings,
    commands: &[Command],
    style: &Style,
    rect: Rect,
    width: u32,
    height: u32,
) -> Result<Option<Vec<u8>>, &'static str> {
    // This isolated PDF never contains source text, table geometry, or
    // unrelated objects, and is never written to disk.
    let doc = Document::new(api, unsafe { api.FPDF_CreateNewDocument() })
        .ok_or("could not allocate PDF scratch document")?;
    let page = Page::new(api, unsafe {
        api.FPDFPage_New(
            doc.raw,
            0,
            f64::from(rect.width()),
            f64::from(rect.height()),
        )
    })
    .ok_or("could not allocate PDF scratch page")?;
    let local = |(x, y): Point| (x - rect.left, rect.bottom - y);
    let Some(Command::Move(first)) = commands.first() else {
        return Err("PDF vector does not start with move");
    };
    let (x, y) = local(*first);
    let raw = unsafe { api.FPDFPageObj_CreateNewPath(x, y) };
    if raw.is_null() {
        return Err("could not allocate PDF scratch path");
    }
    let mut object = ScratchObject { api, raw };
    for command in &commands[1..] {
        let ok = unsafe {
            match *command {
                Command::Move(p) => {
                    let (x, y) = local(p);
                    api.FPDFPath_MoveTo(raw, x, y)
                }
                Command::Line(p) => {
                    let (x, y) = local(p);
                    api.FPDFPath_LineTo(raw, x, y)
                }
                Command::Cubic(p, q, r) => {
                    let (x, y) = local(p);
                    let (u, v) = local(q);
                    let (a, b) = local(r);
                    api.FPDFPath_BezierTo(raw, x, y, u, v, a, b)
                }
                Command::Close => api.FPDFPath_Close(raw),
            }
        };
        if ok == 0 {
            return Err("could not rebuild isolated PDF vector segment");
        }
    }
    let ok = unsafe {
        (api.FPDFPath_SetDrawMode(raw, style.fill_mode, i32::from(style.stroke.is_some())) != 0)
            && style
                .fill
                .is_none_or(|[r, g, b, a]| api.FPDFPageObj_SetFillColor(raw, r, g, b, a) != 0)
            && style.stroke.is_none_or(|[r, g, b, a]| {
                api.FPDFPageObj_SetStrokeColor(raw, r, g, b, a) != 0
                    && api.FPDFPageObj_SetStrokeWidth(raw, style.width) != 0
                    && api.FPDFPageObj_SetLineCap(raw, style.cap) != 0
                    && api.FPDFPageObj_SetLineJoin(raw, style.join) != 0
                    && (style.dash.is_empty()
                        || api.FPDFPageObj_SetDashArray(
                            raw,
                            style.dash.as_ptr(),
                            style.dash.len(),
                            style.dash_phase,
                        ) != 0)
            })
    };
    if !ok {
        return Err("could not rebuild isolated PDF vector style");
    }
    // 7881 takes ownership both on success and on failure.
    object.raw = null_mut();
    if unsafe { api.FPDFPage_InsertObject(page.raw, raw) } == 0 {
        return Err("could not place isolated PDF vector");
    }
    let (w, h) = (width as c_int, height as c_int);
    let bitmap = Bitmap::new(api, unsafe { api.FPDFBitmap_Create(w, h, 1) })
        .ok_or("could not allocate PDF vector bitmap")?;
    if unsafe { api.FPDFBitmap_FillRect(bitmap.raw, 0, 0, w, h, 0) } == 0 {
        return Err("could not clear PDF vector bitmap");
    }
    unsafe { api.FPDF_RenderPageBitmap(bitmap.raw, page.raw, 0, 0, w, h, 0, 0) };
    let pixels = bitmap_rgba(&bitmap, width, height)?;
    if pixels.pixels().all(|p| p[3] == 0) {
        return Ok(None);
    }
    png(pixels).map(Some)
}

struct ScratchObject<'a> {
    api: &'a dyn PdfiumLibraryBindings,
    raw: FPDF_PAGEOBJECT,
}
impl Drop for ScratchObject<'_> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { self.api.FPDFPageObj_Destroy(self.raw) };
        }
    }
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    #[test]
    fn affine_image_rotation_preserves_native_non_square_samples() {
        let original = RgbaImage::from_fn(4, 2, |x, y| Rgba([x as u8 * 30, y as u8 * 40, 72, 128]));
        let placed = Transform {
            a: 0.,
            b: 120.,
            c: -60.,
            d: 0.,
            e: 100.,
            f: 70.,
        };
        let bounds = Rect {
            left: 40.,
            top: 70.,
            right: 100.,
            bottom: 190.,
        };
        let mut limits = Limits {
            graphics: 0,
            bytes: 0,
            work_pixels: 0,
        };
        let output = transform_image(&original, placed, bounds, &mut limits).unwrap();
        assert_eq!(output, image::imageops::rotate90(&original));
        assert_eq!(limits.work_pixels, 8);
    }

    #[test]
    fn affine_image_resampling_cannot_exceed_bounds() {
        for (w, h) in [(1., 1.), (800_000., 10.), (200_000., 180_000.), (120., 31.)] {
            let (w, h) = bounded_dimensions(w, h, MAX_VECTOR_SIDE, MAX_VECTOR_PIXELS);
            assert!((1..=MAX_VECTOR_SIDE).contains(&w) && (1..=MAX_VECTOR_SIDE).contains(&h));
            assert!(u64::from(w) * u64::from(h) <= MAX_VECTOR_PIXELS);
        }
    }

    #[test]
    fn table_rules_are_suppressed_without_removing_cell_graphics() {
        let table = Rect {
            left: 100.,
            top: 100.,
            right: 400.,
            bottom: 200.,
        };
        let covers_rule = |commands: &[Command], fill| {
            let bounds = Rect::around(commands.iter().flat_map(|c| c.points())).unwrap();
            is_table_rule(commands, bounds, fill, table)
        };
        assert!(covers_rule(
            &[Command::Move((100., 140.)), Command::Line((400., 140.))],
            0
        ));
        assert!(!covers_rule(
            &[Command::Move((120., 140.)), Command::Line((140., 140.))],
            0
        ));
        assert!(!covers_rule(
            &[Command::Move((100., 100.)), Command::Line((400., 200.))],
            0
        ));
        let full = [
            Command::Move((100., 100.)),
            Command::Line((400., 100.)),
            Command::Line((400., 200.)),
            Command::Line((100., 200.)),
            Command::Close,
        ];
        assert!(covers_rule(&full, 0));
        assert!(!covers_rule(&full, 2));
    }

    #[test]
    fn jpeg_passthrough_rejects_metadata_and_trailing_bytes() {
        use image::ImageEncoder;
        let mut data = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut data)
            .write_image(&[100, 45, 65], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        assert!(standalone_jpeg(&data));
        let mut with_trailer = data.clone();
        with_trailer.push(0);
        assert!(!standalone_jpeg(&with_trailer));
        data.splice(2..2, [0xff, 0xe1, 0, 4, b'e', b'x']);
        assert!(!standalone_jpeg(&data));
    }
}
