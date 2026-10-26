#![cfg(feature = "native")]

mod common;

#[test]
fn normalized_roi_tracks_visible_page_at_rotate_zero() {
    assert_rotated_region(0);
}

fn assert_rotated_region(angle: u16) {
    use ocr_pipeline::{process_region, NormalizedRect, RegionMode, RegionRequest};
    let directory = tempfile::tempdir().expect("temporary directory");
    let input = directory.path().join("rotate.pdf");
    common::write_text_pdf(&input, &[("Rotation Fixture", angle)]);
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
    assert_eq!(result.orientation.angle, angle);
    assert!(result.text.contains("Rotation"));
    assert!(result.words.iter().all(|word| {
        word.bbox.x >= result.effective_rect.x
            && word.bbox.y >= result.effective_rect.y
            && word.bbox.x + word.bbox.width
                <= result.effective_rect.x + result.effective_rect.width
            && word.bbox.y + word.bbox.height
                <= result.effective_rect.y + result.effective_rect.height
    }));
}
