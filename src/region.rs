//! Isolated PDF-region processing for embedding applications.
//!
//! Region coordinates are always `normalized_top_left`: `(0, 0)` is the
//! top-left corner of the visible page after PDF `/Rotate`, and `(1, 1)` is
//! the bottom-right corner. Screen, zoom, editor-window, PDF-point, and raster
//! coordinates are deliberately not accepted by the public API.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::api::{
    ClassificationEvidence, ColumnRoleAssignment, GridKind, OcrError, OcrOptions, SemanticTableType,
};
use crate::page_analysis::{NativeTextQuality, PageOrientation, PageStageTimings, ProcessingMode};
#[cfg(feature = "native")]
use crate::table_semantics::classify_table;

/// Schema version emitted by [`RegionResult`].
pub const REGION_RESULT_SCHEMA_VERSION: u32 = 1;

/// A page-relative rectangle in the documented `normalized_top_left` space.
///
/// Every component must be finite and in the range `0.0..=1.0`. Width and
/// height must be positive. A right or bottom edge beyond `1.0` is clipped and
/// reported through [`RegionResult::effective_rect`] and
/// [`RegionWarning::RegionClipped`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct NormalizedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl NormalizedRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[cfg(any(feature = "native", test))]
    fn right(self) -> f32 {
        self.x + self.width
    }

    #[cfg(any(feature = "native", test))]
    fn bottom(self) -> f32 {
        self.y + self.height
    }
}

/// Region-processing behavior requested by a host.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegionMode {
    #[default]
    Auto,
    Text,
    Table,
}

/// One zero-based page selection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct RegionRequest {
    pub page_index: usize,
    pub rect: NormalizedRect,
    pub mode: RegionMode,
}

/// Coordinate system used by every rectangle in [`RegionResult`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegionCoordinateSpace {
    #[default]
    NormalizedTopLeft,
}

/// One recognized word in page-relative normalized coordinates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegionWord {
    pub text: String,
    pub bbox: NormalizedRect,
    pub confidence: f32,
}

/// One table cell in page-relative normalized coordinates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegionTableCell {
    pub row: usize,
    pub column: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub bbox: NormalizedRect,
    pub text: String,
    pub confidence: f32,
}

/// A ruled grid found entirely or partly inside the selected region.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegionTable {
    pub kind: GridKind,
    #[serde(default)]
    pub semantic_type: SemanticTableType,
    #[serde(default)]
    pub semantic_confidence: f32,
    #[serde(default)]
    pub header_rows: Vec<usize>,
    #[serde(default)]
    pub column_roles: Vec<ColumnRoleAssignment>,
    #[serde(default)]
    pub classification_evidence: Vec<ClassificationEvidence>,
    pub bbox: NormalizedRect,
    pub rows: usize,
    pub columns: usize,
    pub cells: Vec<RegionTableCell>,
    pub confidence: f32,
}

/// Region-wide word and cell accounting.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegionMetrics {
    pub recognized_words: u32,
    pub assigned_words: u32,
    pub words_outside_tables: u32,
    pub duplicate_assignments: u32,
    pub filled_cells: u32,
    pub total_cells: u32,
}

/// Non-fatal region-processing conditions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RegionWarning {
    RegionClipped {
        requested: NormalizedRect,
        effective: NormalizedRect,
    },
    NativeTextUnavailable {
        message: String,
    },
    TableDetectionFailed {
        message: String,
    },
    CellOcrFallbackFailed {
        message: String,
    },
}

/// Stable result for one selected PDF page region.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegionResult {
    pub schema_version: u32,
    pub page_index: usize,
    pub requested_rect: NormalizedRect,
    pub effective_rect: NormalizedRect,
    pub coordinate_space: RegionCoordinateSpace,
    pub processing_mode: ProcessingMode,
    /// Complete text selected from the ROI. Attached tables never replace it.
    pub text: String,
    /// Text represented by words that were not assigned to a returned table.
    pub text_outside_tables: String,
    pub words: Vec<RegionWord>,
    pub tables: Vec<RegionTable>,
    pub orientation: PageOrientation,
    pub language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_text_quality: Option<NativeTextQuality>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ocr_confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_dpi: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_height: Option<u32>,
    pub timings: PageStageTimings,
    pub metrics: RegionMetrics,
    pub warnings: Vec<RegionWarning>,
}

/// Serializes one region result without invoking full-document exporters.
pub fn export_region_result_json(result: &RegionResult, output: &Path) -> Result<(), OcrError> {
    let body = serde_json::to_string_pretty(result).map_err(|error| OcrError::Export {
        format: "region JSON",
        path: output.to_path_buf(),
        detail: error.to_string(),
    })?;
    std::fs::write(output, body).map_err(|error| OcrError::Export {
        format: "region JSON",
        path: output.to_path_buf(),
        detail: error.to_string(),
    })
}

/// Processes exactly one selected region without invoking the document
/// pipeline or full-document exporters.
#[cfg(feature = "native")]
pub fn process_region(
    input: impl AsRef<Path>,
    request: RegionRequest,
    options: OcrOptions,
) -> Result<RegionResult, OcrError> {
    use std::time::Instant;

    use crate::native::pdfium_source::{
        target_height_for_width, target_width_for_dpi, PdfiumPageSource,
    };
    use crate::native::tesseract_engine::TesseractOcrEngine;
    use crate::ocr::{OcrEngine, OcrRequest};
    use crate::page_source::PageSource;
    use crate::pipeline::{
        detect_borderless_tables_safely, detect_table_grids_safely,
        fill_empty_grid_cells_with_ocr_diagnostics, matrix_word_assignment_metrics,
        page_word_assignment_metrics, word_is_assignable_to_grid,
    };
    use crate::preprocessing::{preprocess, DEFAULT_MAX_PIXELS};
    use crate::table_grid::classify_grid;

    let started = Instant::now();
    let input = input.as_ref();
    let (effective_rect, clipped) = validate_and_clip_rect(request.rect)?;
    if !input.is_file() {
        return Err(OcrError::PdfOpenFailed {
            path: input.to_path_buf(),
            detail: "input file does not exist".to_string(),
        });
    }
    crate::api::validate_options(input, &options)?;
    let languages = options
        .effective_languages()
        .map_err(OcrError::InvalidOptions)?
        .to_string();
    let ocr_dpi = options
        .effective_ocr_dpi()
        .map_err(OcrError::InvalidOptions)?;

    let source = PdfiumPageSource::open_with_pdfium_dll(
        input,
        f64::from(ocr_dpi),
        options.pdfium_dll_path.as_deref(),
    )
    .map_err(|error| OcrError::PdfOpenFailed {
        path: input.to_path_buf(),
        detail: error.to_string(),
    })?;
    let page_count = source
        .total_pages()
        .map_err(|error| OcrError::PdfOpenFailed {
            path: input.to_path_buf(),
            detail: error.to_string(),
        })? as usize;
    if request.page_index >= page_count {
        return Err(OcrError::InvalidPageIndex {
            page_index: request.page_index,
            page_count,
        });
    }
    let page_num =
        u32::try_from(request.page_index + 1).map_err(|_| OcrError::InvalidPageIndex {
            page_index: request.page_index,
            page_count,
        })?;

    let orientation = source
        .page_orientation_from_metadata(page_num)
        .map_err(|error| OcrError::NativeTextFailed {
            page_index: request.page_index,
            detail: format!("could not read PDF page rotation: {error}"),
        })?;
    let (page_width_points, page_height_points) =
        source
            .page_dimensions_points(page_num)
            .map_err(|error| OcrError::NativeTextFailed {
                page_index: request.page_index,
                detail: format!("could not read visible page dimensions: {error}"),
            })?;
    let native_full_width =
        target_width_for_dpi(page_width_points, f64::from(ocr_dpi)).map_err(|error| {
            OcrError::NativeTextFailed {
                page_index: request.page_index,
                detail: error.to_string(),
            }
        })?;
    let native_full_height =
        target_height_for_width(page_width_points, page_height_points, native_full_width).map_err(
            |error| OcrError::NativeTextFailed {
                page_index: request.page_index,
                detail: error.to_string(),
            },
        )?;
    ensure_minimum_pixel_extent(
        effective_rect,
        native_full_width,
        native_full_height,
        request.page_index,
    )?;

    let mut timings = PageStageTimings::default();
    let mut warnings = Vec::new();
    if clipped {
        warnings.push(RegionWarning::RegionClipped {
            requested: request.rect,
            effective: effective_rect,
        });
    }

    let native_started = Instant::now();
    let native_region_words = match source.extract_native_words_for_rendered_page(
        page_num,
        native_full_width,
        native_full_height,
    ) {
        Ok(words) => words
            .into_iter()
            .filter_map(|word| {
                full_pixel_word_to_region_word(
                    word,
                    native_full_width,
                    native_full_height,
                    effective_rect,
                )
            })
            .collect::<Vec<_>>(),
        Err(error) => {
            warnings.push(RegionWarning::NativeTextUnavailable {
                message: error.to_string(),
            });
            Vec::new()
        }
    };
    timings.native_text_extraction_ms = elapsed_ms(native_started);
    let native_text = words_to_text(&native_region_words);
    let language_profile = crate::language::select_language_profile(
        options.language_routing,
        &languages,
        Some(&native_text),
    );
    let languages = crate::language::effective_languages(&languages, language_profile);
    if language_profile == crate::ocr::OcrLanguageProfile::SimplifiedChinese {
        TesseractOcrEngine::new_lazy_with_tessdata(&languages, options.tessdata_path.as_deref())
            .map_err(|error| OcrError::OcrFailed {
                page_index: request.page_index,
                detail: error.to_string(),
            })?;
    }
    let native_text_quality = region_native_text_quality(&native_text);
    let native_usable = native_text_quality.is_usable;

    let should_detect_tables = matches!(request.mode, RegionMode::Auto | RegionMode::Table);
    let requires_ocr_text = !native_usable;
    let render_dpi = if requires_ocr_text {
        ocr_dpi
    } else {
        options.grid_dpi
    };
    let needs_render = requires_ocr_text || should_detect_tables;

    let mut rendered = None;
    if needs_render {
        let render_started = Instant::now();
        let region = source
            .render_region_at_dpi_with_limit(
                page_num,
                render_dpi,
                effective_rect.x,
                effective_rect.y,
                effective_rect.width,
                effective_rect.height,
                Some(DEFAULT_MAX_PIXELS),
            )
            .map_err(|error| OcrError::RenderFailed {
                page_index: request.page_index,
                detail: error.to_string(),
            })?;
        if region.image.width() < 2 || region.image.height() < 2 {
            return Err(OcrError::InvalidRegion {
                reason: format!(
                    "region maps to only {}x{} pixels at {render_dpi} DPI",
                    region.image.width(),
                    region.image.height()
                ),
            });
        }
        if requires_ocr_text {
            timings.full_render_ms = elapsed_ms(render_started);
        } else {
            timings.thumbnail_render_ms = elapsed_ms(render_started);
        }
        rendered = Some(region);
    }

    let mut grids = Vec::new();
    if should_detect_tables {
        let grid_started = Instant::now();
        let detection = detect_table_grids_safely(
            &rendered
                .as_ref()
                .expect("table route always renders the ROI")
                .image,
        );
        timings.grid_detection_ms = elapsed_ms(grid_started);
        match detection {
            Ok(found) => grids = found,
            Err(message) if request.mode == RegionMode::Table => {
                return Err(OcrError::TableDetectionFailed {
                    page_index: request.page_index,
                    detail: message,
                });
            }
            Err(message) => warnings.push(RegionWarning::TableDetectionFailed { message }),
        }
    }

    let mut ocr_confidence = None;
    let (text, words, mut local_words) = if native_usable {
        let local_words = native_region_words
            .iter()
            .map(|word| region_word_to_local_word(word, effective_rect, rendered.as_ref()))
            .collect::<Vec<_>>();
        (native_text, native_region_words, local_words)
    } else {
        let region = rendered
            .as_ref()
            .expect("OCR fallback always renders the ROI");
        let preprocessing_started = Instant::now();
        let prepared = preprocess(region.image.clone(), DEFAULT_MAX_PIXELS).map_err(|error| {
            OcrError::OcrFailed {
                page_index: request.page_index,
                detail: format!("preprocessing failed: {error}"),
            }
        })?;
        timings.preprocessing_ms = elapsed_ms(preprocessing_started);
        let ocr = TesseractOcrEngine::new_lazy_with_tessdata(
            &languages,
            options.tessdata_path.as_deref(),
        )
        .map_err(|error| OcrError::OcrFailed {
            page_index: request.page_index,
            detail: error.to_string(),
        })?;
        let ocr_started = Instant::now();
        let (recognized, local_words) = ocr
            .recognize_text_and_words_with_request(&prepared, OcrRequest::page(region.dpi.max(1)))
            .map_err(|error| OcrError::OcrFailed {
                page_index: request.page_index,
                detail: error.to_string(),
            })?;
        timings.tesseract_ms = elapsed_ms(ocr_started);
        ocr_confidence = recognized.confidence;
        let words = local_words
            .iter()
            .cloned()
            .map(|word| local_word_to_region_word(word, effective_rect, region.image.dimensions()))
            .collect();
        (recognized.text, words, local_words)
    };

    let mut borderless = if grids.is_empty() && should_detect_tables && !local_words.is_empty() {
        let region = rendered
            .as_ref()
            .expect("borderless ROI detection requires a rendered ROI");
        let detection_started = Instant::now();
        let tables = detect_borderless_tables_safely(
            &local_words,
            region.image.width(),
            region.image.height(),
        )
        .map_err(|detail| OcrError::TableDetectionFailed {
            page_index: request.page_index,
            detail,
        })?;
        timings.grid_detection_ms += elapsed_ms(detection_started);
        tables
    } else {
        Vec::new()
    };

    let assignment_started = Instant::now();
    let assignment = if borderless.is_empty() {
        page_word_assignment_metrics(&local_words, &grids)
    } else {
        matrix_word_assignment_metrics(&local_words, &borderless)
    };
    let words_outside = local_words
        .iter()
        .map(|word| {
            !grids
                .iter()
                .any(|grid| word_is_assignable_to_grid(word, grid))
                && !borderless
                    .iter()
                    .any(|matrix| word_is_assignable_to_matrix(word, matrix))
        })
        .collect::<Vec<_>>();
    timings.assignment_ms += elapsed_ms(assignment_started);
    let mut tables = Vec::with_capacity(grids.len() + borderless.len());
    let mut fallback_confidences = Vec::new();
    let mut filled_cells = 0_u32;
    let mut total_cells = 0_u32;
    for (table_index, mut grid) in grids.into_iter().enumerate() {
        let assignment_started = Instant::now();
        let mut assignment_metrics = grid.assign_words(&local_words);
        timings.assignment_ms += elapsed_ms(assignment_started);
        if options.enable_cell_ocr_fallback {
            let region = rendered
                .as_ref()
                .expect("cell OCR fallback requires a rendered ROI");
            let ocr = TesseractOcrEngine::new_lazy_with_tessdata(
                &languages,
                options.tessdata_path.as_deref(),
            )
            .map_err(|error| OcrError::OcrFailed {
                page_index: request.page_index,
                detail: error.to_string(),
            })?;
            let fallback = fill_empty_grid_cells_with_ocr_diagnostics(
                &mut grid,
                &region.image,
                &ocr,
                DEFAULT_MAX_PIXELS,
                region.dpi.max(1),
                &mut timings,
            )
            .map_err(|error| OcrError::OcrFailed {
                page_index: request.page_index,
                detail: error.to_string(),
            })?;
            fallback_confidences.extend(fallback.confidences);
            warnings.extend(
                fallback
                    .failures
                    .iter()
                    .map(|failure| region_cell_warning(table_index, failure)),
            );
            assignment_metrics.empty_cells = grid
                .cells
                .iter()
                .filter(|cell| cell.text.trim().is_empty())
                .count() as u32;
        }

        total_cells = total_cells.saturating_add(assignment_metrics.cells);
        filled_cells = filled_cells.saturating_add(
            assignment_metrics
                .cells
                .saturating_sub(assignment_metrics.empty_cells),
        );
        let region = rendered
            .as_ref()
            .expect("a returned grid always has a rendered ROI");
        let mut page_grid = grid.clone();
        translate_grid_to_page_pixels(&mut page_grid, region.x, region.y);
        let assignment_started = Instant::now();
        let kind = match classify_grid(
            &page_grid,
            &assignment_metrics,
            region.full_width,
            region.full_height,
        ) {
            crate::dto::TableType::DataTable => GridKind::DataTable,
            crate::dto::TableType::TitleBlock => GridKind::TitleBlock,
        };
        tables.push(region_table_from_grid(
            &grid,
            kind,
            effective_rect,
            region.image.dimensions(),
        ));
        timings.assignment_ms += elapsed_ms(assignment_started);
    }
    if !borderless.is_empty() {
        let region = rendered
            .as_ref()
            .expect("a returned borderless table always has a rendered ROI");
        for matrix in &borderless {
            let cells = matrix.rows.iter().flatten().count() as u32;
            let non_empty = matrix
                .rows
                .iter()
                .flatten()
                .filter(|cell| !cell.text.trim().is_empty())
                .count() as u32;
            total_cells = total_cells.saturating_add(cells);
            filled_cells = filled_cells.saturating_add(non_empty);
            tables.push(region_table_from_matrix(
                matrix,
                GridKind::DataTable,
                effective_rect,
                region.image.dimensions(),
            ));
        }
        borderless.clear();
    }

    if ocr_confidence.is_none() && !fallback_confidences.is_empty() {
        ocr_confidence = Some(
            fallback_confidences.iter().copied().sum::<f32>() / fallback_confidences.len() as f32,
        );
    }
    let text_outside_tables = words_to_text(
        &words
            .iter()
            .zip(words_outside)
            .filter(|(_, outside)| *outside)
            .map(|(word, _)| word.clone())
            .collect::<Vec<_>>(),
    );

    let processing_mode = match (native_usable, tables.is_empty()) {
        (true, true) => ProcessingMode::NativeText,
        (true, false) => ProcessingMode::NativeTextWithGrid,
        (false, true) => ProcessingMode::Ocr,
        (false, false) => ProcessingMode::OcrWithGrid,
    };
    let render_size = rendered.as_ref().map(|region| region.image.dimensions());
    timings.total_ms = elapsed_ms(started);
    let metrics = RegionMetrics {
        recognized_words: assignment.recognized_words,
        assigned_words: assignment.assigned_words,
        words_outside_tables: assignment.words_outside_tables,
        duplicate_assignments: assignment.duplicate_assignments,
        filled_cells,
        total_cells,
    };

    // Keep owned local words only until all assignment and outside-table
    // accounting is complete.
    local_words.clear();
    Ok(RegionResult {
        schema_version: REGION_RESULT_SCHEMA_VERSION,
        page_index: request.page_index,
        requested_rect: request.rect,
        effective_rect,
        coordinate_space: RegionCoordinateSpace::NormalizedTopLeft,
        processing_mode,
        text,
        text_outside_tables,
        words,
        tables,
        orientation,
        language: languages,
        native_text_quality: Some(native_text_quality),
        ocr_confidence,
        render_dpi: rendered.as_ref().map(|region| region.dpi),
        render_width: render_size.map(|(width, _)| width),
        render_height: render_size.map(|(_, height)| height),
        timings,
        metrics,
        warnings,
    })
}

#[cfg(any(feature = "native", test))]
fn region_cell_warning(
    table_index: usize,
    failure: &crate::pipeline::CellOcrFailure,
) -> RegionWarning {
    RegionWarning::CellOcrFallbackFailed {
        message: format!(
            "table={table_index} row={} column={} reason_code={}",
            failure.row, failure.column, failure.reason_code
        ),
    }
}

#[cfg(not(feature = "native"))]
pub fn process_region(
    _input: impl AsRef<Path>,
    _request: RegionRequest,
    _options: OcrOptions,
) -> Result<RegionResult, OcrError> {
    Err(OcrError::NativeFeatureRequired)
}

#[cfg(any(feature = "native", test))]
fn validate_and_clip_rect(requested: NormalizedRect) -> Result<(NormalizedRect, bool), OcrError> {
    let components = [requested.x, requested.y, requested.width, requested.height];
    if components.iter().any(|value| !value.is_finite()) {
        return Err(OcrError::InvalidRegion {
            reason: "all coordinates must be finite (NaN and Infinity are rejected)".to_string(),
        });
    }
    if requested.width == 0.0 || requested.height == 0.0 {
        return Err(OcrError::EmptyRegion);
    }
    if components.iter().any(|value| *value < 0.0) {
        return Err(OcrError::InvalidRegion {
            reason: "coordinates and dimensions must not be negative".to_string(),
        });
    }
    if components.iter().any(|value| *value > 1.0) {
        return Err(OcrError::InvalidRegion {
            reason: "each coordinate and dimension must be in the range 0.0..=1.0".to_string(),
        });
    }
    if requested.x >= 1.0 || requested.y >= 1.0 {
        return Err(OcrError::RegionOutsidePage);
    }

    let right = requested.right().min(1.0);
    let bottom = requested.bottom().min(1.0);
    if right <= requested.x || bottom <= requested.y {
        return Err(OcrError::RegionOutsidePage);
    }
    let effective = NormalizedRect {
        x: requested.x,
        y: requested.y,
        width: right - requested.x,
        height: bottom - requested.y,
    };
    let clipped = effective != requested;
    Ok((effective, clipped))
}

#[cfg(any(feature = "native", test))]
fn ensure_minimum_pixel_extent(
    rect: NormalizedRect,
    full_width: u32,
    full_height: u32,
    _page_index: usize,
) -> Result<(), OcrError> {
    let x0 = (f64::from(rect.x) * f64::from(full_width)).floor();
    let y0 = (f64::from(rect.y) * f64::from(full_height)).floor();
    let x1 = (f64::from(rect.right()) * f64::from(full_width)).ceil();
    let y1 = (f64::from(rect.bottom()) * f64::from(full_height)).ceil();
    let width = (x1 - x0).max(0.0) as u32;
    let height = (y1 - y0).max(0.0) as u32;
    if width < 2 || height < 2 {
        return Err(OcrError::InvalidRegion {
            reason: format!(
                "region is too small: it maps to {width}x{height} pixels at the configured OCR density"
            ),
        });
    }
    Ok(())
}

#[cfg(feature = "native")]
fn region_native_text_quality(text: &str) -> NativeTextQuality {
    use crate::page_analysis::{analyze_native_text, NativeTextLayer};

    let mut quality = analyze_native_text(&NativeTextLayer::new(
        text.to_string(),
        text.chars().count(),
    ));
    // A selected ROI may legitimately contain only one short label or number.
    // Preserve the full-page quality signals, but lower only the minimum
    // content threshold for region routing.
    quality.is_usable = quality.non_whitespace_characters > 0
        && quality.printable_ratio >= 0.90
        && quality.replacement_characters == 0
        && quality.control_characters == 0;
    quality
}

#[cfg(feature = "native")]
fn full_pixel_word_to_region_word(
    word: crate::dto::Word,
    full_width: u32,
    full_height: u32,
    effective: NormalizedRect,
) -> Option<RegionWord> {
    let normalized = legacy_bbox_to_normalized(&word.bbox, full_width, full_height);
    let clipped = intersect_rect(normalized, effective)?;
    Some(RegionWord {
        text: word.text,
        bbox: clipped,
        confidence: word.confidence,
    })
}

#[cfg(feature = "native")]
fn region_word_to_local_word(
    word: &RegionWord,
    effective: NormalizedRect,
    rendered: Option<&crate::native::pdfium_source::RenderedRegion>,
) -> crate::dto::Word {
    let (width, height) = rendered
        .map(|region| region.image.dimensions())
        .unwrap_or((10_000, 10_000));
    let x0 = ((word.bbox.x - effective.x) / effective.width).clamp(0.0, 1.0);
    let y0 = ((word.bbox.y - effective.y) / effective.height).clamp(0.0, 1.0);
    let x1 = ((word.bbox.right() - effective.x) / effective.width).clamp(0.0, 1.0);
    let y1 = ((word.bbox.bottom() - effective.y) / effective.height).clamp(0.0, 1.0);
    crate::dto::Word::new(
        word.text.clone(),
        crate::dto::BoundingBox::new(
            f64::from(x0) * f64::from(width),
            f64::from(y0) * f64::from(height),
            f64::from(x1) * f64::from(width),
            f64::from(y1) * f64::from(height),
        ),
        word.confidence,
    )
}

#[cfg(feature = "native")]
fn local_word_to_region_word(
    word: crate::dto::Word,
    effective: NormalizedRect,
    dimensions: (u32, u32),
) -> RegionWord {
    let (width, height) = dimensions;
    RegionWord {
        text: word.text,
        bbox: local_bbox_to_normalized(&word.bbox, effective, width, height),
        confidence: word.confidence,
    }
}

#[cfg(feature = "native")]
fn legacy_bbox_to_normalized(
    bbox: &crate::dto::BoundingBox,
    width: u32,
    height: u32,
) -> NormalizedRect {
    let width = f64::from(width.max(1));
    let height = f64::from(height.max(1));
    let x0 = (bbox.x0 / width).clamp(0.0, 1.0) as f32;
    let y0 = (bbox.y0 / height).clamp(0.0, 1.0) as f32;
    let x1 = (bbox.x1 / width).clamp(0.0, 1.0) as f32;
    let y1 = (bbox.y1 / height).clamp(0.0, 1.0) as f32;
    NormalizedRect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

#[cfg(feature = "native")]
fn local_bbox_to_normalized(
    bbox: &crate::dto::BoundingBox,
    effective: NormalizedRect,
    width: u32,
    height: u32,
) -> NormalizedRect {
    let local = legacy_bbox_to_normalized(bbox, width, height);
    NormalizedRect {
        x: effective.x + local.x * effective.width,
        y: effective.y + local.y * effective.height,
        width: local.width * effective.width,
        height: local.height * effective.height,
    }
}

#[cfg(any(feature = "native", test))]
fn intersect_rect(a: NormalizedRect, b: NormalizedRect) -> Option<NormalizedRect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.right().min(b.right());
    let y1 = a.bottom().min(b.bottom());
    (x1 > x0 && y1 > y0).then(|| NormalizedRect::new(x0, y0, x1 - x0, y1 - y0))
}

#[cfg(feature = "native")]
fn region_table_from_grid(
    grid: &crate::table_grid::TableGrid,
    kind: GridKind,
    effective: NormalizedRect,
    dimensions: (u32, u32),
) -> RegionTable {
    let table_type = match kind {
        GridKind::TitleBlock => crate::dto::TableType::TitleBlock,
        GridKind::DataTable | GridKind::Unknown => crate::dto::TableType::DataTable,
    };
    let matrix = grid.to_matrix_with_type(table_type);
    region_table_from_matrix(&matrix, kind, effective, dimensions)
}

#[cfg(feature = "native")]
fn region_table_from_matrix(
    matrix: &crate::dto::TableMatrix,
    kind: GridKind,
    effective: NormalizedRect,
    dimensions: (u32, u32),
) -> RegionTable {
    let (width, height) = dimensions;
    let classification = classify_table(matrix, Some(dimensions));
    let bbox = matrix
        .bbox
        .as_ref()
        .or_else(|| {
            matrix
                .rows
                .iter()
                .flatten()
                .find_map(|cell| cell.bbox.as_ref())
        })
        .map(|bbox| local_bbox_to_normalized(bbox, effective, width, height))
        .unwrap_or_default();
    RegionTable {
        kind,
        semantic_type: classification.semantic_type,
        semantic_confidence: classification.semantic_confidence,
        header_rows: classification.header_rows,
        column_roles: classification.column_roles,
        classification_evidence: classification.classification_evidence,
        bbox,
        rows: matrix.rows.len(),
        columns: matrix.rows.iter().map(Vec::len).max().unwrap_or(0),
        cells: matrix
            .rows
            .iter()
            .flatten()
            .filter_map(|cell| {
                cell.bbox.as_ref().map(|bbox| RegionTableCell {
                    row: cell.row as usize,
                    column: cell.column as usize,
                    row_span: cell.row_span as usize,
                    col_span: cell.col_span as usize,
                    bbox: local_bbox_to_normalized(bbox, effective, width, height),
                    text: cell.text.clone(),
                    confidence: cell.confidence,
                })
            })
            .collect(),
        confidence: matrix.confidence,
    }
}

#[cfg(feature = "native")]
fn word_is_assignable_to_matrix(word: &crate::dto::Word, matrix: &crate::dto::TableMatrix) -> bool {
    let center_x = (word.bbox.x0 + word.bbox.x1) / 2.0;
    let center_y = (word.bbox.y0 + word.bbox.y1) / 2.0;
    matrix.rows.iter().flatten().any(|cell| {
        cell.bbox.as_ref().is_some_and(|bbox| {
            bbox_intersection_area(&word.bbox, bbox) > 0.0
                || (center_x >= bbox.x0
                    && center_x <= bbox.x1
                    && center_y >= bbox.y0
                    && center_y <= bbox.y1)
        })
    })
}

#[cfg(feature = "native")]
fn bbox_intersection_area(left: &crate::dto::BoundingBox, right: &crate::dto::BoundingBox) -> f64 {
    let width = (left.x1.min(right.x1) - left.x0.max(right.x0)).max(0.0);
    let height = (left.y1.min(right.y1) - left.y0.max(right.y0)).max(0.0);
    width * height
}

#[cfg(feature = "native")]
fn translate_grid_to_page_pixels(
    grid: &mut crate::table_grid::TableGrid,
    offset_x: u32,
    offset_y: u32,
) {
    let dx = f64::from(offset_x);
    let dy = f64::from(offset_y);
    grid.bbox.x0 += dx;
    grid.bbox.x1 += dx;
    grid.bbox.y0 += dy;
    grid.bbox.y1 += dy;
}

#[cfg(any(feature = "native", test))]
fn words_to_text(words: &[RegionWord]) -> String {
    if words.is_empty() {
        return String::new();
    }
    let mut ordered = words.to_vec();
    ordered.sort_by(|left, right| {
        let left_mid = left.bbox.y + left.bbox.height / 2.0;
        let right_mid = right.bbox.y + right.bbox.height / 2.0;
        left_mid
            .total_cmp(&right_mid)
            .then_with(|| left.bbox.x.total_cmp(&right.bbox.x))
    });
    let mut heights = ordered
        .iter()
        .map(|word| word.bbox.height)
        .filter(|height| *height > 0.0)
        .collect::<Vec<_>>();
    heights.sort_by(f32::total_cmp);
    let median_height = heights.get(heights.len() / 2).copied().unwrap_or(0.01);
    let line_tolerance = (median_height * 0.65).max(0.002);

    let mut lines: Vec<(f32, Vec<RegionWord>)> = Vec::new();
    for word in ordered {
        let center_y = word.bbox.y + word.bbox.height / 2.0;
        if let Some((line_y, line)) = lines
            .iter_mut()
            .find(|(line_y, _)| (center_y - *line_y).abs() <= line_tolerance)
        {
            *line_y = (*line_y * line.len() as f32 + center_y) / (line.len() as f32 + 1.0);
            line.push(word);
        } else {
            lines.push((center_y, vec![word]));
        }
    }
    lines.sort_by(|left, right| left.0.total_cmp(&right.0));
    lines
        .into_iter()
        .map(|(_, mut line)| {
            line.sort_by(|left, right| left.bbox.x.total_cmp(&right.bbox.x));
            line.into_iter()
                .map(|word| word.text)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(feature = "native")]
fn elapsed_ms(started: std::time::Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000.0
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_minimum_pixel_extent, intersect_rect, validate_and_clip_rect, words_to_text,
        NormalizedRect, RegionCoordinateSpace, RegionMetrics, RegionResult, RegionWord,
        REGION_RESULT_SCHEMA_VERSION,
    };
    use crate::api::OcrError;
    use crate::page_analysis::{PageOrientation, PageStageTimings, ProcessingMode};

    #[test]
    fn region_cell_failure_warning_retains_sanitized_location_without_ocr_values() {
        let warning = super::region_cell_warning(
            2,
            &crate::pipeline::CellOcrFailure {
                row: 3,
                column: 4,
                reason_code: "recognition_failed",
            },
        );
        let value = serde_json::to_value(warning).unwrap();
        let message = value["message"].as_str().unwrap();
        assert_eq!(
            message,
            "table=2 row=3 column=4 reason_code=recognition_failed"
        );
    }

    #[test]
    fn rejects_non_finite_negative_and_empty_regions() {
        let nan = validate_and_clip_rect(NormalizedRect::new(f32::NAN, 0.0, 0.5, 0.5))
            .expect_err("NaN must be rejected");
        assert_eq!(nan.code(), "invalid_region");
        let negative = validate_and_clip_rect(NormalizedRect::new(-0.1, 0.0, 0.5, 0.5))
            .expect_err("negative coordinates must be rejected");
        assert_eq!(negative.code(), "invalid_region");
        assert!(matches!(
            validate_and_clip_rect(NormalizedRect::new(0.0, 0.0, 0.0, 0.5)),
            Err(OcrError::EmptyRegion)
        ));
    }

    #[test]
    fn clips_right_and_bottom_edges_explicitly() {
        let requested = NormalizedRect::new(0.8, 0.7, 0.5, 0.5);
        let (effective, clipped) =
            validate_and_clip_rect(requested).expect("partly intersecting ROI");
        assert!(clipped);
        assert!((effective.width - 0.2).abs() < 1.0e-6);
        assert!((effective.height - 0.3).abs() < 1.0e-6);
        assert_eq!((effective.x, effective.y), (0.8, 0.7));
    }

    #[test]
    fn rejects_region_starting_at_page_boundary() {
        let error = validate_and_clip_rect(NormalizedRect::new(1.0, 0.2, 0.2, 0.2))
            .expect_err("ROI has no area inside page");
        assert_eq!(error.code(), "region_outside_page");
    }

    #[test]
    fn intersection_is_page_normalized() {
        let intersection = intersect_rect(
            NormalizedRect::new(0.2, 0.2, 0.5, 0.5),
            NormalizedRect::new(0.5, 0.1, 0.4, 0.4),
        )
        .expect("rectangles overlap");
        assert_eq!((intersection.x, intersection.y), (0.5, 0.2));
        assert!((intersection.width - 0.2).abs() < 1.0e-6);
        assert!((intersection.height - 0.3).abs() < 1.0e-6);
    }

    #[test]
    fn words_are_reconstructed_in_line_order() {
        let words = vec![
            RegionWord {
                text: "two".into(),
                bbox: NormalizedRect::new(0.2, 0.1, 0.05, 0.02),
                confidence: 1.0,
            },
            RegionWord {
                text: "one".into(),
                bbox: NormalizedRect::new(0.1, 0.1, 0.05, 0.02),
                confidence: 1.0,
            },
            RegionWord {
                text: "three".into(),
                bbox: NormalizedRect::new(0.1, 0.2, 0.05, 0.02),
                confidence: 1.0,
            },
        ];
        assert_eq!(words_to_text(&words), "one two\nthree");
    }

    #[test]
    fn rejects_region_that_maps_below_two_pixels() {
        let error = ensure_minimum_pixel_extent(
            NormalizedRect::new(0.0, 0.0, 0.0001, 0.5),
            1_000,
            1_000,
            0,
        )
        .expect_err("sub-pixel width must be rejected");
        assert_eq!(error.code(), "invalid_region");
    }

    #[test]
    fn serializes_region_schema_without_debug_only_geometry() {
        let result = RegionResult {
            schema_version: REGION_RESULT_SCHEMA_VERSION,
            page_index: 0,
            requested_rect: NormalizedRect::new(0.1, 0.2, 0.3, 0.4),
            effective_rect: NormalizedRect::new(0.1, 0.2, 0.3, 0.4),
            coordinate_space: RegionCoordinateSpace::NormalizedTopLeft,
            processing_mode: ProcessingMode::NativeText,
            text: "<fixture>".to_string(),
            text_outside_tables: "<fixture>".to_string(),
            words: Vec::new(),
            tables: Vec::new(),
            orientation: PageOrientation::default(),
            language: "rus+eng".to_string(),
            native_text_quality: None,
            ocr_confidence: None,
            render_dpi: None,
            render_width: None,
            render_height: None,
            timings: PageStageTimings::default(),
            metrics: RegionMetrics::default(),
            warnings: Vec::new(),
        };
        let value = serde_json::to_value(result).expect("serialize region result");
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["coordinate_space"], "normalized_top_left");
        assert_eq!(value["processing_mode"], "native_text");
        assert!(value.get("debug").is_none());
    }
}
