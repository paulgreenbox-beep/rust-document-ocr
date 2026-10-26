//! Library crate for the OCR pipeline.
//!
//! All adapters and the pipeline live in submodules. The default build
//! compiles without any native PDF/OCR library. Real adapters are
//! feature-gated behind `native`.

pub mod api;
pub mod borderless_table;
pub mod config;
pub mod detector;
pub mod dto;
pub mod export_docx;
pub mod export_json;
pub mod export_xlsx;
#[cfg(feature = "native")]
pub mod input_format;
pub mod language;
pub mod ocr;
pub mod orientation;
pub mod page_analysis;
pub mod page_source;
pub mod pipeline;
pub mod preprocessing;
pub mod progress;
pub mod quality;
pub mod reconstruction;
pub mod region;
pub mod table_grid;
pub mod table_semantics;

#[cfg(feature = "native")]
pub mod native;

#[cfg(feature = "native")]
pub use native::pdfium_source::PdfiumPageSource;
#[cfg(feature = "native")]
pub use native::raster_source::RasterImagePageSource;
#[cfg(feature = "native")]
pub use native::tesseract_engine::TesseractOcrEngine;
#[cfg(feature = "native")]
pub use native::tiff_source::TiffPageSource;

pub use api::{
    export_legacy_document_json, export_ocr_result_docx, export_ocr_result_json,
    export_ocr_result_xlsx, process_document, process_document_with_progress,
    ClassificationEvidence, ColumnRole, ColumnRoleAssignment, DetectedGrid, DocumentModel,
    EvidenceKind, GridKind, OcrError, OcrOptions, OcrResult, OcrWarning, PageTables,
    SemanticTableType, OCR_RESULT_SCHEMA_VERSION,
};
pub use config::Config;
pub use dto::{
    BoundingBox, DocumentResult, PageKind, PageResult, TableCell, TableDetectorType, TableMatrix,
    TableType, Word,
};
pub use export_docx::{export_docx, export_docx_with_options, DocxExportOptions};
pub use export_json::export_json;
pub use export_xlsx::{export_xlsx, export_xlsx_with_options, XlsxExportOptions};
pub use language::LanguageRouting;
pub use ocr::{
    OcrEngine, OcrLanguageProfile, OcrRequest, OcrSegmentation, OcrTextResult, StubOcrEngine,
};
pub use page_analysis::{
    analyze_native_text, NativeTextLayer, NativeTextLayout, NativeTextQuality, PageOrientation,
    PageProcessingInfo, PageStageTimings, PageWordAssignmentMetrics, ProcessingMode, RenderSize,
};
pub use page_source::{PageSource, RenderedPage, StubPageSource};
pub use pipeline::{run as run_pipeline, Pipeline, PipelineOutcome, PipelineWarning, TableMode};
pub use preprocessing::{preprocess, DEFAULT_MAX_PIXELS};
pub use progress::{
    OcrProgressEvent, OcrProgressReporter, OcrProgressStage, OCR_PROGRESS_SCHEMA_VERSION,
};
pub use quality::{
    assess_page_quality, PageQualityIssue, PageQualityReport, PageQualitySignals,
    PageQualityStatus, PAGE_QUALITY_PROFILE, PAGE_QUALITY_PROFILE_VERSION,
};
pub use region::{
    export_region_result_json, process_region, NormalizedRect, RegionCoordinateSpace,
    RegionMetrics, RegionMode, RegionRequest, RegionResult, RegionTable, RegionTableCell,
    RegionWarning, RegionWord, REGION_RESULT_SCHEMA_VERSION,
};
