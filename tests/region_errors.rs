#![cfg(feature = "native")]

mod common;

use ocr_pipeline::{process_region, NormalizedRect, OcrError, RegionMode, RegionRequest};

#[test]
fn region_validation_and_page_range_return_stable_error_codes() {
    let missing = std::path::Path::new("does-not-need-to-exist-for-coordinate-validation.pdf");
    let invalid = process_region(
        missing,
        RegionRequest {
            page_index: 0,
            rect: NormalizedRect::new(f32::NAN, 0.0, 0.5, 0.5),
            mode: RegionMode::Auto,
        },
        common::native_options(),
    )
    .expect_err("NaN must fail before PDF open");
    assert_eq!(invalid.code(), "invalid_region");

    let empty = process_region(
        missing,
        RegionRequest {
            page_index: 0,
            rect: NormalizedRect::new(0.0, 0.0, 0.0, 0.5),
            mode: RegionMode::Auto,
        },
        common::native_options(),
    )
    .expect_err("zero-width ROI must fail before PDF open");
    assert!(matches!(empty, OcrError::EmptyRegion));

    let directory = tempfile::tempdir().expect("temporary directory");
    let input = directory.path().join("one-page.pdf");
    common::write_text_pdf(&input, &[("One Page", 0)]);
    let page_error = process_region(
        &input,
        RegionRequest {
            page_index: 1,
            rect: NormalizedRect::new(0.0, 0.0, 1.0, 1.0),
            mode: RegionMode::Text,
        },
        common::native_options(),
    )
    .expect_err("second page is out of range");
    assert_eq!(page_error.code(), "invalid_page_index");
}
