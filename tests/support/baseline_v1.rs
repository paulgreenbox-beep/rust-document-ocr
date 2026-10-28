// Frozen verbatim OcrWarning/OcrResult from baseline 6699a10dbfeff2471471b5eb2688e4d5394fe671.
// Supporting DTOs are unchanged at this boundary; do not replace these definitions
// with aliases to the candidate OcrResult/OcrWarning.
#![allow(dead_code)]
use ocr_pipeline::api::{DocumentModel, PageTables};
use serde::{Deserialize, Serialize};
/// Non-fatal conditions returned to an embedding application.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OcrWarning {
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
