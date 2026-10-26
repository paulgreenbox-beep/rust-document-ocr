#![cfg(feature = "native")]

mod common;

use ocr_pipeline::{process_region, NormalizedRect, RegionMode, RegionRequest};

#[test]
fn valid_blank_region_returns_an_empty_result() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let input = directory.path().join("blank-corner.pdf");
    common::write_text_pdf(&input, &[("Centered Fixture", 0)]);
    let result = process_region(
        input,
        RegionRequest {
            page_index: 0,
            rect: NormalizedRect::new(0.85, 0.85, 0.1, 0.1),
            mode: RegionMode::Text,
        },
        common::native_options(),
    )
    .expect("blank region is a valid selection");

    assert!(result.text.trim().is_empty());
    assert!(result.words.is_empty());
    assert!(result.tables.is_empty());
}
