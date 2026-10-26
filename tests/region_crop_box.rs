#![cfg(feature = "native")]

mod common;

use ocr_pipeline::{process_region, NormalizedRect, RegionMode, RegionRequest};

#[test]
fn normalized_roi_is_relative_to_visible_crop_box() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let input = directory.path().join("crop-box.pdf");
    common::write_cropped_text_pdf(&input);
    let result = process_region(
        input,
        RegionRequest {
            page_index: 0,
            rect: NormalizedRect::new(0.2, 0.2, 0.6, 0.6),
            mode: RegionMode::Text,
        },
        common::native_options(),
    )
    .expect("crop-box region");

    assert!(result.text.contains("Crop"));
    assert!(result.words.iter().all(|word| {
        word.bbox.x >= result.effective_rect.x
            && word.bbox.y >= result.effective_rect.y
            && word.bbox.x + word.bbox.width
                <= result.effective_rect.x + result.effective_rect.width
            && word.bbox.y + word.bbox.height
                <= result.effective_rect.y + result.effective_rect.height
    }));
}
