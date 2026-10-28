#[path = "support/baseline_v1.rs"]
mod baseline;
mod page_analysis {
    pub use ocr_pipeline::page_analysis::PageProcessingInfo;
}

use ocr_pipeline::api::{OcrDiagnostic, OcrResult, OcrWarning};
use ocr_pipeline::dto::{DocumentResult, PageKind, PageResult};

fn success() -> OcrResult {
    OcrResult::from_document(
        DocumentResult::new(vec![PageResult::new(
            1,
            PageKind::Mixed,
            "synthetic page".into(),
            vec![],
        )]),
        vec![OcrWarning::OverlaysUnsupported],
    )
}

fn failure() -> OcrResult {
    let mut result = success();
    result
        .diagnostics
        .push(OcrDiagnostic::CellOcrFallbackFailed {
            page_number: 1,
            table_index: 0,
            row: 1,
            column: 2,
            reason_code: "recognition_failed".into(),
        });
    result
}

#[test]
fn old_v1_client_reads_new_success_result() {
    let result = success();
    let value = serde_json::to_value(&result).unwrap();
    assert!(value.get("diagnostics").is_none());
    let old: baseline::OcrResult = serde_json::from_value(value).unwrap();
    assert_eq!(old.schema_version, 1);
    assert_eq!(old.document.pages[0].text, "synthetic page");
    assert_eq!(
        old.warnings,
        vec![baseline::OcrWarning::OverlaysUnsupported]
    );
}

#[test]
fn old_v1_client_handles_cell_failure_result() {
    let result = failure();
    let json = serde_json::to_string(&result).unwrap();
    let old = serde_json::from_str::<baseline::OcrResult>(&json)
        .expect("baseline v1 reader must accept candidate");
    assert_eq!(old.document.pages[0].text, result.document.pages[0].text);
    assert_eq!(old.warnings.len(), 1);
}

#[test]
fn new_client_preserves_cell_failure_diagnostics() {
    let result = failure();
    let roundtrip: OcrResult =
        serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
    assert_eq!(roundtrip.diagnostics, result.diagnostics);
    assert_eq!(roundtrip.warnings, result.warnings);
    assert_eq!(roundtrip.schema_version, 1);
}

#[test]
fn legacy_json_contract_unchanged() {
    let result = success();
    let legacy = serde_json::to_value(&result.document).unwrap();
    assert!(legacy.get("schema_version").is_none());
    assert!(legacy.get("diagnostics").is_none());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("result.json");
    ocr_pipeline::api::export_legacy_document_json(&result, &path).unwrap();
    let exported: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(exported, legacy);

    // The frozen old envelope is also still readable by the new client.
    let old: baseline::OcrResult =
        serde_json::from_value(serde_json::to_value(&result).unwrap()).unwrap();
    let old_json = serde_json::to_value(old).unwrap();
    let new: OcrResult = serde_json::from_value(old_json.clone()).unwrap();
    assert!(new.diagnostics.is_empty());
    assert_eq!(serde_json::to_value(new).unwrap(), old_json);
}

#[test]
fn json_v1_unknown_additive_fields_are_safe() {
    let mut value = serde_json::to_value(failure()).unwrap();
    value["future_additive_field"] = serde_json::json!({"synthetic": true});
    let old: baseline::OcrResult = serde_json::from_value(value.clone()).unwrap();
    let new: OcrResult = serde_json::from_value(value).unwrap();
    assert_eq!(old.schema_version, new.schema_version);
    assert_eq!(new.diagnostics.len(), 1);
}

#[test]
fn warning_does_not_expose_private_ocr_text() {
    let diagnostic = serde_json::to_value(&failure().diagnostics[0]).unwrap();
    assert_eq!(
        diagnostic,
        serde_json::json!({
            "kind": "cell_ocr_fallback_failed", "page_number": 1,
            "table_index": 0, "row": 1, "column": 2, "reason_code": "recognition_failed"
        })
    );
}

#[test]
fn old_v1_enum_rejects_pre_fix_cell_warning() {
    // Retain the exact pre-fix wire shape as an executable regression witness.
    let mut value = serde_json::to_value(success()).unwrap();
    value["warnings"] = serde_json::json!([{
        "kind": "cell_ocr_fallback_failed", "page_number": 1,
        "table_index": 0, "row": 1, "column": 2, "reason_code": "recognition_failed"
    }]);
    let error = serde_json::from_value::<baseline::OcrResult>(value).unwrap_err();
    assert!(error
        .to_string()
        .starts_with("unknown variant `cell_ocr_fallback_failed`"));
}

#[test]
fn baseline_exhaustive_warning_match_still_compiles() {
    // No wildcard: adding any variant to the v1 enum breaks this consumer.
    fn exhaustive(warning: OcrWarning) -> &'static str {
        match warning {
            OcrWarning::GridDetectorFailed { .. } => "grid_detector_failed",
            OcrWarning::NativeTextUnavailable { .. } => "native_text_unavailable",
            OcrWarning::NativeTextLayoutUnavailable { .. } => "native_text_layout_unavailable",
            OcrWarning::NativeWordGeometryUnavailable { .. } => "native_word_geometry_unavailable",
            OcrWarning::TableRoutingPreflightFailed { .. } => "table_routing_preflight_failed",
            OcrWarning::OrientationDetectionFailed { .. } => "orientation_detection_failed",
            OcrWarning::AdditionalImageFramesIgnored { .. } => "additional_image_frames_ignored",
            OcrWarning::OverlaysUnsupported => "overlays_unsupported",
        }
    }
    assert_eq!(
        exhaustive(OcrWarning::OverlaysUnsupported),
        "overlays_unsupported"
    );
}
