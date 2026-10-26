#![cfg(feature = "native")]

mod common;

#[test]
fn normalized_roi_tracks_visible_page_at_rotate_ninety() {
    use ocr_pipeline::{process_region, NormalizedRect, RegionMode, RegionRequest};
    let directory = tempfile::tempdir().expect("temporary directory");
    let input = directory.path().join("rotate-90.pdf");
    common::write_text_pdf(&input, &[("Rotation Fixture", 90)]);
    let result = process_region(
        input,
        RegionRequest {
            page_index: 0,
            rect: NormalizedRect::new(0.2, 0.2, 0.6, 0.6),
            mode: RegionMode::Text,
        },
        common::native_options(),
    )
    .expect("rotated native region");
    assert_eq!(result.orientation.angle, 90);
    assert!(result.text.contains("Rotation"));
}
