#![cfg(feature = "native")]

mod common;

use ocr_pipeline::{process_region, NormalizedRect, RegionMode, RegionRequest};

#[test]
fn region_processing_does_not_return_neighbor_page_text() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let input = directory.path().join("two-pages.pdf");
    common::write_text_pdf(
        &input,
        &[("Selected Page Alpha", 0), ("Neighbor Page Omega", 0)],
    );

    let result = process_region(
        &input,
        RegionRequest {
            page_index: 0,
            rect: NormalizedRect::new(0.0, 0.0, 1.0, 1.0),
            mode: RegionMode::Text,
        },
        common::native_options(),
    )
    .expect("selected page");

    assert!(result.text.contains("Selected"));
    assert!(!result.text.contains("Neighbor"));
    assert_eq!(result.page_index, 0);
}
