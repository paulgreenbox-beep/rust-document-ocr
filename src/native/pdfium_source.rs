//! Pdfium-backed page source for real PDF rendering.

use std::path::{Path, PathBuf};

use image::GrayImage;
use pdfium_render::prelude::*;

use crate::dto::{BoundingBox, Word};
use crate::native::pdfium_render_ffi::{
    pdfium_dll_path, render_pdf_page_region_with_dll, render_pdf_page_with_dll,
    RenderedPage as FfiRenderedPage,
};
use crate::page_analysis::{NativeTextLayer, NativeTextLayout, PageOrientation};
use crate::page_source::{
    dimensions_within_pixel_limit, PageSource, RenderedPage, RenderedPageRegion,
};

/// A sequential PDF page source backed by the system Pdfium library.
pub struct PdfiumPageSource {
    pdfium: Pdfium,
    path: PathBuf,
    pdfium_dll: PathBuf,
    total: u32,
    dpi: f64,
}

/// One direct-C PDFium render clipped to a normalized page region.
pub(crate) struct RenderedRegion {
    pub image: GrayImage,
    pub dpi: u32,
    pub full_width: u32,
    pub full_height: u32,
    pub x: u32,
    pub y: u32,
}

const BOUNDED_TEXT_CONTROL_BOUNDARY_THRESHOLD: f32 = 0.85;
const SPARSE_CONTROL_RUN_COUNT_THRESHOLD: usize = 2;
const SPARSE_CONTROL_RUN_DENSITY_THRESHOLD: f32 = 0.025;
const CONTROL_ANGLE_TOLERANCE_RADIANS: f32 = 5.0_f32.to_radians();
const CONTROL_LINE_CHANGE_THRESHOLD: f32 = 0.65;
const CONTROL_WORD_GAP_THRESHOLD: f32 = 0.70;
const STACKED_CONTROL_MIN_RUNS: usize = 8;
const STACKED_CONTROL_MIN_GEOMETRY_COVERAGE: f32 = 0.75;
const STACKED_CONTROL_CORE_MIN_GEOMETRY_COVERAGE: f32 = 0.80;
const STACKED_CONTROL_MEDIAN_CROSS_THRESHOLD: f32 = 1.50;
const STACKED_CONTROL_ORIGIN_ADVANCE_Q75_THRESHOLD: f32 = 0.25;
const STACKED_CONTROL_HIGH_TAIL_MEDIAN_CROSS_THRESHOLD: f32 = 1.25;
const STACKED_CONTROL_HIGH_TAIL_CROSS_P90_THRESHOLD: f32 = 8.0;
const STACKED_CONTROL_MODERATE_TAIL_MEDIAN_CROSS_THRESHOLD: f32 = 1.40;
const STACKED_CONTROL_MODERATE_TAIL_CROSS_P90_MIN: f32 = 5.5;
const STACKED_CONTROL_MODERATE_TAIL_CROSS_P90_MAX: f32 = 8.0;
const STACKED_CONTROL_MODERATE_TAIL_ORIGIN_Q75_THRESHOLD: f32 = -0.5;

#[derive(Debug, Clone, Copy)]
struct PdfTextGlyph {
    value: char,
    origin: Option<(f32, f32)>,
    angle_radians: Option<f32>,
    bounds: Option<PdfWordBounds>,
}

#[derive(Debug, Clone, Copy)]
struct ControlRunGeometry {
    cross_line_distance: f32,
    along_gap: f32,
    origin_advance: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct ControlRunProfile {
    stacked_backtracking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PdfTextBoundary {
    None,
    Word,
    Line,
}

fn pdf_control_runs_are_boundaries(
    bounded_characters: usize,
    total_characters: usize,
    insertable_control_runs: usize,
) -> bool {
    let bounded_coverage = bounded_characters as f32 / total_characters.max(1) as f32;
    let control_run_density = insertable_control_runs as f32 / total_characters.max(1) as f32;
    bounded_coverage < BOUNDED_TEXT_CONTROL_BOUNDARY_THRESHOLD
        || insertable_control_runs <= SPARSE_CONTROL_RUN_COUNT_THRESHOLD
        || control_run_density < SPARSE_CONTROL_RUN_DENSITY_THRESHOLD
}

fn insertable_pdf_control_run_count(glyphs: &[PdfTextGlyph]) -> usize {
    let mut count = 0usize;
    let mut index = 0usize;
    while index < glyphs.len() {
        if !glyphs[index].value.is_control() {
            index += 1;
            continue;
        }

        let run_start = index;
        while index < glyphs.len() && glyphs[index].value.is_control() {
            index += 1;
        }
        if run_start > 0
            && index < glyphs.len()
            && !glyphs[run_start - 1].value.is_whitespace()
            && !glyphs[index].value.is_whitespace()
        {
            count += 1;
        }
    }
    count
}

fn angular_difference(left: f32, right: f32) -> f32 {
    let difference = (left - right).rem_euclid(std::f32::consts::PI);
    difference.min(std::f32::consts::PI - difference)
}

fn projected_extent(bounds: PdfWordBounds, axis_x: f32, axis_y: f32) -> (f32, f32) {
    let values = [
        bounds.left * axis_x + bounds.bottom * axis_y,
        bounds.left * axis_x + bounds.top * axis_y,
        bounds.right * axis_x + bounds.bottom * axis_y,
        bounds.right * axis_x + bounds.top * axis_y,
    ];
    (
        values.iter().copied().fold(f32::INFINITY, f32::min),
        values.iter().copied().fold(f32::NEG_INFINITY, f32::max),
    )
}

fn control_run_geometry(previous: PdfTextGlyph, next: PdfTextGlyph) -> Option<ControlRunGeometry> {
    let (previous_origin, next_origin, previous_angle, next_angle, previous_bounds, next_bounds) = (
        previous.origin?,
        next.origin?,
        previous.angle_radians?,
        next.angle_radians?,
        previous.bounds?,
        next.bounds?,
    );
    if !previous_angle.is_finite()
        || !next_angle.is_finite()
        || angular_difference(previous_angle, next_angle) > CONTROL_ANGLE_TOLERANCE_RADIANS
    {
        return None;
    }

    let baseline_x = previous_angle.cos();
    let baseline_y = previous_angle.sin();
    let normal_x = -baseline_y;
    let normal_y = baseline_x;
    let delta_x = next_origin.0 - previous_origin.0;
    let delta_y = next_origin.1 - previous_origin.1;
    let cross_line_distance = (delta_x * normal_x + delta_y * normal_y).abs();
    let previous_cross = projected_extent(previous_bounds, normal_x, normal_y);
    let next_cross = projected_extent(next_bounds, normal_x, normal_y);
    let line_scale = (previous_cross.1 - previous_cross.0)
        .max(next_cross.1 - next_cross.0)
        .max(1.0);
    let previous_along = projected_extent(previous_bounds, baseline_x, baseline_y);
    let next_along = projected_extent(next_bounds, baseline_x, baseline_y);
    let geometry = ControlRunGeometry {
        cross_line_distance: cross_line_distance / line_scale,
        along_gap: (next_along.0 - previous_along.1) / line_scale,
        origin_advance: (delta_x * baseline_x + delta_y * baseline_y) / line_scale,
    };
    (geometry.cross_line_distance.is_finite()
        && geometry.along_gap.is_finite()
        && geometry.origin_advance.is_finite())
    .then_some(geometry)
}

fn percentile(values: &mut [f32], fraction: f32) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let index = ((values.len() - 1) as f32 * fraction).round() as usize;
    values.get(index).copied()
}

fn control_run_profile(glyphs: &[PdfTextGlyph]) -> ControlRunProfile {
    let insertable_runs = insertable_pdf_control_run_count(glyphs);
    let control_density = insertable_runs as f32 / glyphs.len().max(1) as f32;
    if insertable_runs < STACKED_CONTROL_MIN_RUNS
        || control_density < SPARSE_CONTROL_RUN_DENSITY_THRESHOLD
    {
        return ControlRunProfile::default();
    }

    let mut geometries = Vec::with_capacity(insertable_runs);
    let mut index = 0usize;
    while index < glyphs.len() {
        if !glyphs[index].value.is_control() {
            index += 1;
            continue;
        }

        let run_start = index;
        while index < glyphs.len() && glyphs[index].value.is_control() {
            index += 1;
        }
        if run_start == 0
            || index >= glyphs.len()
            || glyphs[run_start - 1].value.is_whitespace()
            || glyphs[index].value.is_whitespace()
        {
            continue;
        }

        let previous = glyphs[..run_start]
            .iter()
            .rev()
            .find(|glyph| !glyph.value.is_control() && !glyph.value.is_whitespace())
            .copied();
        let next = glyphs[index..]
            .iter()
            .find(|glyph| !glyph.value.is_control() && !glyph.value.is_whitespace())
            .copied();
        if let (Some(previous), Some(next)) = (previous, next) {
            if let Some(geometry) = control_run_geometry(previous, next) {
                geometries.push(geometry);
            }
        }
    }

    let geometry_coverage = geometries.len() as f32 / insertable_runs.max(1) as f32;
    if geometry_coverage < STACKED_CONTROL_MIN_GEOMETRY_COVERAGE {
        return ControlRunProfile::default();
    }

    let mut cross_distances = geometries
        .iter()
        .map(|geometry| geometry.cross_line_distance)
        .collect::<Vec<_>>();
    let mut origin_advances = geometries
        .iter()
        .map(|geometry| geometry.origin_advance)
        .collect::<Vec<_>>();
    let median_cross = percentile(&mut cross_distances, 0.50).unwrap_or_default();
    let cross_p90 = percentile(&mut cross_distances, 0.90).unwrap_or_default();
    let origin_advance_q75 = percentile(&mut origin_advances, 0.75).unwrap_or_default();

    let core_stacked_profile = geometry_coverage >= STACKED_CONTROL_CORE_MIN_GEOMETRY_COVERAGE
        && median_cross >= STACKED_CONTROL_MEDIAN_CROSS_THRESHOLD
        && origin_advance_q75 <= STACKED_CONTROL_ORIGIN_ADVANCE_Q75_THRESHOLD;
    let high_tail_stacked_profile = median_cross
        >= STACKED_CONTROL_HIGH_TAIL_MEDIAN_CROSS_THRESHOLD
        && cross_p90 >= STACKED_CONTROL_HIGH_TAIL_CROSS_P90_THRESHOLD
        && origin_advance_q75 <= STACKED_CONTROL_ORIGIN_ADVANCE_Q75_THRESHOLD;
    let moderate_tail_stacked_profile = median_cross
        >= STACKED_CONTROL_MODERATE_TAIL_MEDIAN_CROSS_THRESHOLD
        && (STACKED_CONTROL_MODERATE_TAIL_CROSS_P90_MIN
            ..STACKED_CONTROL_MODERATE_TAIL_CROSS_P90_MAX)
            .contains(&cross_p90)
        && origin_advance_q75 <= STACKED_CONTROL_MODERATE_TAIL_ORIGIN_Q75_THRESHOLD;

    ControlRunProfile {
        // CAD generators can emit one logical token as a stack of separate,
        // backtracking text objects. Their control runs have a dominant large
        // normal-axis jump without forward baseline progress. Treating those
        // runs as spaces fragments nearly every token. Both dimensions are
        // normalized by local glyph height, so this policy is DPI-independent.
        // Two conservative tail branches cover mixed CAD pages whose dominant
        // line pitch is hidden by a minority of very large jumps. They retain
        // the same no-forward-progress requirement and the global run/density
        // gates above; ordinary prose must not enter this profile.
        stacked_backtracking: core_stacked_profile
            || high_tail_stacked_profile
            || moderate_tail_stacked_profile,
    }
}

fn geometry_control_boundary(
    previous: PdfTextGlyph,
    next: PdfTextGlyph,
    profile: ControlRunProfile,
) -> Option<PdfTextBoundary> {
    let geometry = control_run_geometry(previous, next)?;
    if geometry.cross_line_distance > CONTROL_LINE_CHANGE_THRESHOLD {
        return Some(if profile.stacked_backtracking {
            PdfTextBoundary::None
        } else {
            PdfTextBoundary::Line
        });
    }

    Some(if geometry.along_gap > CONTROL_WORD_GAP_THRESHOLD {
        PdfTextBoundary::Word
    } else {
        PdfTextBoundary::None
    })
}

fn classify_control_run(
    glyphs: &[PdfTextGlyph],
    run_start: usize,
    run_end: usize,
    profile: ControlRunProfile,
    fallback_boundaries: bool,
) -> PdfTextBoundary {
    let previous = glyphs[..run_start]
        .iter()
        .rev()
        .find(|glyph| !glyph.value.is_control() && !glyph.value.is_whitespace())
        .copied();
    let next = glyphs[run_end..]
        .iter()
        .find(|glyph| !glyph.value.is_control() && !glyph.value.is_whitespace())
        .copied();

    match (previous, next) {
        (Some(previous), Some(next)) => geometry_control_boundary(previous, next, profile)
            .unwrap_or(if fallback_boundaries && !profile.stacked_backtracking {
                PdfTextBoundary::Word
            } else {
                PdfTextBoundary::None
            }),
        _ => PdfTextBoundary::None,
    }
}

fn resolve_pdf_control_runs(glyphs: &[PdfTextGlyph], fallback_boundaries: bool) -> String {
    let profile = control_run_profile(glyphs);
    let mut output = String::with_capacity(glyphs.len());
    let mut index = 0usize;
    while index < glyphs.len() {
        if !glyphs[index].value.is_control() {
            output.push(glyphs[index].value);
            index += 1;
            continue;
        }

        let run_start = index;
        while index < glyphs.len() && glyphs[index].value.is_control() {
            index += 1;
        }
        if output.is_empty() || index >= glyphs.len() {
            continue;
        }

        match classify_control_run(glyphs, run_start, index, profile, fallback_boundaries) {
            PdfTextBoundary::None => {}
            PdfTextBoundary::Word => {
                let output_has_separator =
                    output.chars().next_back().is_some_and(char::is_whitespace);
                let input_has_separator = glyphs
                    .get(index)
                    .is_some_and(|glyph| glyph.value.is_whitespace() && !glyph.value.is_control());
                if !output_has_separator && !input_has_separator {
                    output.push(' ');
                }
            }
            PdfTextBoundary::Line => {
                while output.ends_with([' ', '\t']) {
                    output.pop();
                }
                if !output.ends_with('\n') {
                    output.push('\n');
                }
            }
        }
    }
    output
}

fn native_words_from_glyphs(
    glyphs: &[PdfTextGlyph],
    fallback_boundaries: bool,
) -> Vec<RawNativeWord> {
    let profile = control_run_profile(glyphs);
    let mut raw_words = Vec::new();
    let mut builder = NativeWordBuilder::default();
    let mut suppress_geometry_split = false;
    let mut index = 0usize;

    while index < glyphs.len() {
        let glyph = glyphs[index];
        if glyph.value.is_control() {
            let run_start = index;
            while index < glyphs.len() && glyphs[index].value.is_control() {
                index += 1;
            }
            let boundary =
                classify_control_run(glyphs, run_start, index, profile, fallback_boundaries);
            if boundary == PdfTextBoundary::None {
                // CAD exporters can place control glyphs between the letters of
                // one logical word. Suppress the otherwise equivalent geometry
                // split for the next glyph so page text and positioned words
                // follow exactly the same boundary decision.
                suppress_geometry_split = true;
            } else {
                builder.flush_into(&mut raw_words);
                suppress_geometry_split = false;
            }
            continue;
        }
        if glyph.value.is_whitespace() {
            builder.flush_into(&mut raw_words);
            suppress_geometry_split = false;
            index += 1;
            continue;
        }

        if !suppress_geometry_split
            && glyph
                .bounds
                .as_ref()
                .is_some_and(|bounds| builder.should_split_before(bounds))
        {
            builder.flush_into(&mut raw_words);
        }
        builder.push(glyph.value, glyph.bounds);
        suppress_geometry_split = false;
        index += 1;
    }
    builder.flush_into(&mut raw_words);
    raw_words
}

impl PdfiumPageSource {
    /// Opens the system Pdfium library and inspects the PDF's page count.
    pub fn open(path: &Path, dpi: f64) -> Result<Self, Box<dyn std::error::Error>> {
        Self::open_with_pdfium_dll(path, dpi, None)
    }

    /// Opens a PDF with an optional explicit PDFium runtime DLL. The same DLL
    /// is later passed to the direct C render path, so the two bindings cannot
    /// accidentally resolve different versions through `PATH`.
    pub fn open_with_pdfium_dll(
        path: &Path,
        dpi: f64,
        pdfium_dll_override: Option<&Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if !path.is_file() {
            return Err(format!("PDF input file not found: {}", path.display()).into());
        }
        if !dpi.is_finite() || dpi <= 0.0 {
            return Err(format!("DPI must be a finite positive value, got {dpi}").into());
        }

        let pdfium_dll = match pdfium_dll_override {
            Some(path) => path.to_path_buf(),
            None => pdfium_dll_path()?,
        };
        if !pdfium_dll.is_file() {
            return Err(format!("PDFium DLL not found: {}", pdfium_dll.display()).into());
        }
        let bindings = Pdfium::bind_to_library(&pdfium_dll).map_err(|e| {
            format!(
                "Failed to load versioned Pdfium library {}: {e}.",
                pdfium_dll.display()
            )
        })?;
        let pdfium = Pdfium::new(bindings);
        let doc = pdfium.load_pdf_from_file(path, None)?;
        let total = doc.pages().len() as u32;
        drop(doc);

        Ok(Self {
            pdfium,
            path: path.to_path_buf(),
            pdfium_dll,
            total,
            dpi,
        })
    }

    /// Extracts the native text layer using `pdfium-render`. This remains
    /// separate from the direct C render path and is useful to host
    /// applications that want to inspect a PDF before OCR.
    pub fn extract_native_text(&self, page_num: u32) -> Result<String, Box<dyn std::error::Error>> {
        Ok(self.extract_native_text_layer(page_num)?.text)
    }

    /// Extracts both PDFium's character count and the UTF-8 text layer for
    /// automatic NativeText/OCR selection. Rendering remains intentionally in
    /// the separate direct C API path.
    pub fn extract_native_text_layer(
        &self,
        page_num: u32,
    ) -> Result<NativeTextLayer, Box<dyn std::error::Error>> {
        if page_num == 0 || page_num > self.total {
            return Err(format!("page {page_num} out of range (total {})", self.total).into());
        }
        let page_index = i32::try_from(page_num - 1)
            .map_err(|_| format!("page index {page_num} exceeds Pdfium's supported range"))?;
        let doc = self.pdfium.load_pdf_from_file(&self.path, None)?;
        let page = doc.pages().get(page_index)?;
        let page_text = page.text()?;
        let pdfium_character_count = usize::try_from(page_text.len()).map_err(|_| {
            format!("PDFium returned a negative native text character count for page {page_num}")
        })?;
        // PdfPageText::all() is implemented as inside_rect(page_size()) in
        // pdfium-render 0.9.3. CAD generators frequently place searchable
        // glyph origins just outside that rectangle even though PDFium counts
        // them in FPDFText_CountChars(). Iterating the complete character list
        // avoids silently losing large parts of an otherwise valid layer.
        //
        // Preserve the count with U+FFFD when a glyph has no Unicode mapping;
        // the quality gate will then reject the incomplete layer rather than
        // pretending it is complete.
        let glyphs = page_text
            .chars()
            .iter()
            .map(|character| {
                let bounds = character
                    .loose_bounds()
                    .or_else(|_| character.tight_bounds())
                    .ok()
                    .map(|bounds| PdfWordBounds {
                        left: bounds.left().value,
                        bottom: bounds.bottom().value,
                        right: bounds.right().value,
                        top: bounds.top().value,
                    })
                    .filter(PdfWordBounds::is_valid);
                let origin = character.origin().ok().map(|(x, y)| (x.value, y.value));
                PdfTextGlyph {
                    value: character.unicode_char().unwrap_or('\u{fffd}'),
                    origin,
                    angle_radians: character.angle_radians().ok(),
                    bounds,
                }
            })
            .collect::<Vec<_>>();
        let bounded_character_count = page_text.all().chars().count();
        let fallback_boundaries = pdf_control_runs_are_boundaries(
            bounded_character_count,
            pdfium_character_count,
            insertable_pdf_control_run_count(&glyphs),
        );
        let text = resolve_pdf_control_runs(&glyphs, fallback_boundaries);
        Ok(NativeTextLayer::new(text, pdfium_character_count))
    }

    /// Extracts geometry-only native text evidence for the table-routing
    /// preflight. The text itself remains owned by [`NativeTextLayer`]; this
    /// method intentionally retains only block/alignment counts.
    pub fn extract_native_text_layout(
        &self,
        page_num: u32,
    ) -> Result<NativeTextLayout, Box<dyn std::error::Error>> {
        if page_num == 0 || page_num > self.total {
            return Err(format!("page {page_num} out of range (total {})", self.total).into());
        }
        let page_index = i32::try_from(page_num - 1)
            .map_err(|_| format!("page index {page_num} exceeds Pdfium's supported range"))?;
        let doc = self.pdfium.load_pdf_from_file(&self.path, None)?;
        let page = doc.pages().get(page_index)?;
        let page_text = page.text()?;
        let segments = page_text.segments();
        let bounds = segments.iter().map(|segment| {
            let bounds = segment.bounds();
            (
                bounds.left().value,
                bounds.bottom().value,
                bounds.right().value,
                bounds.top().value,
            )
        });
        Ok(NativeTextLayout::from_block_bounds(bounds))
    }

    /// Extracts whitespace-delimited native words and converts their PDF-space
    /// glyph boxes into the exact pixel coordinate system of a rendered page.
    pub fn extract_native_words_for_rendered_page(
        &self,
        page_num: u32,
        width: u32,
        height: u32,
    ) -> Result<Vec<Word>, Box<dyn std::error::Error>> {
        if page_num == 0 || page_num > self.total {
            return Err(format!("page {page_num} out of range (total {})", self.total).into());
        }
        let render_width = i32::try_from(width)
            .map_err(|_| format!("render width {width} exceeds PDFium's coordinate range"))?;
        let render_height = i32::try_from(height)
            .map_err(|_| format!("render height {height} exceeds PDFium's coordinate range"))?;
        if render_width == 0 || render_height == 0 {
            return Err("native-word target bitmap must be non-empty".into());
        }

        let page_index = i32::try_from(page_num - 1)
            .map_err(|_| format!("page index {page_num} exceeds Pdfium's supported range"))?;
        let doc = self.pdfium.load_pdf_from_file(&self.path, None)?;
        let page = doc.pages().get(page_index)?;
        let page_text = page.text()?;
        let glyphs = page_text
            .chars()
            .iter()
            .map(|character| {
                let bounds = character
                    .loose_bounds()
                    .or_else(|_| character.tight_bounds())
                    .ok()
                    .map(|bounds| PdfWordBounds {
                        left: bounds.left().value,
                        bottom: bounds.bottom().value,
                        right: bounds.right().value,
                        top: bounds.top().value,
                    })
                    .filter(PdfWordBounds::is_valid);
                let origin = character.origin().ok().map(|(x, y)| (x.value, y.value));
                PdfTextGlyph {
                    value: character.unicode_char().unwrap_or('\u{fffd}'),
                    origin,
                    angle_radians: character.angle_radians().ok(),
                    bounds,
                }
            })
            .collect::<Vec<_>>();
        let bounded_character_count = page_text.all().chars().count();
        let fallback_boundaries = pdf_control_runs_are_boundaries(
            bounded_character_count,
            glyphs.len(),
            insertable_pdf_control_run_count(&glyphs),
        );
        let raw_words = native_words_from_glyphs(&glyphs, fallback_boundaries);

        let render_config = PdfRenderConfig::new().set_fixed_size(render_width, render_height);
        let mut words = Vec::with_capacity(raw_words.len());
        for raw in raw_words {
            let Some(bounds) = raw.bounds else {
                continue;
            };
            let corners = [
                (bounds.left, bounds.bottom),
                (bounds.left, bounds.top),
                (bounds.right, bounds.bottom),
                (bounds.right, bounds.top),
            ];
            let mut xs = [0_i32; 4];
            let mut ys = [0_i32; 4];
            for (index, (x, y)) in corners.into_iter().enumerate() {
                let (pixel_x, pixel_y) =
                    page.points_to_pixels(PdfPoints::new(x), PdfPoints::new(y), &render_config)?;
                xs[index] = pixel_x;
                ys[index] = pixel_y;
            }
            let x0 = xs.into_iter().min().unwrap_or(0).clamp(0, render_width);
            let x1 = xs.into_iter().max().unwrap_or(0).clamp(0, render_width);
            let y0 = ys.into_iter().min().unwrap_or(0).clamp(0, render_height);
            let y1 = ys.into_iter().max().unwrap_or(0).clamp(0, render_height);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            words.push(Word::new(
                raw.text,
                BoundingBox::new(f64::from(x0), f64::from(y0), f64::from(x1), f64::from(y1)),
                1.0,
            ));
        }
        Ok(words)
    }

    pub(crate) fn page_dimensions_points(
        &self,
        page_num: u32,
    ) -> Result<(f32, f32), Box<dyn std::error::Error>> {
        if page_num == 0 || page_num > self.total {
            return Err(format!("page {page_num} out of range (total {})", self.total).into());
        }
        let page_index = i32::try_from(page_num - 1)
            .map_err(|_| format!("page index {page_num} exceeds Pdfium's supported range"))?;
        let doc = self.pdfium.load_pdf_from_file(&self.path, None)?;
        let page = doc.pages().get(page_index)?;
        Ok((page.width().value, page.height().value))
    }

    /// Renders only a normalized top-left page region at the requested density.
    /// PDFium renders a translated virtual full page directly into the
    /// smaller bounded ROI bitmap; no full-page raster is allocated.
    #[expect(
        clippy::too_many_arguments,
        reason = "Preserve the accepted OCR baseline during source-only publication."
    )]
    pub(crate) fn render_region_at_dpi_with_limit(
        &self,
        page_num: u32,
        target_dpi: u32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        max_pixels: Option<usize>,
    ) -> Result<RenderedRegion, Box<dyn std::error::Error>> {
        if target_dpi == 0 {
            return Err("render DPI must be greater than zero".into());
        }
        if !(x.is_finite()
            && y.is_finite()
            && width.is_finite()
            && height.is_finite()
            && x >= 0.0
            && y >= 0.0
            && width > 0.0
            && height > 0.0
            && x + width <= 1.0 + f32::EPSILON
            && y + height <= 1.0 + f32::EPSILON)
        {
            return Err(
                format!("invalid normalized render region ({x}, {y}, {width}, {height})").into(),
            );
        }

        let (page_width_points, page_height_points) = self.page_dimensions_points(page_num)?;
        let full_width = target_width_for_dpi(page_width_points, f64::from(target_dpi))?;
        let full_height =
            target_height_for_width(page_width_points, page_height_points, full_width)?;

        let x0 = (f64::from(x) * f64::from(full_width))
            .floor()
            .clamp(0.0, f64::from(full_width)) as u32;
        let y0 = (f64::from(y) * f64::from(full_height))
            .floor()
            .clamp(0.0, f64::from(full_height)) as u32;
        let x1 = (f64::from(x + width) * f64::from(full_width))
            .ceil()
            .clamp(0.0, f64::from(full_width)) as u32;
        let y1 = (f64::from(y + height) * f64::from(full_height))
            .ceil()
            .clamp(0.0, f64::from(full_height)) as u32;
        let region_width = x1.saturating_sub(x0);
        let region_height = y1.saturating_sub(y0);
        if region_width == 0 || region_height == 0 {
            return Err("normalized render region maps to an empty bitmap".into());
        }
        let region_pixels = usize::try_from(region_width)?
            .checked_mul(usize::try_from(region_height)?)
            .ok_or("render region pixel count overflow")?;
        if max_pixels.is_some_and(|limit| region_pixels > limit) {
            return Err(format!(
                "render region requires {region_pixels} pixels, exceeding limit {}",
                max_pixels.expect("checked render region limit")
            )
            .into());
        }

        let page_index = usize::try_from(
            page_num
                .checked_sub(1)
                .ok_or("page numbers are 1-indexed")?,
        )?;
        let bitmap = render_pdf_page_region_with_dll(
            &self.path,
            page_index,
            full_width,
            full_height,
            x0,
            y0,
            region_width,
            region_height,
            &self.pdfium_dll,
        )?;
        Ok(RenderedRegion {
            image: bgra_to_gray(&bitmap)?,
            dpi: target_dpi,
            full_width,
            full_height,
            x: x0,
            y: y0,
        })
    }

    fn render_gray_at_width(
        &self,
        page_num: u32,
        target_width: u32,
    ) -> Result<GrayImage, Box<dyn std::error::Error>> {
        if page_num == 0 || page_num > self.total {
            return Err(format!("page {page_num} out of range (total {})", self.total).into());
        }
        if target_width == 0 {
            return Err("PDFium render target width must be greater than zero".into());
        }
        let page_index = usize::try_from(page_num - 1)?;
        let bitmap =
            render_pdf_page_with_dll(&self.path, page_index, target_width, &self.pdfium_dll)?;
        bgra_to_gray(&bitmap)
    }
}

impl PageSource for PdfiumPageSource {
    fn total_pages(&self) -> Result<u32, Box<dyn std::error::Error>> {
        Ok(self.total)
    }

    fn render_page(&mut self, page_num: u32) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        let (page_width_points, _) = self.page_dimensions_points(page_num)?;
        let target_width = target_width_for_dpi(page_width_points, self.dpi)?;
        let gray = self.render_gray_at_width(page_num, target_width)?;

        Ok(RenderedPage::new(page_num, gray, self.dpi))
    }

    fn render_page_with_max_pixels(
        &mut self,
        page_num: u32,
        max_pixels: usize,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        let (page_width_points, page_height_points) = self.page_dimensions_points(page_num)?;
        let requested_width = target_width_for_dpi(page_width_points, self.dpi)?;
        let target_width = target_width_for_pixel_limit(
            page_width_points,
            page_height_points,
            requested_width,
            max_pixels,
        )?;
        let gray = self.render_gray_at_width(page_num, target_width)?;
        let effective_dpi = f64::from(target_width) * 72.0 / f64::from(page_width_points);
        Ok(RenderedPage::new(page_num, gray, effective_dpi))
    }

    fn render_region_at_dpi(
        &mut self,
        page_num: u32,
        target_dpi: u32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        max_pixels: usize,
    ) -> Result<Option<RenderedPageRegion>, Box<dyn std::error::Error>> {
        let region = self.render_region_at_dpi_with_limit(
            page_num,
            target_dpi,
            x,
            y,
            width,
            height,
            Some(max_pixels),
        )?;
        Ok(Some(RenderedPageRegion {
            image: region.image,
            dpi: region.dpi,
            full_width: region.full_width,
            full_height: region.full_height,
            x: region.x,
            y: region.y,
        }))
    }

    fn render_page_at_dpi(
        &mut self,
        page_num: u32,
        target_dpi: u32,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        if target_dpi == 0 {
            return Err("render DPI must be greater than zero".into());
        }
        let (page_width_points, _) = self.page_dimensions_points(page_num)?;
        let target_width = target_width_for_dpi(page_width_points, f64::from(target_dpi))?;
        let gray = self.render_gray_at_width(page_num, target_width)?;
        Ok(RenderedPage::new(page_num, gray, f64::from(target_dpi)))
    }

    fn render_page_at_dpi_with_max_pixels(
        &mut self,
        page_num: u32,
        target_dpi: u32,
        max_pixels: usize,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        if target_dpi == 0 {
            return Err("render DPI must be greater than zero".into());
        }
        let (page_width_points, page_height_points) = self.page_dimensions_points(page_num)?;
        let requested_width = target_width_for_dpi(page_width_points, f64::from(target_dpi))?;
        let target_width = target_width_for_pixel_limit(
            page_width_points,
            page_height_points,
            requested_width,
            max_pixels,
        )?;
        let gray = self.render_gray_at_width(page_num, target_width)?;
        let effective_dpi = f64::from(target_width) * 72.0 / f64::from(page_width_points);
        Ok(RenderedPage::new(page_num, gray, effective_dpi))
    }

    fn native_text_for_page(
        &self,
        page_num: u32,
    ) -> Result<Option<NativeTextLayer>, Box<dyn std::error::Error>> {
        self.extract_native_text_layer(page_num).map(Some)
    }

    fn native_text_layout_for_page(
        &self,
        page_num: u32,
    ) -> Result<Option<NativeTextLayout>, Box<dyn std::error::Error>> {
        self.extract_native_text_layout(page_num).map(Some)
    }

    fn page_orientation_from_metadata(
        &self,
        page_num: u32,
    ) -> Result<PageOrientation, Box<dyn std::error::Error>> {
        if page_num == 0 || page_num > self.total {
            return Err(format!("page {page_num} out of range (total {})", self.total).into());
        }
        let page_index = i32::try_from(page_num - 1)
            .map_err(|_| format!("page index {page_num} exceeds Pdfium's supported range"))?;
        let doc = self.pdfium.load_pdf_from_file(&self.path, None)?;
        let page = doc.pages().get(page_index)?;
        let angle = page.rotation()?.as_degrees() as u16;
        Ok(PageOrientation {
            angle,
            // The value comes from authoritative PDF page metadata, not an
            // OCR heuristic. A valid Pdfium enum therefore has full confidence.
            confidence: 1.0,
        })
    }

    fn native_words_for_rendered_page(
        &self,
        page_num: u32,
        width: u32,
        height: u32,
    ) -> Result<Option<Vec<Word>>, Box<dyn std::error::Error>> {
        self.extract_native_words_for_rendered_page(page_num, width, height)
            .map(Some)
    }

    fn render_table_thumbnail(
        &mut self,
        page_num: u32,
        max_dimension: u32,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        let (page_width_points, page_height_points) = self.page_dimensions_points(page_num)?;
        let target_width =
            target_width_for_max_dimension(page_width_points, page_height_points, max_dimension)?;
        let gray = self.render_gray_at_width(page_num, target_width)?;
        Ok(RenderedPage::new(page_num, gray, self.dpi))
    }
}

#[derive(Debug, Clone, Copy)]
struct PdfWordBounds {
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
}

impl PdfWordBounds {
    fn is_valid(&self) -> bool {
        self.left.is_finite()
            && self.bottom.is_finite()
            && self.right.is_finite()
            && self.top.is_finite()
            && self.right > self.left
            && self.top > self.bottom
    }

    fn height(&self) -> f32 {
        self.top - self.bottom
    }

    fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            bottom: self.bottom.min(other.bottom),
            right: self.right.max(other.right),
            top: self.top.max(other.top),
        }
    }
}

#[derive(Debug, Default)]
struct NativeWordBuilder {
    text: String,
    bounds: Option<PdfWordBounds>,
    last_glyph: Option<PdfWordBounds>,
}

impl NativeWordBuilder {
    fn should_split_before(&self, next: &PdfWordBounds) -> bool {
        let Some(previous) = self.last_glyph else {
            return false;
        };
        let scale = previous.height().max(next.height()).max(1.0);
        let previous_center_y = (previous.bottom + previous.top) / 2.0;
        let next_center_y = (next.bottom + next.top) / 2.0;
        let changed_line = (previous_center_y - next_center_y).abs() > scale * 0.65;
        let large_forward_gap = next.left - previous.right > scale * 0.70;
        changed_line || large_forward_gap
    }

    fn push(&mut self, value: char, glyph_bounds: Option<PdfWordBounds>) {
        self.text.push(value);
        if let Some(bounds) = glyph_bounds {
            self.bounds = Some(self.bounds.map_or(bounds, |current| current.union(bounds)));
            self.last_glyph = Some(bounds);
        }
    }

    fn flush_into(&mut self, output: &mut Vec<RawNativeWord>) {
        if !self.text.is_empty() {
            output.push(RawNativeWord {
                text: std::mem::take(&mut self.text),
                bounds: self.bounds.take(),
            });
        }
        self.bounds = None;
        self.last_glyph = None;
    }
}

#[derive(Debug)]
struct RawNativeWord {
    text: String,
    bounds: Option<PdfWordBounds>,
}

pub(crate) fn target_width_for_dpi(
    page_width_points: f32,
    dpi: f64,
) -> Result<u32, Box<dyn std::error::Error>> {
    let target_width = f64::from(page_width_points) * dpi / 72.0;
    if !target_width.is_finite() || target_width < 1.0 || target_width > f64::from(u32::MAX) {
        return Err(format!(
            "invalid render target width {target_width} from {page_width_points} points at {dpi} DPI"
        )
        .into());
    }
    Ok(target_width.round() as u32)
}

fn target_width_for_max_dimension(
    page_width_points: f32,
    page_height_points: f32,
    max_dimension: u32,
) -> Result<u32, Box<dyn std::error::Error>> {
    if max_dimension == 0 {
        return Err("table-thumbnail max dimension must be greater than zero".into());
    }
    let width = f64::from(page_width_points);
    let height = f64::from(page_height_points);
    let longest_side = width.max(height);
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(format!(
            "invalid PDF page dimensions {page_width_points}x{page_height_points} points"
        )
        .into());
    }
    let target_width = width * f64::from(max_dimension) / longest_side;
    if !target_width.is_finite() || target_width < 1.0 || target_width > f64::from(u32::MAX) {
        return Err(format!(
            "invalid table-thumbnail target width {target_width} from {page_width_points}x{page_height_points} points"
        )
        .into());
    }
    Ok(target_width.round().max(1.0) as u32)
}

fn target_width_for_pixel_limit(
    page_width_points: f32,
    page_height_points: f32,
    requested_width: u32,
    max_pixels: usize,
) -> Result<u32, Box<dyn std::error::Error>> {
    let requested_height =
        target_height_for_width(page_width_points, page_height_points, requested_width)?;
    let (mut target_width, _) =
        dimensions_within_pixel_limit(requested_width, requested_height, max_pixels)?;

    // PDFium rounds the derived height. Tighten the width one pixel at a time
    // only around the limit so the final FFI bitmap can never exceed the
    // pipeline's established memory budget.
    while target_width > 1 {
        let target_height =
            target_height_for_width(page_width_points, page_height_points, target_width)?;
        let pixels = usize::try_from(target_width)?
            .checked_mul(usize::try_from(target_height)?)
            .ok_or("PDFium bounded bitmap pixel count overflow")?;
        if pixels <= max_pixels {
            return Ok(target_width);
        }
        target_width -= 1;
    }

    let target_height = target_height_for_width(page_width_points, page_height_points, 1)?;
    if usize::try_from(target_height)? > max_pixels {
        return Err(format!(
            "page pixel limit {max_pixels} is too small for PDFium page dimensions {page_width_points}x{page_height_points}"
        )
        .into());
    }
    Ok(1)
}

pub(crate) fn target_height_for_width(
    page_width_points: f32,
    page_height_points: f32,
    target_width: u32,
) -> Result<u32, Box<dyn std::error::Error>> {
    let width = f64::from(page_width_points);
    let height = f64::from(page_height_points);
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(format!(
            "invalid PDF page dimensions {page_width_points}x{page_height_points} points"
        )
        .into());
    }
    let target_height = f64::from(target_width) * height / width;
    if !target_height.is_finite() || target_height < 1.0 || target_height > f64::from(u32::MAX) {
        return Err(format!(
            "invalid PDFium target height {target_height} for width {target_width}"
        )
        .into());
    }
    Ok(target_height.round() as u32)
}

fn bgra_to_gray(bitmap: &FfiRenderedPage) -> Result<GrayImage, Box<dyn std::error::Error>> {
    let width = usize::try_from(bitmap.width)?;
    let height = usize::try_from(bitmap.height)?;
    let packed_row_bytes = width
        .checked_mul(4)
        .ok_or("PDFium BGRA row size overflow")?;
    if bitmap.stride < packed_row_bytes {
        return Err(format!(
            "invalid PDFium BGRA stride {} for {}-byte row",
            bitmap.stride, packed_row_bytes
        )
        .into());
    }
    let expected_len = bitmap
        .stride
        .checked_mul(height)
        .ok_or("PDFium BGRA bitmap size overflow")?;
    if bitmap.bgra.len() != expected_len {
        return Err(format!(
            "invalid PDFium BGRA buffer: {} bytes for {}x{} with stride {}",
            bitmap.bgra.len(),
            bitmap.width,
            bitmap.height,
            bitmap.stride
        )
        .into());
    }

    let mut gray = Vec::with_capacity(
        width
            .checked_mul(height)
            .ok_or("gray bitmap size overflow")?,
    );
    for row in bitmap.bgra.chunks_exact(bitmap.stride) {
        for pixel in row[..packed_row_bytes].chunks_exact(4) {
            let blue = u32::from(pixel[0]);
            let green = u32::from(pixel[1]);
            let red = u32::from(pixel[2]);
            // Integer Rec. 601 luma conversion; alpha is already composited by
            // PDFium against the white background passed to FPDFBitmap_FillRect.
            gray.push(((299 * red + 587 * green + 114 * blue + 500) / 1000) as u8);
        }
    }

    GrayImage::from_raw(bitmap.width, bitmap.height, gray)
        .ok_or_else(|| "failed to construct grayscale image from PDFium BGRA bitmap".into())
}

#[cfg(test)]
#[expect(
    clippy::manual_is_multiple_of,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
fn pack_gray_rows(
    raw: &[u8],
    width: u32,
    height: u32,
) -> Result<GrayImage, Box<dyn std::error::Error>> {
    let width = usize::try_from(width)?;
    let height = usize::try_from(height)?;
    if height == 0 || raw.len() % height != 0 {
        return Err(format!(
            "invalid Pdfium grayscale bitmap buffer: {} bytes for {width}x{height}",
            raw.len()
        )
        .into());
    }

    let stride = raw.len() / height;
    if stride < width {
        return Err(
            format!("invalid Pdfium grayscale bitmap stride {stride} for width {width}").into(),
        );
    }

    let packed_len = width
        .checked_mul(height)
        .ok_or("Pdfium grayscale bitmap dimensions overflow")?;
    let mut packed = Vec::with_capacity(packed_len);
    for row in raw.chunks_exact(stride) {
        packed.extend_from_slice(&row[..width]);
    }

    GrayImage::from_raw(width as u32, height as u32, packed)
        .ok_or_else(|| "failed to construct packed Pdfium grayscale bitmap".into())
}

#[cfg(test)]
mod tests {
    use super::{
        bgra_to_gray, control_run_profile, insertable_pdf_control_run_count,
        native_words_from_glyphs, pack_gray_rows, pdf_control_runs_are_boundaries,
        resolve_pdf_control_runs, target_height_for_width, target_width_for_dpi,
        target_width_for_pixel_limit, PdfTextGlyph, PdfWordBounds,
    };
    use crate::native::pdfium_render_ffi::RenderedPage as FfiRenderedPage;

    #[cfg(windows)]
    use super::PdfiumPageSource;

    #[test]
    fn packs_pdfium_row_padding() {
        let raw = [1, 2, 3, 99, 4, 5, 6, 99];
        let image = pack_gray_rows(&raw, 3, 2).unwrap();
        assert_eq!(image.as_raw(), &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn rejects_short_stride() {
        assert!(pack_gray_rows(&[1, 2], 3, 1).is_err());
    }

    fn glyph(value: char, left: f32, bottom: f32, right: f32, top: f32) -> PdfTextGlyph {
        PdfTextGlyph {
            value,
            origin: Some((left, bottom)),
            angle_radians: Some(0.0),
            bounds: Some(PdfWordBounds {
                left,
                bottom,
                right,
                top,
            }),
        }
    }

    fn unknown_glyph(value: char) -> PdfTextGlyph {
        PdfTextGlyph {
            value,
            origin: None,
            angle_radians: None,
            bounds: None,
        }
    }

    fn control_sequence(runs: usize, cross: f32, origin_advance: f32) -> Vec<PdfTextGlyph> {
        control_sequence_with_steps(&vec![(cross, origin_advance); runs])
    }

    fn control_sequence_with_steps(steps: &[(f32, f32)]) -> Vec<PdfTextGlyph> {
        let mut glyphs = Vec::with_capacity(steps.len() * 2 + 1);
        let (mut x, mut y) = (100.0_f32, 0.0_f32);
        for index in 0..=steps.len() {
            let value = char::from_u32(u32::from(b'A') + index as u32).unwrap();
            glyphs.push(glyph(value, x, y, x + 10.0, y + 10.0));
            if let Some((cross, origin_advance)) = steps.get(index).copied() {
                glyphs.push(unknown_glyph('\r'));
                x += origin_advance * 10.0;
                y += cross * 10.0;
            }
        }
        glyphs
    }

    #[test]
    fn adjacent_glyphs_drop_internal_pdf_controls() {
        let glyphs = [
            glyph('A', 0.0, 0.0, 10.0, 10.0),
            unknown_glyph('\r'),
            unknown_glyph('\n'),
            glyph('B', 10.5, 0.0, 20.5, 10.0),
        ];
        assert_eq!(resolve_pdf_control_runs(&glyphs, true), "AB");
    }

    #[test]
    fn large_gap_preserves_one_word_boundary() {
        let glyphs = [
            glyph('A', 0.0, 0.0, 10.0, 10.0),
            unknown_glyph('\r'),
            unknown_glyph('\n'),
            glyph('B', 18.0, 0.0, 28.0, 10.0),
        ];
        assert_eq!(resolve_pdf_control_runs(&glyphs, false), "A B");
    }

    #[test]
    fn mixed_control_runs_are_decided_locally() {
        let glyphs = [
            glyph('A', 0.0, 0.0, 10.0, 10.0),
            unknown_glyph('\r'),
            glyph('B', 10.5, 0.0, 20.5, 10.0),
            unknown_glyph('\r'),
            glyph('C', 30.0, 0.0, 40.0, 10.0),
        ];
        assert_eq!(resolve_pdf_control_runs(&glyphs, false), "AB C");
    }

    #[test]
    fn line_change_preserves_one_boundary() {
        let glyphs = [
            glyph('A', 0.0, 20.0, 10.0, 30.0),
            unknown_glyph('\r'),
            glyph('B', 0.0, 0.0, 10.0, 10.0),
        ];
        assert_eq!(
            resolve_pdf_control_runs(&glyphs, false),
            format!("A{}B", char::from(10))
        );
    }

    #[test]
    fn stacked_backtracking_control_runs_do_not_fragment_cad_tokens() {
        let glyphs = control_sequence(8, 2.4, -1.0);
        assert_eq!(resolve_pdf_control_runs(&glyphs, false), "ABCDEFGHI");
        let words = native_words_from_glyphs(&glyphs, false);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, "ABCDEFGHI");
    }

    #[test]
    fn positioned_words_and_page_text_share_word_and_line_boundaries() {
        let glyphs = [
            glyph('A', 0.0, 20.0, 10.0, 30.0),
            unknown_glyph(char::from(13)),
            glyph('B', 18.0, 20.0, 28.0, 30.0),
            unknown_glyph(char::from(10)),
            glyph('C', 0.0, 0.0, 10.0, 10.0),
        ];

        let expected_text = format!("A B{}C", char::from(10));
        assert_eq!(resolve_pdf_control_runs(&glyphs, false), expected_text);
        let words = native_words_from_glyphs(&glyphs, false);
        assert_eq!(
            words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", "C"]
        );
    }

    #[test]
    fn high_cross_tail_detects_a_mixed_stacked_page_at_seventy_five_percent_coverage() {
        let steps = [
            (1.3, -1.0),
            (1.3, -1.0),
            (1.3, -1.0),
            (1.3, -1.0),
            (9.0, -1.0),
            (9.0, -1.0),
            (1.3, -1.0),
            (1.3, -1.0),
        ];
        let mut glyphs = control_sequence_with_steps(&steps);
        glyphs[0] = unknown_glyph('A');
        glyphs[16] = unknown_glyph('I');

        assert!(control_run_profile(&glyphs).stacked_backtracking);
        assert_eq!(resolve_pdf_control_runs(&glyphs, true), "ABCDEFGHI");
    }

    #[test]
    fn moderate_cross_tail_requires_strong_backtracking() {
        let steps = [
            (1.5, -1.0),
            (1.5, -1.0),
            (1.5, -1.0),
            (1.5, -1.0),
            (1.5, -1.0),
            (1.5, -1.0),
            (6.0, -1.0),
            (6.0, -1.0),
        ];
        let glyphs = control_sequence_with_steps(&steps);

        assert!(control_run_profile(&glyphs).stacked_backtracking);
        assert_eq!(resolve_pdf_control_runs(&glyphs, false), "ABCDEFGHI");
    }

    #[test]
    fn positive_origin_tail_preserves_real_boundaries() {
        let steps = [
            (1.3, -1.0),
            (1.3, -1.0),
            (1.3, -1.0),
            (1.3, -1.0),
            (1.3, -1.0),
            (9.0, 1.0),
            (9.0, 1.0),
            (9.0, 1.0),
        ];
        let glyphs = control_sequence_with_steps(&steps);

        assert!(!control_run_profile(&glyphs).stacked_backtracking);
        assert_eq!(
            resolve_pdf_control_runs(&glyphs, false),
            "A B C D E F G H I".replace(' ', "\n")
        );
    }

    #[test]
    fn insufficient_geometry_coverage_keeps_the_page_fallback() {
        let steps = [
            (1.3, -1.0),
            (1.3, -1.0),
            (1.3, -1.0),
            (1.3, -1.0),
            (9.0, -1.0),
            (9.0, -1.0),
            (1.3, -1.0),
            (1.3, -1.0),
        ];
        let mut glyphs = control_sequence_with_steps(&steps);
        glyphs[0] = unknown_glyph('A');
        glyphs[8] = unknown_glyph('E');
        glyphs[16] = unknown_glyph('I');

        assert!(!control_run_profile(&glyphs).stacked_backtracking);
        assert_eq!(
            resolve_pdf_control_runs(&glyphs, true),
            "A B|C|D E F|G|H I".replace('|', "\n")
        );
    }

    #[test]
    fn normal_line_pitch_preserves_boundaries_even_when_origins_backtrack() {
        let glyphs = control_sequence(8, 1.2, -1.0);
        assert_eq!(
            resolve_pdf_control_runs(&glyphs, false),
            "A B C D E F G H I".replace(' ', "\n")
        );
    }

    #[test]
    fn forward_progress_prevents_stacked_control_classification() {
        let glyphs = control_sequence(8, 2.4, 1.0);
        assert_eq!(
            resolve_pdf_control_runs(&glyphs, false),
            "A B C D E F G H I".replace(' ', "\n")
        );
    }

    #[test]
    fn sparse_high_cross_runs_do_not_activate_stacked_profile() {
        let glyphs = control_sequence(7, 2.4, -1.0);
        assert_eq!(
            resolve_pdf_control_runs(&glyphs, false),
            "A B C D E F G H".replace(' ', "\n")
        );
    }

    #[test]
    fn stacked_page_still_preserves_a_same_line_word_gap() {
        let mut glyphs = control_sequence(8, 2.4, -1.0);
        let previous_bounds = glyphs.last().unwrap().bounds.unwrap();
        glyphs.push(unknown_glyph('\r'));
        glyphs.push(glyph(
            'J',
            previous_bounds.left + 20.0,
            previous_bounds.bottom,
            previous_bounds.right + 20.0,
            previous_bounds.top,
        ));
        assert_eq!(resolve_pdf_control_runs(&glyphs, false), "ABCDEFGHI J");
    }

    #[test]
    fn missing_geometry_uses_page_fallback() {
        let glyphs = [unknown_glyph('A'), unknown_glyph('\r'), unknown_glyph('B')];
        assert_eq!(resolve_pdf_control_runs(&glyphs, false), "AB");
        assert_eq!(resolve_pdf_control_runs(&glyphs, true), "A B");
    }

    #[test]
    fn existing_whitespace_is_not_duplicated_and_edge_controls_are_dropped() {
        let glyphs = [
            unknown_glyph('\r'),
            glyph('A', 0.0, 0.0, 10.0, 10.0),
            unknown_glyph('\n'),
            unknown_glyph(' '),
            glyph('B', 18.0, 0.0, 28.0, 10.0),
            unknown_glyph('\r'),
        ];
        assert_eq!(resolve_pdf_control_runs(&glyphs, true), "A B");
    }

    #[test]
    fn bounded_coverage_selects_only_the_missing_geometry_fallback() {
        assert!(pdf_control_runs_are_boundaries(84, 100, 10));
        assert!(!pdf_control_runs_are_boundaries(85, 100, 3));
    }

    #[test]
    fn counts_insertable_control_runs_for_fallback_density() {
        let glyphs = [
            unknown_glyph('A'),
            unknown_glyph('\r'),
            unknown_glyph('\n'),
            unknown_glyph('B'),
            unknown_glyph(' '),
            unknown_glyph('C'),
        ];
        assert_eq!(insertable_pdf_control_run_count(&glyphs), 1);
    }

    #[test]
    fn converts_bgra_rows_with_padding_to_gray() {
        let bitmap = FfiRenderedPage {
            width: 2,
            height: 2,
            stride: 12,
            bgra: vec![
                0, 0, 0, 255, 255, 255, 255, 255, 99, 99, 99, 99, // first row + padding
                0, 0, 255, 255, 0, 255, 0, 255, 99, 99, 99, 99, // second row + padding
            ],
        };
        let gray = bgra_to_gray(&bitmap).unwrap();
        assert_eq!(gray.as_raw(), &[0, 255, 76, 150]);
    }

    #[test]
    fn dpi_controls_direct_pdfium_render_width() {
        // A one-inch (72 point) page makes the DPI relationship exact.
        assert_eq!(target_width_for_dpi(72.0, 200.0).unwrap(), 200);
        assert_eq!(target_width_for_dpi(72.0, 300.0).unwrap(), 300);
        assert_eq!(target_width_for_dpi(72.0, 400.0).unwrap(), 400);
    }

    #[test]
    fn bounded_render_target_respects_pixel_budget_before_pdfium_allocates() {
        let width = target_width_for_pixel_limit(72.0, 72.0, 300, 50_000).unwrap();
        let height = target_height_for_width(72.0, 72.0, width).unwrap();
        assert!(usize::try_from(width).unwrap() * usize::try_from(height).unwrap() <= 50_000);
        assert!(width < 300);
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "run separately: pdfium-render permits only one binding initialization per process"]
    fn extracts_native_text_from_synthetic_control_pdf() {
        let _guard = crate::native::pdfium_test_lock().lock().unwrap();
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("native-text-control.pdf");
        write_minimal_text_pdf(&path);
        let source = PdfiumPageSource::open(&path, 200.0).expect("open control PDF");
        let text = source.extract_native_text(1).expect("extract native text");
        assert!(
            text.contains("Native Text"),
            "synthetic PDF should expose its native text layer: {text:?}"
        );
    }

    #[cfg(windows)]
    fn write_minimal_text_pdf(path: &std::path::Path) {
        let content = "BT /F1 24 Tf 20 50 Td (Native Text Fixture) Tj ET\n";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        std::fs::write(path, pdf).expect("write synthetic text PDF");
    }
}
