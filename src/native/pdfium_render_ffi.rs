//! Minimal direct PDFium C API adapter for page rasterization.
//!
//! `pdfium-render` remains responsible for document inspection and native text.
//! This module exists solely because the high-level render configuration path is
//! not reliable with the PDFium 7881 runtime used by this project.

use std::{
    ffi::{c_char, c_int, c_uint, c_void, CString},
    path::{Path, PathBuf},
    ptr,
    sync::{Mutex, OnceLock},
};

use libloading::Library;
use thiserror::Error;

#[cfg(windows)]
const PDFIUM_DLL_PATH: &str = r"runtime\pdfium\7881\bin\pdfium.dll";
const FPDF_BITMAP_BGRA: c_int = 4;
const WHITE_BGRA: c_uint = 0xffff_ffff;

type Document = *mut c_void;
type Page = *mut c_void;
type Bitmap = *mut c_void;

type LoadDocument = unsafe extern "C" fn(*const c_char, *const c_char) -> Document;
type CloseDocument = unsafe extern "C" fn(Document);
type GetPageCount = unsafe extern "C" fn(Document) -> c_int;
type LoadPage = unsafe extern "C" fn(Document, c_int) -> Page;
type ClosePage = unsafe extern "C" fn(Page);
type GetPageWidthF = unsafe extern "C" fn(Page) -> f32;
type GetPageHeightF = unsafe extern "C" fn(Page) -> f32;
type CreateBitmap = unsafe extern "C" fn(c_int, c_int, c_int, *mut c_void, c_int) -> Bitmap;
type DestroyBitmap = unsafe extern "C" fn(Bitmap);
type FillRect = unsafe extern "C" fn(Bitmap, c_int, c_int, c_int, c_int, c_uint);
type RenderPageBitmap =
    unsafe extern "C" fn(Bitmap, Page, c_int, c_int, c_int, c_int, c_int, c_int);
type GetBuffer = unsafe extern "C" fn(Bitmap) -> *mut c_void;
type GetStride = unsafe extern "C" fn(Bitmap) -> c_int;
type GetBitmapWidth = unsafe extern "C" fn(Bitmap) -> c_int;
type GetBitmapHeight = unsafe extern "C" fn(Bitmap) -> c_int;
type GetBitmapFormat = unsafe extern "C" fn(Bitmap) -> c_int;
type GetLastError = unsafe extern "C" fn() -> c_uint;

/// A PDFium-rendered BGRA bitmap. `bgra` contains `height` rows of exactly
/// `stride` bytes, including any PDFium alignment padding at the end of a row.
#[derive(Debug)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub bgra: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum PdfRenderError {
    #[error("direct PDFium rendering is configured only for Windows")]
    UnsupportedPlatform,
    #[error("PDF input file not found: {0}")]
    InputMissing(PathBuf),
    #[error("target width must be positive, got {0}")]
    InvalidTargetWidth(u32),
    #[error("invalid PDF render region: {0}")]
    InvalidRenderRegion(String),
    #[error("PDF path contains an interior NUL byte: {0}")]
    InvalidPath(PathBuf),
    #[error("PDFium render lock was poisoned by an earlier failure")]
    RenderLockPoisoned,
    #[error("failed to load PDFium DLL {path}: {source}")]
    LoadLibrary {
        path: PathBuf,
        #[source]
        source: libloading::Error,
    },
    #[error("PDFium DLL is missing required symbol {symbol}: {source}")]
    MissingSymbol {
        symbol: &'static str,
        #[source]
        source: libloading::Error,
    },
    #[error("PDFium could not open {path} (FPDF error {code})")]
    LoadDocument { path: PathBuf, code: u32 },
    #[error("page index {page_index} is out of range for {page_count} pages")]
    PageIndexOutOfRange {
        page_index: usize,
        page_count: usize,
    },
    #[error("PDFium could not load page {page_index} (FPDF error {code})")]
    LoadPage { page_index: usize, code: u32 },
    #[error("invalid PDF page dimensions: {width_points} x {height_points} points")]
    InvalidPageDimensions {
        width_points: f32,
        height_points: f32,
    },
    #[error("rendered bitmap dimensions overflow: {width} x {height}")]
    BitmapDimensionsOverflow { width: u32, height: u32 },
    #[error("FPDFBitmap_CreateEx returned null (FPDF error {code})")]
    CreateBitmap { code: u32 },
    #[error("PDFium returned bitmap format {actual}; expected BGRA ({expected})")]
    UnexpectedBitmapFormat { actual: i32, expected: i32 },
    #[error("PDFium returned invalid bitmap dimensions {width} x {height}")]
    InvalidBitmapDimensions { width: i32, height: i32 },
    #[error("PDFium returned invalid bitmap stride {stride}; packed BGRA row needs {packed_row_bytes} bytes")]
    InvalidStride {
        stride: i32,
        packed_row_bytes: usize,
    },
    #[error("PDFium returned a null bitmap buffer")]
    NullBitmapBuffer,
    #[error("rendered bitmap size overflows usize")]
    BitmapSizeOverflow,
}

#[derive(Clone, Copy)]
struct Api {
    load_document: LoadDocument,
    close_document: CloseDocument,
    get_page_count: GetPageCount,
    load_page: LoadPage,
    close_page: ClosePage,
    get_page_width_f: GetPageWidthF,
    get_page_height_f: GetPageHeightF,
    create_bitmap: CreateBitmap,
    destroy_bitmap: DestroyBitmap,
    fill_rect: FillRect,
    render_page_bitmap: RenderPageBitmap,
    get_buffer: GetBuffer,
    get_stride: GetStride,
    get_bitmap_width: GetBitmapWidth,
    get_bitmap_height: GetBitmapHeight,
    get_bitmap_format: GetBitmapFormat,
    get_last_error: GetLastError,
}

struct PdfiumLibrary {
    // The library must outlive all function pointers and PDFium handles.
    _library: Library,
    api: Api,
}

struct DocumentHandle {
    raw: Document,
    api: Api,
}

impl Drop for DocumentHandle {
    fn drop(&mut self) {
        // SAFETY: `raw` is a non-null document returned by FPDF_LoadDocument.
        unsafe { (self.api.close_document)(self.raw) };
    }
}

struct PageHandle {
    raw: Page,
    api: Api,
}

impl Drop for PageHandle {
    fn drop(&mut self) {
        // SAFETY: `raw` is a non-null page returned by FPDF_LoadPage.
        unsafe { (self.api.close_page)(self.raw) };
    }
}

struct BitmapHandle {
    raw: Bitmap,
    api: Api,
}

#[derive(Debug, Clone, Copy)]
struct RenderViewport {
    bitmap_width: c_int,
    bitmap_height: c_int,
    page_start_x: c_int,
    page_start_y: c_int,
    page_width: c_int,
    page_height: c_int,
}

impl Drop for BitmapHandle {
    fn drop(&mut self) {
        // SAFETY: `raw` is a non-null bitmap returned by FPDFBitmap_CreateEx.
        unsafe { (self.api.destroy_bitmap)(self.raw) };
    }
}

fn render_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Resolves the explicit, versioned runtime DLL shared by both PDFium bindings.
///
/// The preferred deployment mechanism is an explicit path passed through
/// `OcrOptions`. When no override is supplied, the resolver checks a sibling
/// `pdfium.dll` and a versioned `runtime\\pdfium\\7881\\bin\\pdfium.dll`
/// next to the executable before falling back to the development bundle. It
/// never searches global `PATH`.
pub(crate) fn pdfium_dll_path() -> Result<PathBuf, PdfRenderError> {
    #[cfg(windows)]
    {
        let development_bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(PDFIUM_DLL_PATH);
        let mut candidates = Vec::new();
        if let Ok(executable) = std::env::current_exe() {
            if let Some(directory) = executable.parent() {
                candidates.push(directory.join("pdfium.dll"));
                candidates.push(directory.join(PDFIUM_DLL_PATH));
            }
        }
        candidates.push(development_bundle.clone());

        Ok(candidates
            .into_iter()
            .find(|path| path.is_file())
            .unwrap_or(development_bundle))
    }

    #[cfg(not(windows))]
    {
        Err(PdfRenderError::UnsupportedPlatform)
    }
}

unsafe fn symbol<T: Copy>(
    library: &Library,
    name: &'static [u8],
    printable_name: &'static str,
) -> Result<T, PdfRenderError> {
    // SAFETY: the caller declares the exact C ABI signature associated with `name`.
    unsafe { library.get::<T>(name) }
        .map(|symbol| *symbol)
        .map_err(|source| PdfRenderError::MissingSymbol {
            symbol: printable_name,
            source,
        })
}

impl PdfiumLibrary {
    #[cfg(windows)]
    fn load(path: &Path) -> Result<Self, PdfRenderError> {
        // SAFETY: this is the project's explicit, versioned PDFium runtime path.
        let library =
            unsafe { Library::new(path) }.map_err(|source| PdfRenderError::LoadLibrary {
                path: path.to_path_buf(),
                source,
            })?;

        // SAFETY: all names and signatures below come from the PDFium public C API.
        let api = unsafe {
            Api {
                load_document: symbol(&library, b"FPDF_LoadDocument\0", "FPDF_LoadDocument")?,
                close_document: symbol(&library, b"FPDF_CloseDocument\0", "FPDF_CloseDocument")?,
                get_page_count: symbol(&library, b"FPDF_GetPageCount\0", "FPDF_GetPageCount")?,
                load_page: symbol(&library, b"FPDF_LoadPage\0", "FPDF_LoadPage")?,
                close_page: symbol(&library, b"FPDF_ClosePage\0", "FPDF_ClosePage")?,
                get_page_width_f: symbol(&library, b"FPDF_GetPageWidthF\0", "FPDF_GetPageWidthF")?,
                get_page_height_f: symbol(
                    &library,
                    b"FPDF_GetPageHeightF\0",
                    "FPDF_GetPageHeightF",
                )?,
                create_bitmap: symbol(&library, b"FPDFBitmap_CreateEx\0", "FPDFBitmap_CreateEx")?,
                destroy_bitmap: symbol(&library, b"FPDFBitmap_Destroy\0", "FPDFBitmap_Destroy")?,
                fill_rect: symbol(&library, b"FPDFBitmap_FillRect\0", "FPDFBitmap_FillRect")?,
                render_page_bitmap: symbol(
                    &library,
                    b"FPDF_RenderPageBitmap\0",
                    "FPDF_RenderPageBitmap",
                )?,
                get_buffer: symbol(&library, b"FPDFBitmap_GetBuffer\0", "FPDFBitmap_GetBuffer")?,
                get_stride: symbol(&library, b"FPDFBitmap_GetStride\0", "FPDFBitmap_GetStride")?,
                get_bitmap_width: symbol(
                    &library,
                    b"FPDFBitmap_GetWidth\0",
                    "FPDFBitmap_GetWidth",
                )?,
                get_bitmap_height: symbol(
                    &library,
                    b"FPDFBitmap_GetHeight\0",
                    "FPDFBitmap_GetHeight",
                )?,
                get_bitmap_format: symbol(
                    &library,
                    b"FPDFBitmap_GetFormat\0",
                    "FPDFBitmap_GetFormat",
                )?,
                get_last_error: symbol(&library, b"FPDF_GetLastError\0", "FPDF_GetLastError")?,
            }
        };

        Ok(Self {
            _library: library,
            api,
        })
    }
}

/// Renders a zero-indexed PDF page to a BGRA bitmap using direct PDFium C API.
///
/// The returned byte vector retains PDFium's row alignment. Consumers must use
/// `stride` rather than assuming that rows are tightly packed. The caller must
/// initialize PDFium first (the production path does so through `Pdfium::new()`)
/// and must keep that initialization alive for the render call.
pub fn render_pdf_page(
    pdf_path: &Path,
    page_index: usize,
    target_width: u32,
) -> Result<RenderedPage, PdfRenderError> {
    let pdfium_dll = pdfium_dll_path()?;
    render_pdf_page_with_dll(pdf_path, page_index, target_width, &pdfium_dll)
}

/// Renders a page through an explicitly supplied PDFium DLL. This keeps the
/// direct C render path deployable from an embedding application without
/// changing global process search paths.
pub fn render_pdf_page_with_dll(
    pdf_path: &Path,
    page_index: usize,
    target_width: u32,
    pdfium_dll: &Path,
) -> Result<RenderedPage, PdfRenderError> {
    #[cfg(not(windows))]
    {
        let _ = (pdf_path, page_index, target_width, pdfium_dll);
        return Err(PdfRenderError::UnsupportedPlatform);
    }

    #[cfg(windows)]
    {
        if target_width == 0 {
            return Err(PdfRenderError::InvalidTargetWidth(target_width));
        }

        render_pdf_page_viewport_with_dll(
            pdf_path,
            page_index,
            pdfium_dll,
            |page_width, page_height| {
                let bitmap_width = c_int::try_from(target_width).map_err(|_| {
                    PdfRenderError::BitmapDimensionsOverflow {
                        width: target_width,
                        height: 0,
                    }
                })?;
                let height_f64 =
                    f64::from(target_width) * f64::from(page_height) / f64::from(page_width);
                if !height_f64.is_finite() || height_f64 < 1.0 || height_f64 > f64::from(c_int::MAX)
                {
                    return Err(PdfRenderError::BitmapDimensionsOverflow {
                        width: target_width,
                        height: height_f64.clamp(0.0, f64::from(u32::MAX)) as u32,
                    });
                }
                let bitmap_height = height_f64.round() as c_int;
                Ok(RenderViewport {
                    bitmap_width,
                    bitmap_height,
                    page_start_x: 0,
                    page_start_y: 0,
                    page_width: bitmap_width,
                    page_height: bitmap_height,
                })
            },
        )
    }
}

/// Renders only one pixel-aligned region of a zero-indexed PDF page.
///
/// `full_width` and `full_height` describe the virtual full-page render at the
/// requested density. The returned bitmap contains only the requested region;
/// PDFium clips the translated page directly into that smaller bitmap.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_pdf_page_region_with_dll(
    pdf_path: &Path,
    page_index: usize,
    full_width: u32,
    full_height: u32,
    region_x: u32,
    region_y: u32,
    region_width: u32,
    region_height: u32,
    pdfium_dll: &Path,
) -> Result<RenderedPage, PdfRenderError> {
    #[cfg(not(windows))]
    {
        let _ = (
            pdf_path,
            page_index,
            full_width,
            full_height,
            region_x,
            region_y,
            region_width,
            region_height,
            pdfium_dll,
        );
        return Err(PdfRenderError::UnsupportedPlatform);
    }

    #[cfg(windows)]
    {
        if full_width == 0 || full_height == 0 || region_width == 0 || region_height == 0 {
            return Err(PdfRenderError::InvalidRenderRegion(format!(
                "full page {full_width}x{full_height}, region {region_width}x{region_height}"
            )));
        }
        let region_right = region_x.checked_add(region_width).ok_or_else(|| {
            PdfRenderError::InvalidRenderRegion("region X extent overflow".into())
        })?;
        let region_bottom = region_y.checked_add(region_height).ok_or_else(|| {
            PdfRenderError::InvalidRenderRegion("region Y extent overflow".into())
        })?;
        if region_right > full_width || region_bottom > full_height {
            return Err(PdfRenderError::InvalidRenderRegion(format!(
                "region ({region_x}, {region_y}, {region_width}, {region_height}) exceeds full page {full_width}x{full_height}"
            )));
        }

        let bitmap_width = c_int::try_from(region_width).map_err(|_| {
            PdfRenderError::BitmapDimensionsOverflow {
                width: region_width,
                height: region_height,
            }
        })?;
        let bitmap_height = c_int::try_from(region_height).map_err(|_| {
            PdfRenderError::BitmapDimensionsOverflow {
                width: region_width,
                height: region_height,
            }
        })?;
        let page_width =
            c_int::try_from(full_width).map_err(|_| PdfRenderError::BitmapDimensionsOverflow {
                width: full_width,
                height: full_height,
            })?;
        let page_height =
            c_int::try_from(full_height).map_err(|_| PdfRenderError::BitmapDimensionsOverflow {
                width: full_width,
                height: full_height,
            })?;
        let page_start_x = c_int::try_from(region_x)
            .ok()
            .and_then(c_int::checked_neg)
            .ok_or_else(|| {
                PdfRenderError::InvalidRenderRegion("region X offset overflow".into())
            })?;
        let page_start_y = c_int::try_from(region_y)
            .ok()
            .and_then(c_int::checked_neg)
            .ok_or_else(|| {
                PdfRenderError::InvalidRenderRegion("region Y offset overflow".into())
            })?;

        render_pdf_page_viewport_with_dll(pdf_path, page_index, pdfium_dll, move |_, _| {
            Ok(RenderViewport {
                bitmap_width,
                bitmap_height,
                page_start_x,
                page_start_y,
                page_width,
                page_height,
            })
        })
    }
}

#[cfg(windows)]
fn render_pdf_page_viewport_with_dll(
    pdf_path: &Path,
    page_index: usize,
    pdfium_dll: &Path,
    viewport_for_page: impl FnOnce(f32, f32) -> Result<RenderViewport, PdfRenderError>,
) -> Result<RenderedPage, PdfRenderError> {
    if !pdf_path.is_file() {
        return Err(PdfRenderError::InputMissing(pdf_path.to_path_buf()));
    }

    let _guard = render_lock()
        .lock()
        .map_err(|_| PdfRenderError::RenderLockPoisoned)?;
    let library = PdfiumLibrary::load(pdfium_dll)?;
    let input = CString::new(pdf_path.to_string_lossy().as_bytes())
        .map_err(|_| PdfRenderError::InvalidPath(pdf_path.to_path_buf()))?;

    // SAFETY: `input` remains live while PDFium consumes its NUL-terminated path.
    let document_raw = unsafe { (library.api.load_document)(input.as_ptr(), ptr::null()) };
    if document_raw.is_null() {
        // SAFETY: queried immediately after the failed PDFium call.
        let code = unsafe { (library.api.get_last_error)() };
        return Err(PdfRenderError::LoadDocument {
            path: pdf_path.to_path_buf(),
            code,
        });
    }
    let document = DocumentHandle {
        raw: document_raw,
        api: library.api,
    };

    // SAFETY: `document.raw` is a valid PDFium document handle.
    let page_count = unsafe { (library.api.get_page_count)(document.raw) };
    let page_count = usize::try_from(page_count).unwrap_or(0);
    if page_index >= page_count {
        return Err(PdfRenderError::PageIndexOutOfRange {
            page_index,
            page_count,
        });
    }
    let page_index_i32 =
        c_int::try_from(page_index).map_err(|_| PdfRenderError::PageIndexOutOfRange {
            page_index,
            page_count,
        })?;

    // SAFETY: page_index was validated against the document page count.
    let page_raw = unsafe { (library.api.load_page)(document.raw, page_index_i32) };
    if page_raw.is_null() {
        // SAFETY: queried immediately after the failed PDFium call.
        let code = unsafe { (library.api.get_last_error)() };
        return Err(PdfRenderError::LoadPage { page_index, code });
    }
    let page = PageHandle {
        raw: page_raw,
        api: library.api,
    };

    // SAFETY: `page.raw` is a valid PDFium page handle.
    let page_width = unsafe { (library.api.get_page_width_f)(page.raw) };
    // SAFETY: `page.raw` is a valid PDFium page handle.
    let page_height = unsafe { (library.api.get_page_height_f)(page.raw) };
    if !(page_width.is_finite() && page_height.is_finite() && page_width > 0.0 && page_height > 0.0)
    {
        return Err(PdfRenderError::InvalidPageDimensions {
            width_points: page_width,
            height_points: page_height,
        });
    }
    let viewport = viewport_for_page(page_width, page_height)?;

    // SAFETY: a null buffer asks PDFium to allocate an internal BGRA bitmap.
    let bitmap_raw = unsafe {
        (library.api.create_bitmap)(
            viewport.bitmap_width,
            viewport.bitmap_height,
            FPDF_BITMAP_BGRA,
            ptr::null_mut(),
            0,
        )
    };
    if bitmap_raw.is_null() {
        // SAFETY: queried immediately after the failed PDFium call.
        let code = unsafe { (library.api.get_last_error)() };
        return Err(PdfRenderError::CreateBitmap { code });
    }
    let bitmap = BitmapHandle {
        raw: bitmap_raw,
        api: library.api,
    };

    // SAFETY: the target rectangle exactly covers the valid bitmap.
    unsafe {
        (library.api.fill_rect)(
            bitmap.raw,
            0,
            0,
            viewport.bitmap_width,
            viewport.bitmap_height,
            WHITE_BGRA,
        )
    };
    // SAFETY: page and bitmap are valid. PDFium clips the translated virtual
    // full-page viewport into the allocated bitmap. Rotation and flags remain
    // zero, matching the proven production render configuration.
    unsafe {
        (library.api.render_page_bitmap)(
            bitmap.raw,
            page.raw,
            viewport.page_start_x,
            viewport.page_start_y,
            viewport.page_width,
            viewport.page_height,
            0,
            0,
        )
    };

    // SAFETY: `bitmap.raw` remains valid until the local RAII handle drops.
    let width = unsafe { (library.api.get_bitmap_width)(bitmap.raw) };
    // SAFETY: `bitmap.raw` remains valid until the local RAII handle drops.
    let height = unsafe { (library.api.get_bitmap_height)(bitmap.raw) };
    // SAFETY: `bitmap.raw` remains valid until the local RAII handle drops.
    let stride = unsafe { (library.api.get_stride)(bitmap.raw) };
    // SAFETY: `bitmap.raw` remains valid until the local RAII handle drops.
    let format = unsafe { (library.api.get_bitmap_format)(bitmap.raw) };
    if format != FPDF_BITMAP_BGRA {
        return Err(PdfRenderError::UnexpectedBitmapFormat {
            actual: format,
            expected: FPDF_BITMAP_BGRA,
        });
    }
    if width <= 0 || height <= 0 {
        return Err(PdfRenderError::InvalidBitmapDimensions { width, height });
    }
    let width_u32 = u32::try_from(width).expect("positive i32 fits u32");
    let height_u32 = u32::try_from(height).expect("positive i32 fits u32");
    let packed_row_bytes = usize::try_from(width_u32)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or(PdfRenderError::BitmapSizeOverflow)?;
    let stride_usize = usize::try_from(stride).map_err(|_| PdfRenderError::InvalidStride {
        stride,
        packed_row_bytes,
    })?;
    if stride_usize < packed_row_bytes {
        return Err(PdfRenderError::InvalidStride {
            stride,
            packed_row_bytes,
        });
    }
    let byte_len = stride_usize
        .checked_mul(usize::try_from(height_u32).expect("u32 fits usize on Windows x64"))
        .ok_or(PdfRenderError::BitmapSizeOverflow)?;
    // SAFETY: PDFium allocated this bitmap and reports `stride * height` bytes.
    let buffer = unsafe { (library.api.get_buffer)(bitmap.raw).cast::<u8>() };
    if buffer.is_null() {
        return Err(PdfRenderError::NullBitmapBuffer);
    }
    // SAFETY: `buffer` is valid for the PDFium-reported bitmap extent while
    // `bitmap` is alive; immediately clone it into Rust-owned memory.
    let bgra = unsafe { std::slice::from_raw_parts(buffer, byte_len).to_vec() };

    Ok(RenderedPage {
        width: width_u32,
        height: height_u32,
        stride: stride_usize,
        bgra,
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::{pdfium_dll_path, render_pdf_page, render_pdf_page_region_with_dll, RenderedPage};
    use crate::native::pdfium_test_lock;
    use pdfium_render::prelude::*;
    use std::{path::PathBuf, sync::OnceLock};

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/fixtures/w3c-dummy.pdf")
    }

    fn initialize_pdfium() {
        static PDFIUM: OnceLock<Pdfium> = OnceLock::new();
        PDFIUM.get_or_init(|| {
            let path = pdfium_dll_path().expect("Windows PDFium runtime path");
            let bindings = Pdfium::bind_to_library(&path).expect("bind versioned PDFium DLL");
            Pdfium::new(bindings)
        });
    }

    fn assert_not_all_white(page: &RenderedPage) {
        assert!(page
            .bgra
            .chunks_exact(4)
            .any(|pixel| pixel[0] < 255 || pixel[1] < 255 || pixel[2] < 255));
    }

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        pdfium_test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn renders_control_page_zero() {
        let _guard = test_guard();
        initialize_pdfium();
        let page = render_pdf_page(&fixture(), 0, 800).expect("render control PDF page zero");
        assert_eq!(page.width, 800);
        assert!(page.height > 0);
        assert_eq!(page.bgra.len(), page.stride * page.height as usize);
    }

    #[test]
    fn rejects_invalid_page_index() {
        let _guard = test_guard();
        initialize_pdfium();
        let error = render_pdf_page(&fixture(), 1, 800).expect_err("page one must be out of range");
        assert!(error.to_string().contains("out of range"));
    }

    #[test]
    fn returned_stride_is_large_enough_for_bgra() {
        let _guard = test_guard();
        initialize_pdfium();
        let page = render_pdf_page(&fixture(), 0, 800).expect("render control PDF page zero");
        assert!(page.stride >= page.width as usize * 4);
    }

    #[test]
    fn control_page_is_not_all_white() {
        let _guard = test_guard();
        initialize_pdfium();
        let page = render_pdf_page(&fixture(), 0, 800).expect("render control PDF page zero");
        assert_not_all_white(&page);
    }

    #[test]
    fn cleanup_after_error_allows_next_render() {
        let _guard = test_guard();
        initialize_pdfium();
        assert!(render_pdf_page(&fixture(), 1, 800).is_err());
        let page = render_pdf_page(&fixture(), 0, 800).expect("render after prior error");
        assert_not_all_white(&page);
    }

    #[test]
    fn direct_region_render_matches_full_page_crop() {
        let _guard = test_guard();
        initialize_pdfium();
        let full = render_pdf_page(&fixture(), 0, 800).expect("render full control page");
        let x = 80;
        let y = 60;
        let width = 400;
        let height = (full.height / 2).max(1);
        let region = render_pdf_page_region_with_dll(
            &fixture(),
            0,
            full.width,
            full.height,
            x,
            y,
            width,
            height,
            &pdfium_dll_path().expect("PDFium path"),
        )
        .expect("render clipped control page");

        assert_eq!((region.width, region.height), (width, height));
        let mut absolute_difference = 0_u64;
        let mut compared_channels = 0_u64;
        let mut dark_pixels = 0_u64;
        for row in 0..height as usize {
            let full_start = (y as usize + row) * full.stride + x as usize * 4;
            let region_start = row * region.stride;
            let region_row = &region.bgra[region_start..region_start + width as usize * 4];
            let full_row = &full.bgra[full_start..full_start + width as usize * 4];
            for (region_pixel, full_pixel) in
                region_row.chunks_exact(4).zip(full_row.chunks_exact(4))
            {
                dark_pixels += u64::from(region_pixel[..3].iter().any(|value| *value < 250));
                for channel in 0..3 {
                    absolute_difference +=
                        u64::from(region_pixel[channel].abs_diff(full_pixel[channel]));
                    compared_channels += 1;
                }
            }
        }
        let mean_difference = absolute_difference as f64 / compared_channels as f64;
        assert!(dark_pixels > 0, "ROI must contain rendered page content");
        assert!(
            mean_difference < 3.0,
            "direct ROI and full-page crop differ too much: mean channel error {mean_difference}"
        );
    }
}
