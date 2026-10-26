//! Stable, opt-in progress events for long-running local OCR jobs.
//!
//! The result JSON remains schema version 1. Progress is intentionally a
//! separate JSON Lines stream so embedding applications can observe a job
//! without parsing user text or waiting for the final output transaction.

use serde::Serialize;

use crate::page_analysis::ProcessingMode;

/// Version of the progress-event wire format.
pub const OCR_PROGRESS_SCHEMA_VERSION: u32 = 1;

/// A completed or terminal OCR pipeline stage.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OcrProgressStage {
    DocumentOpen,
    NativeText,
    Render,
    Ocr,
    TableDetection,
    Aggregation,
    Export,
    Completed,
    Failed,
}

/// One machine-readable progress record.
///
/// `elapsed_ms` is the elapsed time for the reported stage when available;
/// `aggregation` and terminal events use the elapsed page or document work
/// known at the point the event is emitted. No OCR text is included.
#[derive(Debug, Clone, Serialize)]
pub struct OcrProgressEvent {
    pub schema_version: u32,
    pub stage: OcrProgressStage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_number: Option<u32>,
    pub total_pages: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing_mode: Option<ProcessingMode>,
    pub elapsed_ms: f64,
    /// Stable error code for terminal failures. Absent for successful events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl OcrProgressEvent {
    pub fn document(stage: OcrProgressStage, total_pages: u32, elapsed_ms: f64) -> Self {
        Self {
            schema_version: OCR_PROGRESS_SCHEMA_VERSION,
            stage,
            page_number: None,
            total_pages,
            processing_mode: None,
            elapsed_ms,
            code: None,
        }
    }

    pub fn page(
        stage: OcrProgressStage,
        page_number: u32,
        total_pages: u32,
        processing_mode: ProcessingMode,
        elapsed_ms: f64,
    ) -> Self {
        Self {
            schema_version: OCR_PROGRESS_SCHEMA_VERSION,
            stage,
            page_number: Some(page_number),
            total_pages,
            processing_mode: Some(processing_mode),
            elapsed_ms,
            code: None,
        }
    }

    /// Records the start of a specific page stage before the processing mode is
    /// known. This is intended for live diagnostics of native calls.
    pub fn page_stage_started(stage: OcrProgressStage, page_number: u32, total_pages: u32) -> Self {
        Self {
            schema_version: OCR_PROGRESS_SCHEMA_VERSION,
            stage,
            page_number: Some(page_number),
            total_pages,
            processing_mode: None,
            elapsed_ms: 0.0,
            code: None,
        }
    }
    /// Records the start of a page before its processing mode is known. This
    /// makes a stalled native call observable without emitting user content.
    pub fn page_started(page_number: u32, total_pages: u32) -> Self {
        Self {
            schema_version: OCR_PROGRESS_SCHEMA_VERSION,
            stage: OcrProgressStage::NativeText,
            page_number: Some(page_number),
            total_pages,
            processing_mode: None,
            elapsed_ms: 0.0,
            code: None,
        }
    }

    pub fn failure(page_number: Option<u32>, total_pages: u32, code: impl Into<String>) -> Self {
        Self {
            schema_version: OCR_PROGRESS_SCHEMA_VERSION,
            stage: OcrProgressStage::Failed,
            page_number,
            total_pages,
            processing_mode: None,
            elapsed_ms: 0.0,
            code: Some(code.into()),
        }
    }
}

/// Receives progress without coupling the OCR Core to a transport, CLI or UI.
/// Implementations must return promptly; expensive or blocking work belongs in
/// the embedding application's own progress consumer.
pub trait OcrProgressReporter: Send + Sync {
    fn report(&self, event: &OcrProgressEvent);
}

impl<F> OcrProgressReporter for F
where
    F: Fn(&OcrProgressEvent) + Send + Sync,
{
    fn report(&self, event: &OcrProgressEvent) {
        self(event);
    }
}

#[cfg(test)]
mod tests {
    use super::{OcrProgressEvent, OcrProgressStage, OCR_PROGRESS_SCHEMA_VERSION};
    use crate::page_analysis::ProcessingMode;

    #[test]
    fn page_event_has_stable_json_fields() {
        let event = OcrProgressEvent::page(
            OcrProgressStage::Aggregation,
            2,
            35,
            ProcessingMode::OcrWithGrid,
            12.5,
        );
        let value = serde_json::to_value(event).expect("serialize progress event");
        assert_eq!(value["schema_version"], OCR_PROGRESS_SCHEMA_VERSION);
        assert_eq!(value["stage"], "aggregation");
        assert_eq!(value["page_number"], 2);
        assert_eq!(value["total_pages"], 35);
        assert_eq!(value["processing_mode"], "ocr+grid");
        assert_eq!(value["elapsed_ms"], 12.5);
    }
}
