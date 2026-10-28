//! Stable host-facing integration API for OCR Core v1.
//!
//! The existing DTOs remain the legacy document wire model. This module adds a
//! versioned projection with integer `(x, y, width, height)` geometry so an
//! embedding application does not need to depend on detector internals.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dto::{
    BoundingBox as LegacyBoundingBox, DocumentResult, TableDetectorType, TableMatrix, TableType,
};
use crate::export_docx::{export_docx_with_options, DocxExportOptions};
use crate::export_xlsx::{export_xlsx_with_options, XlsxExportOptions};
#[cfg(feature = "native")]
use crate::page_source::PageSource;
#[cfg(feature = "native")]
use crate::pipeline::{Pipeline, PipelineOutcome, PipelineWarning};
#[cfg(feature = "native")]
use crate::preprocessing::DEFAULT_MAX_PIXELS;
#[cfg(feature = "native")]
use crate::progress::{OcrProgressEvent, OcrProgressReporter, OcrProgressStage};
use crate::table_semantics::classify_table;
pub use crate::table_semantics::{
    ClassificationEvidence, ColumnRole, ColumnRoleAssignment, EvidenceKind, SemanticTableType,
};
#[cfg(feature = "native")]
use crate::{
    input_format::{InputLimits, InputSourceError},
    native::{
        djvu_source::DjVuSourceError,
        input_source::{map_djvu_error, InputSourceWarning, NativeInputError, OpenedPageSource},
    },
};

pub use crate::pipeline::TableMode;

/// Schema version emitted by [`OcrResult`] when serialized with
/// [`export_ocr_result_json`].
pub const OCR_RESULT_SCHEMA_VERSION: u32 = 1;
const DEFAULT_LANGUAGES: &str = "rus+eng";

/// The stable v1 name for the legacy document model retained in [`OcrResult`].
pub type DocumentModel = DocumentResult;

/// Structural/technical role of a table-like region in the host-facing
/// contract. Subject-domain classification is stored separately in
/// [`DetectedGrid::semantic_type`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GridKind {
    DataTable,
    TitleBlock,
    /// A legacy word-alignment table that does not provide verified grid
    /// geometry. Its original representation remains available in
    /// [`OcrResult::document`].
    Unknown,
}

/// Integer pixel geometry used by the stable host-facing API.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoundingBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl BoundingBox {
    fn from_legacy(bbox: &LegacyBoundingBox) -> Self {
        let x0 = finite_non_negative_u32(bbox.x0);
        let y0 = finite_non_negative_u32(bbox.y0);
        let x1 = finite_non_negative_u32(bbox.x1);
        let y1 = finite_non_negative_u32(bbox.y1);
        Self {
            x: x0,
            y: y0,
            width: x1.saturating_sub(x0),
            height: y1.saturating_sub(y0),
        }
    }
}

/// One cell in a [`DetectedGrid`]. Grid cells always have verified geometry.
/// For [`GridKind::Unknown`] legacy cells without word coordinates, `bbox` is
/// the zero rectangle and callers should rely on the legacy document model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableCell {
    pub row: usize,
    pub column: usize,
    #[serde(default = "default_span")]
    pub row_span: usize,
    #[serde(default = "default_span")]
    pub col_span: usize,
    pub bbox: BoundingBox,
    pub text: String,
    pub confidence: f32,
}

impl Default for TableCell {
    fn default() -> Self {
        Self {
            row: 0,
            column: 0,
            row_span: 1,
            col_span: 1,
            bbox: BoundingBox::default(),
            text: String::new(),
            confidence: 0.0,
        }
    }
}

const fn default_span() -> usize {
    1
}

/// Stable projection of one detected table or title block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DetectedGrid {
    pub kind: GridKind,
    /// Additive probabilistic subject-domain classification. This never
    /// changes `kind` or legacy export routing.
    #[serde(default)]
    pub semantic_type: SemanticTableType,
    #[serde(default)]
    pub semantic_confidence: f32,
    /// Zero-based rows recognized as column-heading rows.
    #[serde(default)]
    pub header_rows: Vec<usize>,
    /// One entry per physical column; uncertain roles are `unknown`.
    #[serde(default)]
    pub column_roles: Vec<ColumnRoleAssignment>,
    /// Stable rule identifiers and bounded scores; source OCR text is omitted.
    #[serde(default)]
    pub classification_evidence: Vec<ClassificationEvidence>,
    pub bbox: BoundingBox,
    pub rows: usize,
    pub columns: usize,
    pub cells: Vec<TableCell>,
    pub confidence: f32,
}

/// All detected table regions belonging to one document page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageTables {
    pub page_number: u32,
    pub grids: Vec<DetectedGrid>,
}

/// Runtime controls for the host-facing processing API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OcrOptions {
    pub table_mode: TableMode,
    /// Controls DOCX/XLSX projection only. Title blocks are always retained in
    /// [`OcrResult`] and its JSON serialization.
    pub export_title_blocks: bool,
    /// Overlay production is deliberately not part of the v1 runtime pipeline.
    /// Setting this to `true` returns a structured warning rather than silently
    /// creating no artifacts.
    pub generate_overlays: bool,
    /// Backward-compatible full-page OCR density. DPI is only an input limit,
    /// not a document-independent quality guarantee: identical DPI values can
    /// produce different OCR and grid results for different source documents.
    #[deprecated(note = "use OcrOptions::ocr_dpi")]
    pub dpi: u32,
    /// Full-resolution density for OCR-only pages.
    pub ocr_dpi: u32,
    /// Initial low-resolution density for ruled-grid detection on searchable
    /// pages. Validate it on representative documents rather than treating one
    /// value as universally optimal.
    pub grid_dpi: u32,
    /// Maximum number of CPU page workers; PDFium access remains serialized.
    /// Four is the current production default; callers may increase it after
    /// benchmarking their own workstation and documents.
    pub max_page_workers: usize,
    /// OCR visually non-empty cells that received no positioned native text.
    pub enable_cell_ocr_fallback: bool,
    /// Explicit Tesseract language data selection, for example `rus+eng`.
    /// The default deliberately matches bilingual engineering documents.
    pub languages: String,
    /// Opt-in per-page Chinese routing. Older serialized options retain the
    /// configured language route; native-text evidence is used only in AutoChinese.
    #[serde(default)]
    pub language_routing: crate::language::LanguageRouting,
    /// Backward-compatible spelling retained for v1 embedding callers.
    /// New callers should use [`OcrOptions::languages`]. If only this field is
    /// changed from its default, it remains effective; conflicting explicit
    /// values are rejected instead of choosing a language silently.
    #[deprecated(note = "use OcrOptions::languages")]
    pub language: String,
    /// Optional directory containing `*.traineddata`. When omitted, the
    /// process-local `TESSDATA_PREFIX` is used, then the bundled runtime path.
    pub tessdata_path: Option<PathBuf>,
    pub page_limit: Option<u32>,
    /// Optional explicit path to the PDFium 7881 DLL. When omitted, the
    /// documented bundled runtime path is used.
    pub pdfium_dll_path: Option<PathBuf>,
}

impl Default for OcrOptions {
    fn default() -> Self {
        Self {
            table_mode: TableMode::GridWithFallback,
            // Preserve the existing behavior: title blocks are exported only
            // into separate sheets/sections, never merged with data tables.
            export_title_blocks: true,
            generate_overlays: false,
            #[allow(deprecated)]
            dpi: 300,
            ocr_dpi: 300,
            grid_dpi: 100,
            max_page_workers: 4,
            enable_cell_ocr_fallback: true,
            languages: DEFAULT_LANGUAGES.to_string(),
            language_routing: crate::language::LanguageRouting::Configured,
            #[allow(deprecated)]
            language: DEFAULT_LANGUAGES.to_string(),
            tessdata_path: None,
            page_limit: None,
            pdfium_dll_path: None,
        }
    }
}

impl OcrOptions {
    #[allow(deprecated)]
    pub fn effective_ocr_dpi(&self) -> Result<u32, String> {
        let legacy_changed = self.dpi != 300;
        let explicit_changed = self.ocr_dpi != 300;
        if legacy_changed && explicit_changed && self.dpi != self.ocr_dpi {
            return Err(format!(
                "conflicting OCR DPI settings: dpi={}, ocr_dpi={}",
                self.dpi, self.ocr_dpi
            ));
        }
        Ok(if legacy_changed && !explicit_changed {
            self.dpi
        } else {
            self.ocr_dpi
        })
    }

    /// Resolves the v1 compatibility field and the explicit v1.1 language
    /// setting without silently choosing between two conflicting values.
    #[allow(deprecated)]
    pub fn effective_languages(&self) -> Result<&str, String> {
        let legacy_changed = self.language != DEFAULT_LANGUAGES;
        let explicit_changed = self.languages != DEFAULT_LANGUAGES;
        if legacy_changed && explicit_changed && self.language != self.languages {
            return Err(format!(
                "conflicting OCR language settings: language={:?}, languages={:?}",
                self.language, self.languages
            ));
        }
        if legacy_changed && !explicit_changed {
            Ok(&self.language)
        } else {
            Ok(&self.languages)
        }
    }
}

/// Non-fatal conditions returned to an embedding application.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OcrWarning {
    /// A local supplemental recognition failure; the existing cell is retained.
    CellOcrFallbackFailed {
        page_number: u32,
        table_index: usize,
        row: u32,
        column: u32,
        reason_code: String,
    },
    GridDetectorFailed {
        page_number: u32,
        message: String,
    },
    NativeTextUnavailable {
        page_number: u32,
        message: String,
    },
    NativeTextLayoutUnavailable {
        page_number: u32,
        message: String,
    },
    NativeWordGeometryUnavailable {
        page_number: u32,
        message: String,
    },
    TableRoutingPreflightFailed {
        page_number: u32,
        message: String,
    },
    OrientationDetectionFailed {
        page_number: u32,
        message: String,
    },
    AdditionalImageFramesIgnored {
        format: String,
        total_frames: usize,
        processed_frames: usize,
    },
    OverlaysUnsupported,
}

/// Versioned processing result returned by [`process_document`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    pub schema_version: u32,
    /// Canonical OCR document retained for backward-compatible exports.
    pub document: DocumentModel,
    /// Read-only stable v1 projection of the canonical `document` table
    /// matrices. It is for host inspection and serialization, not an editable
    /// source for legacy DOCX/XLSX exports.
    pub tables: Vec<PageTables>,
    /// Additive v1.1 processing diagnostics. `#[serde(default)]` preserves
    /// deserialization compatibility with schema-v1 envelopes emitted by v1.0.
    #[serde(default)]
    pub page_processing: Vec<crate::page_analysis::PageProcessingInfo>,
    pub warnings: Vec<OcrWarning>,
}

impl OcrResult {
    /// Creates the stable v1 projection from the existing document model.
    pub fn from_document(document: DocumentModel, warnings: Vec<OcrWarning>) -> Self {
        let mut tables: Vec<PageTables> = document
            .pages
            .iter()
            .map(|page| PageTables {
                page_number: page.page_number,
                grids: page
                    .tables
                    .iter()
                    .map(|matrix| DetectedGrid::from_matrix(matrix, None))
                    .collect(),
            })
            .collect();
        propagate_adjacent_table_semantics(&mut tables);

        Self {
            schema_version: OCR_RESULT_SCHEMA_VERSION,
            document,
            tables,
            page_processing: Vec::new(),
            warnings,
        }
    }

    #[cfg(feature = "native")]
    fn from_pipeline_outcome(outcome: PipelineOutcome, warnings: Vec<OcrWarning>) -> Self {
        let PipelineOutcome {
            document,
            page_processing,
            ..
        } = outcome;
        let mut result = Self::from_document(document, warnings);
        result.page_processing = page_processing;
        result.tables = result
            .document
            .pages
            .iter()
            .map(|page| {
                let page_size = result
                    .page_processing
                    .iter()
                    .find(|processing| processing.page_number == page.page_number)
                    .and_then(|processing| processing.render_size)
                    .map(|size| (size.width, size.height));
                PageTables {
                    page_number: page.page_number,
                    grids: page
                        .tables
                        .iter()
                        .map(|matrix| DetectedGrid::from_matrix(matrix, page_size))
                        .collect(),
                }
            })
            .collect();
        propagate_adjacent_table_semantics(&mut result.tables);
        result
    }
}

impl DetectedGrid {
    fn from_matrix(matrix: &TableMatrix, page_size: Option<(u32, u32)>) -> Self {
        let kind = match matrix.detector_type {
            TableDetectorType::Grid | TableDetectorType::Borderless => match matrix.table_type {
                TableType::DataTable => GridKind::DataTable,
                TableType::TitleBlock => GridKind::TitleBlock,
            },
            TableDetectorType::WordAlignmentFallback => GridKind::Unknown,
        };
        let rows = matrix.rows.len();
        let columns = matrix.rows.iter().map(Vec::len).max().unwrap_or(0);
        let bbox = matrix
            .bbox
            .clone()
            .or_else(|| matrix_bbox_from_cells(matrix))
            .as_ref()
            .map(BoundingBox::from_legacy)
            .unwrap_or_default();
        let cells = matrix
            .rows
            .iter()
            .flat_map(|row| row.iter())
            .map(|cell| TableCell {
                row: cell.row as usize,
                column: cell.column as usize,
                row_span: cell.row_span as usize,
                col_span: cell.col_span as usize,
                bbox: cell
                    .bbox
                    .as_ref()
                    .map(BoundingBox::from_legacy)
                    .unwrap_or_default(),
                text: cell.text.clone(),
                confidence: cell.confidence,
            })
            .collect();

        let classification = classify_table(matrix, page_size);
        Self {
            kind,
            semantic_type: classification.semantic_type,
            semantic_confidence: classification.semantic_confidence,
            header_rows: classification.header_rows,
            column_roles: classification.column_roles,
            classification_evidence: classification.classification_evidence,
            bbox,
            rows,
            columns,
            cells,
            confidence: matrix.confidence,
        }
    }
}

/// Carries a confident subject-domain type across adjacent pages only when
/// their physical column geometry is strongly compatible.
///
/// Engineering specifications commonly repeat the ruled layout on continuation
/// pages while omitting the heading row. Classifying every page independently
/// therefore turns a valid continuation into `generic_table`. This projection
/// keeps the page-local grid and OCR data unchanged; it only adds conservative
/// semantic evidence to the stable JSON view.
fn propagate_adjacent_table_semantics(pages: &mut [PageTables]) {
    let mut order = (0..pages.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| pages[*index].page_number);

    for pair in order.windows(2) {
        let previous_index = pair[0];
        let current_index = pair[1];
        if pages[current_index].page_number != pages[previous_index].page_number.saturating_add(1) {
            continue;
        }

        // Cloning the compact stable projection avoids aliasing two pages and
        // intentionally lets an accepted continuation seed the next page.
        let previous_grids = pages[previous_index].grids.clone();
        for current in &mut pages[current_index].grids {
            if current.kind != GridKind::DataTable
                || !matches!(
                    current.semantic_type,
                    SemanticTableType::GenericTable | SemanticTableType::Unknown
                )
            {
                continue;
            }

            let best = previous_grids
                .iter()
                .filter(|previous| {
                    previous.kind == GridKind::DataTable
                        && is_continuable_semantic_type(previous.semantic_type)
                        && previous.semantic_confidence >= 0.66
                })
                .filter_map(|previous| {
                    continuation_geometry_score(previous, current).map(|score| (previous, score))
                })
                .max_by(|left, right| left.1.total_cmp(&right.1));

            let Some((previous, geometry_score)) = best else {
                continue;
            };
            let confidence = (previous.semantic_confidence * geometry_score * 0.95)
                .clamp(0.0, previous.semantic_confidence.min(0.88));
            if confidence < 0.60 {
                continue;
            }

            current.semantic_type = previous.semantic_type;
            current.semantic_confidence = confidence;
            current
                .classification_evidence
                .push(ClassificationEvidence {
                    kind: EvidenceKind::SemanticRule,
                    rule: "semantic:adjacent_page_continuation_geometry".to_string(),
                    row: None,
                    column: None,
                    score: geometry_score,
                });
        }
    }
}

fn is_continuable_semantic_type(semantic_type: SemanticTableType) -> bool {
    matches!(
        semantic_type,
        SemanticTableType::MaterialSpecification
            | SemanticTableType::EquipmentSchedule
            | SemanticTableType::CableJournal
            | SemanticTableType::WorkEstimate
            | SemanticTableType::CommercialOffer
    )
}

fn continuation_geometry_score(previous: &DetectedGrid, current: &DetectedGrid) -> Option<f32> {
    if previous.columns < 3
        || previous.columns != current.columns
        || previous.rows < 2
        || current.rows < 2
        || previous.confidence < 0.40
        || current.confidence < 0.40
        || previous.bbox.width == 0
        || current.bbox.width == 0
    {
        return None;
    }

    let previous_width = previous.bbox.width as f32;
    let current_width = current.bbox.width as f32;
    let width_similarity = previous_width.min(current_width) / previous_width.max(current_width);
    if width_similarity < 0.82 {
        return None;
    }
    let left_delta = previous.bbox.x.abs_diff(current.bbox.x) as f32
        / previous_width.max(current_width).max(1.0);
    if left_delta > 0.12 {
        return None;
    }

    let previous_centers = normalized_column_centers(previous)?;
    let current_centers = normalized_column_centers(current)?;
    let deltas = previous_centers
        .iter()
        .zip(current_centers.iter())
        .map(|(left, right)| (left - right).abs())
        .collect::<Vec<_>>();
    let mean_delta = deltas.iter().sum::<f32>() / deltas.len() as f32;
    let max_delta = deltas.iter().copied().fold(0.0_f32, f32::max);
    if mean_delta > 0.055 || max_delta > 0.10 {
        return None;
    }

    let alignment = (1.0 - mean_delta / 0.055).clamp(0.0, 1.0);
    let location = (1.0 - left_delta / 0.12).clamp(0.0, 1.0);
    Some((width_similarity * 0.45 + alignment * 0.40 + location * 0.15).clamp(0.0, 1.0))
}

fn normalized_column_centers(grid: &DetectedGrid) -> Option<Vec<f32>> {
    let left = grid.bbox.x as f32;
    let width = grid.bbox.width as f32;
    let mut result = Vec::with_capacity(grid.columns);
    for column in 0..grid.columns {
        let mut centers = grid
            .cells
            .iter()
            .filter(|cell| cell.column == column && cell.col_span == 1 && cell.bbox.width > 0)
            .map(|cell| {
                let center = cell.bbox.x as f32 + cell.bbox.width as f32 / 2.0;
                ((center - left) / width).clamp(0.0, 1.0)
            })
            .collect::<Vec<_>>();
        if centers.is_empty() {
            return None;
        }
        centers.sort_by(f32::total_cmp);
        result.push(centers[centers.len() / 2]);
    }
    Some(result)
}

/// Errors returned by the stable host-facing API.
#[derive(Debug, Error)]
pub enum OcrError {
    #[error("the 'native' feature is required to process real documents")]
    NativeFeatureRequired,
    #[error("invalid OCR options: {0}")]
    InvalidOptions(String),
    #[error("page index {page_index} is out of range for {page_count} pages")]
    InvalidPageIndex {
        page_index: usize,
        page_count: usize,
    },
    #[error("invalid normalized region: {reason}")]
    InvalidRegion { reason: String },
    #[error("normalized region has zero width or height")]
    EmptyRegion,
    #[error("normalized region does not intersect the visible page")]
    RegionOutsidePage,
    #[error("failed to open PDF {path}: {detail}")]
    PdfOpenFailed { path: PathBuf, detail: String },
    #[error("unsupported input format: {path}")]
    UnsupportedInputFormat { path: PathBuf },
    #[error("failed to read input {path}: {detail}")]
    InputReadFailed { path: PathBuf, detail: String },
    #[error("image decode failed for {path}: {detail}")]
    ImageDecodeFailed { path: PathBuf, detail: String },
    #[error("HEIF decode failed for {path}: {detail}")]
    HeifDecodeFailed { path: PathBuf, detail: String },
    #[error("DjVu decode failed for {path}: {detail}")]
    DjvuDecodeFailed { path: PathBuf, detail: String },
    #[error(
        "image frame {frame_number} is {width}x{height} ({pixels} pixels), exceeding the limit of {max_pixels}"
    )]
    ImagePixelLimitExceeded {
        frame_number: u32,
        width: u32,
        height: u32,
        pixels: u64,
        max_pixels: u64,
    },
    #[error("input contains {frames} frames, exceeding the limit of {max_frames}")]
    ImageFrameLimitExceeded { frames: u32, max_frames: u32 },
    #[error("input file is {bytes} bytes, exceeding the limit of {max_bytes}")]
    InputFileSizeLimitExceeded { bytes: u64, max_bytes: u64 },
    #[error("failed to render page {page_index} region: {detail}")]
    RenderFailed { page_index: usize, detail: String },
    #[error("failed to extract native text from page {page_index}: {detail}")]
    NativeTextFailed { page_index: usize, detail: String },
    #[error("OCR failed for page {page_index} region: {detail}")]
    OcrFailed { page_index: usize, detail: String },
    #[error("table detection failed for page {page_index} region: {detail}")]
    TableDetectionFailed { page_index: usize, detail: String },
    #[error("failed to initialize {component}: {detail}")]
    NativeInitialization {
        component: &'static str,
        detail: String,
    },
    #[error("OCR pipeline failed: {0}")]
    Pipeline(String),
    #[error("failed to export {format} to {path}: {detail}")]
    Export {
        format: &'static str,
        path: PathBuf,
        detail: String,
    },
}

impl OcrError {
    /// Stable machine-readable error identifier for host integrations.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NativeFeatureRequired => "native_feature_required",
            Self::InvalidOptions(_) => "invalid_options",
            Self::InvalidPageIndex { .. } => "invalid_page_index",
            Self::InvalidRegion { .. } => "invalid_region",
            Self::EmptyRegion => "empty_region",
            Self::RegionOutsidePage => "region_outside_page",
            Self::PdfOpenFailed { .. } => "pdf_open_failed",
            Self::UnsupportedInputFormat { .. } => "unsupported_input_format",
            Self::InputReadFailed { .. } => "input_read_failed",
            Self::ImageDecodeFailed { .. } => "image_decode_failed",
            Self::HeifDecodeFailed { .. } => "heif_decode_failed",
            Self::DjvuDecodeFailed { .. } => "djvu_decode_failed",
            Self::ImagePixelLimitExceeded { .. } => "image_pixel_limit_exceeded",
            Self::ImageFrameLimitExceeded { .. } => "image_frame_limit_exceeded",
            Self::InputFileSizeLimitExceeded { .. } => "input_file_size_limit_exceeded",
            Self::RenderFailed { .. } => "render_failed",
            Self::NativeTextFailed { .. } => "native_text_failed",
            Self::OcrFailed { .. } => "ocr_failed",
            Self::TableDetectionFailed { .. } => "table_detection_failed",
            Self::NativeInitialization { .. } => "native_initialization_failed",
            Self::Pipeline(_) => "pipeline_failed",
            Self::Export { .. } => "export_failed",
        }
    }
}

/// Processes a supported document entirely in-process, without invoking the
/// CLI. Format selection is based on the input signature, not its extension.
///
/// The `native` feature must be enabled. The returned document and all title
/// blocks remain in memory; callers choose which projections to export.
#[cfg(feature = "native")]
pub fn process_document(input: &Path, options: &OcrOptions) -> Result<OcrResult, OcrError> {
    process_document_with_progress(input, options, None)
}

/// Processes a document and optionally emits transport-independent progress
/// records. The schema-v1 result remains unchanged; progress is additive.
#[cfg(feature = "native")]
pub fn process_document_with_progress(
    input: &Path,
    options: &OcrOptions,
    progress: Option<&dyn OcrProgressReporter>,
) -> Result<OcrResult, OcrError> {
    validate_options(input, options)?;
    let languages = options
        .effective_languages()
        .map_err(OcrError::InvalidOptions)?;
    let ocr_dpi = options
        .effective_ocr_dpi()
        .map_err(OcrError::InvalidOptions)?;

    let OpenedPageSource {
        mut source,
        warnings: source_warnings,
        ..
    } = OpenedPageSource::open(
        input,
        f64::from(ocr_dpi),
        options.pdfium_dll_path.as_deref(),
        InputLimits::default(),
    )
    .map_err(ocr_error_from_native_input)?;
    let total_pages = source
        .total_pages()
        .map_err(|error| OcrError::Pipeline(error.to_string()))?;
    let effective_pages = options
        .page_limit
        .map_or(total_pages, |limit| limit.min(total_pages));
    if let Some(reporter) = progress {
        reporter.report(&OcrProgressEvent::document(
            OcrProgressStage::DocumentOpen,
            effective_pages,
            0.0,
        ));
    }
    let initial_profile =
        crate::language::select_language_profile(options.language_routing, languages, None);
    let initial_languages = crate::language::effective_languages(languages, initial_profile);
    let ocr = crate::native::tesseract_engine::TesseractOcrEngine::new_lazy_with_tessdata(
        &initial_languages,
        options.tessdata_path.as_deref(),
    )
    .map_err(|error| OcrError::NativeInitialization {
        component: "Tesseract",
        detail: error.to_string(),
    })?;
    let outcome = Pipeline::new(&mut source, &ocr, DEFAULT_MAX_PIXELS, options.page_limit)
        .with_table_mode(options.table_mode)
        .with_languages(languages)
        .with_language_routing(options.language_routing)
        .with_grid_dpi(options.grid_dpi)
        .with_cell_ocr_dpi(ocr_dpi)
        .with_cell_ocr_fallback(options.enable_cell_ocr_fallback)
        .with_progress(progress)
        .run_with_warnings_parallel(options.max_page_workers);
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            if let Some(reporter) = progress {
                reporter.report(&OcrProgressEvent::failure(
                    None,
                    effective_pages,
                    "pipeline_failed",
                ));
            }
            return Err(ocr_error_from_pipeline(input, error));
        }
    };

    let mut warnings = source_warnings
        .into_iter()
        .map(ocr_warning_from_input_source)
        .chain(
            outcome
                .warnings
                .iter()
                .cloned()
                .map(ocr_warning_from_pipeline),
        )
        .collect::<Vec<_>>();
    if options.generate_overlays {
        warnings.push(OcrWarning::OverlaysUnsupported);
    }

    // The CLI emits `export` and the terminal `completed` event only after the
    // output transaction has published every artifact. The library returns the
    // aggregated result without claiming publication succeeded.
    Ok(OcrResult::from_pipeline_outcome(outcome, warnings))
}

/// Stub-build form of [`process_document`]. It keeps the public signature
/// stable while making the missing native capability explicit.
#[cfg(not(feature = "native"))]
pub fn process_document(_input: &Path, _options: &OcrOptions) -> Result<OcrResult, OcrError> {
    Err(OcrError::NativeFeatureRequired)
}
/// Stub-build counterpart of [`process_document_with_progress`].
#[cfg(not(feature = "native"))]
pub fn process_document_with_progress(
    _input: &Path,
    _options: &OcrOptions,
    _progress: Option<&dyn crate::progress::OcrProgressReporter>,
) -> Result<OcrResult, OcrError> {
    Err(OcrError::NativeFeatureRequired)
}

/// Writes the stable schema-v1 result envelope as pretty-printed JSON.
pub fn export_ocr_result_json(result: &OcrResult, output: &Path) -> Result<(), OcrError> {
    let body = serde_json::to_string_pretty(result).map_err(|error| OcrError::Export {
        format: "JSON",
        path: output.to_path_buf(),
        detail: error.to_string(),
    })?;
    std::fs::write(output, body).map_err(|error| OcrError::Export {
        format: "JSON",
        path: output.to_path_buf(),
        detail: error.to_string(),
    })
}

/// Writes the pre-v1 document JSON shape for existing CLI consumers.
///
/// New integrations should prefer [`export_ocr_result_json`]. DOCX/XLSX
/// exports intentionally use the canonical legacy document to preserve all
/// existing layout fields, while `tables` remains the stable inspection view.
pub fn export_legacy_document_json(result: &OcrResult, output: &Path) -> Result<(), OcrError> {
    crate::export_json::export_json(&result.document, output).map_err(|error| OcrError::Export {
        format: "legacy JSON",
        path: output.to_path_buf(),
        detail: error.to_string(),
    })
}

/// Writes XLSX from the stable result while applying the title-block export
/// policy. The in-memory result itself is never filtered.
pub fn export_ocr_result_xlsx(
    result: &OcrResult,
    output: &Path,
    export_title_blocks: bool,
) -> Result<(), OcrError> {
    let tables = document_tables(&result.document, export_title_blocks);
    export_xlsx_with_options(
        &tables,
        output,
        XlsxExportOptions {
            include_title_blocks: export_title_blocks,
        },
    )
    .map_err(|error| OcrError::Export {
        format: "XLSX",
        path: output.to_path_buf(),
        detail: error.to_string(),
    })
}

/// Writes DOCX from the stable result while applying the title-block export
/// policy. All data tables on a page are exported in reading order.
pub fn export_ocr_result_docx(
    result: &OcrResult,
    output: &Path,
    export_title_blocks: bool,
) -> Result<(), OcrError> {
    export_docx_with_options(
        &result.document.pages,
        output,
        DocxExportOptions {
            include_title_blocks: export_title_blocks,
        },
    )
    .map_err(|error| OcrError::Export {
        format: "DOCX",
        path: output.to_path_buf(),
        detail: error.to_string(),
    })
}

#[cfg(feature = "native")]
pub(crate) fn validate_options(input: &Path, options: &OcrOptions) -> Result<(), OcrError> {
    if !input.is_file() {
        return Err(OcrError::InvalidOptions(format!(
            "input file not found: {}",
            input.display()
        )));
    }
    let ocr_dpi = options
        .effective_ocr_dpi()
        .map_err(OcrError::InvalidOptions)?;
    if !(150..=400).contains(&ocr_dpi) {
        return Err(OcrError::InvalidOptions(format!(
            "DPI must be between 150 and 400, got {}",
            ocr_dpi
        )));
    }
    if !(72..=200).contains(&options.grid_dpi) {
        return Err(OcrError::InvalidOptions(format!(
            "grid_dpi must be between 72 and 200, got {}",
            options.grid_dpi
        )));
    }
    if options.max_page_workers == 0 || options.max_page_workers > 64 {
        return Err(OcrError::InvalidOptions(format!(
            "max_page_workers must be between 1 and 64, got {}",
            options.max_page_workers
        )));
    }
    if options
        .effective_languages()
        .map_err(OcrError::InvalidOptions)?
        .trim()
        .is_empty()
    {
        return Err(OcrError::InvalidOptions(
            "OCR languages must not be empty".to_string(),
        ));
    }
    if let Some(path) = &options.tessdata_path {
        if !path.is_dir() {
            return Err(OcrError::InvalidOptions(format!(
                "tessdata directory not found: {}",
                path.display()
            )));
        }
    }
    if let Some(path) = &options.pdfium_dll_path {
        if !path.is_file() {
            return Err(OcrError::InvalidOptions(format!(
                "PDFium DLL not found: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn document_tables(
    document: &DocumentModel,
    export_title_blocks: bool,
) -> Vec<(u32, &TableMatrix)> {
    document
        .pages
        .iter()
        .flat_map(|page| {
            page.tables.iter().filter_map(move |matrix| {
                if export_title_blocks || matrix.table_type != TableType::TitleBlock {
                    Some((page.page_number, matrix))
                } else {
                    None
                }
            })
        })
        .collect()
}

fn matrix_bbox_from_cells(matrix: &TableMatrix) -> Option<LegacyBoundingBox> {
    let mut boxes = matrix
        .rows
        .iter()
        .flat_map(|row| row.iter())
        .filter_map(|cell| cell.bbox.as_ref());
    let first = boxes.next()?.clone();
    Some(boxes.fold(first, |union, bbox| {
        LegacyBoundingBox::new(
            union.x0.min(bbox.x0),
            union.y0.min(bbox.y0),
            union.x1.max(bbox.x1),
            union.y1.max(bbox.y1),
        )
    }))
}

fn finite_non_negative_u32(value: f64) -> u32 {
    if value.is_finite() && value > 0.0 {
        value.round().min(f64::from(u32::MAX)) as u32
    } else {
        0
    }
}

#[cfg(feature = "native")]
fn ocr_warning_from_pipeline(warning: PipelineWarning) -> OcrWarning {
    match warning {
        PipelineWarning::CellOcrFallbackFailed {
            page_number,
            table_index,
            row,
            column,
            reason_code,
        } => OcrWarning::CellOcrFallbackFailed {
            page_number,
            table_index,
            row,
            column,
            reason_code,
        },
        PipelineWarning::GridDetectorFailed {
            page_number,
            message,
        } => OcrWarning::GridDetectorFailed {
            page_number,
            message,
        },
        PipelineWarning::NativeTextUnavailable {
            page_number,
            message,
        } => OcrWarning::NativeTextUnavailable {
            page_number,
            message,
        },
        PipelineWarning::NativeTextLayoutUnavailable {
            page_number,
            message,
        } => OcrWarning::NativeTextLayoutUnavailable {
            page_number,
            message,
        },
        PipelineWarning::NativeWordGeometryUnavailable {
            page_number,
            message,
        } => OcrWarning::NativeWordGeometryUnavailable {
            page_number,
            message,
        },
        PipelineWarning::TableRoutingPreflightFailed {
            page_number,
            message,
        } => OcrWarning::TableRoutingPreflightFailed {
            page_number,
            message,
        },
        PipelineWarning::OrientationDetectionFailed {
            page_number,
            message,
        } => OcrWarning::OrientationDetectionFailed {
            page_number,
            message,
        },
    }
}

#[cfg(feature = "native")]
fn ocr_warning_from_input_source(warning: InputSourceWarning) -> OcrWarning {
    match warning {
        InputSourceWarning::AdditionalImageFramesIgnored {
            format,
            total_frames,
            processed_frames,
        } => OcrWarning::AdditionalImageFramesIgnored {
            format: format.to_string(),
            total_frames,
            processed_frames,
        },
    }
}

#[cfg(feature = "native")]
fn ocr_error_from_native_input(error: NativeInputError) -> OcrError {
    match error {
        NativeInputError::PdfOpenFailed { path, detail } => {
            OcrError::PdfOpenFailed { path, detail }
        }
        NativeInputError::Source(error) => match error {
            InputSourceError::UnsupportedInputFormat { path } => {
                OcrError::UnsupportedInputFormat { path }
            }
            InputSourceError::InputReadFailed { path, detail } => {
                OcrError::InputReadFailed { path, detail }
            }
            InputSourceError::ImageDecodeFailed { path, detail } => {
                OcrError::ImageDecodeFailed { path, detail }
            }
            InputSourceError::HeifDecodeFailed { path, detail } => {
                OcrError::HeifDecodeFailed { path, detail }
            }
            InputSourceError::DjvuDecodeFailed { path, detail } => {
                OcrError::DjvuDecodeFailed { path, detail }
            }
            InputSourceError::ImagePixelLimitExceeded {
                frame_number,
                width,
                height,
                pixels,
                max_pixels,
            } => OcrError::ImagePixelLimitExceeded {
                frame_number,
                width,
                height,
                pixels,
                max_pixels,
            },
            InputSourceError::ImageFrameLimitExceeded { frames, max_frames } => {
                OcrError::ImageFrameLimitExceeded { frames, max_frames }
            }
            InputSourceError::InputFileSizeLimitExceeded { bytes, max_bytes } => {
                OcrError::InputFileSizeLimitExceeded { bytes, max_bytes }
            }
            InputSourceError::PageOutOfRange {
                page_number,
                page_count,
            } => OcrError::InvalidPageIndex {
                page_index: page_number.saturating_sub(1) as usize,
                page_count: page_count as usize,
            },
        },
    }
}

#[cfg(feature = "native")]
fn ocr_error_from_pipeline(input: &Path, error: Box<dyn std::error::Error>) -> OcrError {
    let error = match error.downcast::<InputSourceError>() {
        Ok(source_error) => {
            return ocr_error_from_native_input(NativeInputError::Source(*source_error));
        }
        Err(error) => error,
    };
    let error = match error.downcast::<DjVuSourceError>() {
        Ok(source_error) => {
            return ocr_error_from_native_input(NativeInputError::Source(map_djvu_error(
                input,
                *source_error,
            )));
        }
        Err(error) => error,
    };
    OcrError::Pipeline(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        export_ocr_result_json, BoundingBox, ColumnRole, ColumnRoleAssignment, DetectedGrid,
        GridKind, OcrOptions, OcrResult, SemanticTableType, TableCell, TableMode,
        OCR_RESULT_SCHEMA_VERSION,
    };
    #[cfg(feature = "native")]
    use super::{ocr_error_from_pipeline, OcrError};
    use crate::dto::{
        BoundingBox as LegacyBoundingBox, DocumentResult, PageKind, PageResult, TableMatrix,
        TableType,
    };
    use crate::page_analysis::{
        PageOrientation, PageProcessingInfo, PageStageTimings, PageWordAssignmentMetrics,
        ProcessingMode, RenderSize,
    };
    use tempfile::tempdir;

    #[test]
    fn cell_fallback_warning_is_additive_schema_v1_and_contains_only_location_and_code() {
        let warning = super::OcrWarning::CellOcrFallbackFailed {
            page_number: 2,
            table_index: 0,
            row: 3,
            column: 1,
            reason_code: "recognition_failed".into(),
        };
        let value = serde_json::to_value(&warning).unwrap();
        assert_eq!(value["kind"], "cell_ocr_fallback_failed");
        assert_eq!(value["page_number"], 2);
        assert_eq!(value["row"], 3);
        assert_eq!(value["column"], 1);
        assert!(value.get("text").is_none());
        assert!(value.get("message").is_none());
        assert_eq!(
            serde_json::from_value::<super::OcrWarning>(value).unwrap(),
            warning
        );
        assert_eq!(OCR_RESULT_SCHEMA_VERSION, 1);
    }

    #[test]
    #[allow(deprecated)]
    fn defaults_to_grid_with_fallback() {
        assert_eq!(
            OcrOptions::default().table_mode,
            TableMode::GridWithFallback
        );
        assert_eq!(OcrOptions::default().languages, "rus+eng");
        assert_eq!(OcrOptions::default().language, "rus+eng");
        assert_eq!(OcrOptions::default().dpi, 300);
        assert_eq!(OcrOptions::default().max_page_workers, 4);
    }

    #[test]
    #[allow(deprecated)]
    fn legacy_language_field_remains_effective_when_new_field_is_default() {
        let options = OcrOptions {
            language: "eng".to_string(),
            ..OcrOptions::default()
        };
        assert_eq!(options.effective_languages().unwrap(), "eng");
    }

    #[test]
    fn serializes_schema_v1_with_stable_grid_fields() {
        let matrix = TableMatrix::grid_with_type(
            vec![vec![crate::dto::TableCell::grid(
                0,
                0,
                LegacyBoundingBox::new(10.0, 20.0, 30.0, 40.0),
            )]],
            LegacyBoundingBox::new(10.0, 20.0, 30.0, 40.0),
            0.9,
            TableType::TitleBlock,
        );
        let document = DocumentResult::new(vec![PageResult::new(
            1,
            PageKind::Mixed,
            "Complete native page text".to_string(),
            vec![matrix],
        )]);
        let mut result = OcrResult::from_document(document, Vec::new());
        result.page_processing.push(PageProcessingInfo {
            page_number: 1,
            processing_mode: ProcessingMode::NativeTextWithGrid,
            native_text_quality: None,
            native_text_layout: None,
            ocr_confidence: Some(0.91),
            orientation: PageOrientation {
                angle: 90,
                confidence: 0.88,
            },
            language: "rus+eng".to_string(),
            render_size: Some(RenderSize {
                width: 1200,
                height: 800,
                dpi: 300,
            }),
            word_assignment: Some(PageWordAssignmentMetrics {
                recognized_words: 12,
                assigned_words: 9,
                words_outside_tables: 3,
                duplicate_assignments: 0,
            }),
            quality: None,
            timings: PageStageTimings::default(),
        });
        let dir = tempdir().expect("temporary directory");
        let path = dir.path().join("result-v1.json");
        export_ocr_result_json(&result, &path).expect("serialize v1 result");
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("read v1 JSON"))
                .expect("parse v1 JSON");

        assert_eq!(value["schema_version"], OCR_RESULT_SCHEMA_VERSION);
        assert_eq!(value["tables"][0]["grids"][0]["kind"], "title_block");
        assert_eq!(value["tables"][0]["grids"][0]["bbox"]["x"], 10);
        assert_eq!(value["tables"][0]["grids"][0]["cells"][0]["row_span"], 1);
        assert_eq!(value["tables"][0]["grids"][0]["cells"][0]["col_span"], 1);
        assert_eq!(
            value["page_processing"][0]["processing_mode"],
            "native_text+grid"
        );
        assert_eq!(value["page_processing"][0]["language"], "rus+eng");
        assert_eq!(value["page_processing"][0]["orientation"]["angle"], 90);
        assert_eq!(value["page_processing"][0]["render_size"]["dpi"], 300);
        assert_eq!(
            value["page_processing"][0]["word_assignment"]["recognized_words"],
            12
        );
        assert_eq!(
            value["page_processing"][0]["word_assignment"]["words_outside_tables"],
            3
        );
    }

    #[test]
    fn stable_cell_defaults_to_unit_spans() {
        let cell = TableCell::default();
        assert_eq!(cell.row_span, 1);
        assert_eq!(cell.col_span, 1);
        assert_eq!(cell.bbox, BoundingBox::default());
    }

    #[test]
    fn stable_grid_model_is_publicly_constructible() {
        let grid = DetectedGrid {
            kind: GridKind::DataTable,
            semantic_type: SemanticTableType::GenericTable,
            semantic_confidence: 0.75,
            header_rows: vec![0],
            column_roles: vec![ColumnRoleAssignment {
                column: 0,
                role: ColumnRole::Name,
                confidence: 0.9,
            }],
            classification_evidence: Vec::new(),
            bbox: BoundingBox {
                x: 0,
                y: 0,
                width: 30,
                height: 20,
            },
            rows: 1,
            columns: 1,
            cells: vec![TableCell::default()],
            confidence: 1.0,
        };
        assert_eq!(grid.kind, GridKind::DataTable);
        assert_eq!(grid.semantic_type, SemanticTableType::GenericTable);
    }

    fn ruled_matrix(rows: &[&[&str]], column_widths: &[f64]) -> TableMatrix {
        let left = 100.0;
        let top = 60.0;
        let row_height = 28.0;
        let mut matrix_rows = Vec::with_capacity(rows.len());
        for (row_index, values) in rows.iter().enumerate() {
            assert_eq!(values.len(), column_widths.len());
            let mut x = left;
            let mut row = Vec::with_capacity(values.len());
            for (column_index, (value, width)) in
                values.iter().zip(column_widths.iter()).enumerate()
            {
                let mut cell = crate::dto::TableCell::grid(
                    row_index as u32,
                    column_index as u32,
                    LegacyBoundingBox::new(
                        x,
                        top + row_index as f64 * row_height,
                        x + width,
                        top + (row_index + 1) as f64 * row_height,
                    ),
                );
                cell.text = (*value).to_string();
                cell.confidence = 0.92;
                row.push(cell);
                x += width;
            }
            matrix_rows.push(row);
        }
        TableMatrix::grid(
            matrix_rows,
            LegacyBoundingBox::new(
                left,
                top,
                left + column_widths.iter().sum::<f64>(),
                top + rows.len() as f64 * row_height,
            ),
            0.91,
        )
    }

    #[test]
    fn adjacent_compatible_grid_inherits_specific_semantics_without_replacing_content() {
        let widths = [55.0, 105.0, 260.0, 85.0, 80.0];
        let first = ruled_matrix(
            &[
                &["Поз.", "Код", "Наименование", "Ед. изм.", "Кол-во"],
                &["1", "AX-100", "Шкаф управления", "шт", "2"],
                &["2", "BX-200", "Датчик давления", "шт", "4"],
            ],
            &widths,
        );
        let continuation = ruled_matrix(
            &[
                &["3", "CX-300", "Позиция без повторной шапки", "шт", "1"],
                &["4", "DX-400", "Следующая позиция", "шт", "3"],
                &["5", "EX-500", "Последняя позиция", "шт", "2"],
            ],
            &widths,
        );
        let independent = DetectedGrid::from_matrix(&continuation, None);
        assert!(matches!(
            independent.semantic_type,
            SemanticTableType::GenericTable | SemanticTableType::Unknown
        ));
        let expected_rows = continuation.rows.clone();
        let result = OcrResult::from_document(
            DocumentResult::new(vec![
                PageResult::new(1, PageKind::Mixed, "native first".to_string(), vec![first]),
                PageResult::new(
                    2,
                    PageKind::Mixed,
                    "native continuation".to_string(),
                    vec![continuation],
                ),
            ]),
            Vec::new(),
        );

        let continuation_grid = &result.tables[1].grids[0];
        assert_eq!(
            continuation_grid.semantic_type,
            SemanticTableType::MaterialSpecification
        );
        assert!(continuation_grid
            .classification_evidence
            .iter()
            .any(|item| { item.rule == "semantic:adjacent_page_continuation_geometry" }));
        assert_eq!(result.document.pages[1].text, "native continuation");
        assert_eq!(result.document.pages[1].tables[0].rows, expected_rows);
    }

    #[test]
    fn continuation_semantics_reject_incompatible_or_non_adjacent_grids() {
        let first = ruled_matrix(
            &[
                &["Поз.", "Код", "Наименование", "Ед. изм.", "Кол-во"],
                &["1", "AX-100", "Шкаф управления", "шт", "2"],
                &["2", "BX-200", "Датчик давления", "шт", "4"],
            ],
            &[55.0, 105.0, 260.0, 85.0, 80.0],
        );
        let incompatible = ruled_matrix(
            &[
                &["A", "B", "C", "D"],
                &["E", "F", "G", "H"],
                &["I", "J", "K", "L"],
            ],
            &[80.0, 140.0, 170.0, 195.0],
        );
        let non_adjacent = ruled_matrix(
            &[
                &["3", "CX-300", "Позиция без шапки", "шт", "1"],
                &["4", "DX-400", "Следующая позиция", "шт", "3"],
                &["5", "EX-500", "Последняя позиция", "шт", "2"],
            ],
            &[55.0, 105.0, 260.0, 85.0, 80.0],
        );
        let result = OcrResult::from_document(
            DocumentResult::new(vec![
                PageResult::new(1, PageKind::Mixed, String::new(), vec![first]),
                PageResult::new(2, PageKind::Mixed, String::new(), vec![incompatible]),
                PageResult::new(4, PageKind::Mixed, String::new(), vec![non_adjacent]),
            ]),
            Vec::new(),
        );

        for page_index in [1, 2] {
            assert_ne!(
                result.tables[page_index].grids[0].semantic_type,
                SemanticTableType::MaterialSpecification
            );
            assert!(!result.tables[page_index].grids[0]
                .classification_evidence
                .iter()
                .any(|item| item.rule == "semantic:adjacent_page_continuation_geometry"));
        }
    }

    #[cfg(feature = "native")]
    #[test]
    fn render_time_input_errors_keep_their_stable_public_code() {
        let error = crate::input_format::InputSourceError::ImageDecodeFailed {
            path: std::path::PathBuf::from("broken.tiff"),
            detail: "truncated compressed strip".to_string(),
        };
        let mapped = ocr_error_from_pipeline(std::path::Path::new("broken.tiff"), Box::new(error));
        assert!(matches!(mapped, OcrError::ImageDecodeFailed { .. }));
        assert_eq!(mapped.code(), "image_decode_failed");
    }
}
