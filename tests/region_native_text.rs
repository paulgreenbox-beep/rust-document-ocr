#![cfg(feature = "native")]

mod common;

use ocr_pipeline::{process_region, NormalizedRect, ProcessingMode, RegionMode, RegionRequest};

#[test]
fn searchable_text_region_uses_native_text_without_rendering() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let input = directory.path().join("native-region.pdf");
    common::write_text_pdf(&input, &[("Native Region Fixture 12345", 0)]);

    let result = process_region(
        &input,
        RegionRequest {
            page_index: 0,
            rect: NormalizedRect::new(0.2, 0.2, 0.6, 0.6),
            mode: RegionMode::Text,
        },
        common::native_options(),
    )
    .expect("native region");

    assert_eq!(result.processing_mode, ProcessingMode::NativeText);
    assert!(result.text.contains("Native"));
    assert!(!result.words.is_empty());
    assert!(result.render_dpi.is_none());
    assert_eq!(result.timings.tesseract_ms, 0.0);
    assert!(result.tables.is_empty());
}
