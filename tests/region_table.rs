#![cfg(feature = "native")]

mod common;

use ocr_pipeline::{process_region, NormalizedRect, ProcessingMode, RegionMode, RegionRequest};

#[test]
fn searchable_table_region_preserves_native_text_and_returns_grid() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let input = directory.path().join("native-grid-region.pdf");
    common::write_grid_text_pdf(&input);

    let result = process_region(
        &input,
        RegionRequest {
            page_index: 0,
            rect: NormalizedRect::new(0.08, 0.08, 0.84, 0.84),
            mode: RegionMode::Table,
        },
        common::native_options(),
    )
    .expect("native table region");

    assert_eq!(result.processing_mode, ProcessingMode::NativeTextWithGrid);
    assert!(result.text.contains("Alpha"));
    assert!(result.text.contains("Delta"));
    assert!(!result.tables.is_empty());
    assert!(result.metrics.assigned_words > 0);
    assert_eq!(result.metrics.duplicate_assignments, 0);
    assert!(result.metrics.filled_cells > 0);
    assert!(result.text.len() >= result.text_outside_tables.len());
}
