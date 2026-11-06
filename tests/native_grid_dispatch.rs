#![cfg(feature = "native")]

mod common;

use ocr_pipeline::{
    native::input_source::NativePageSource, BoundingBox, PageSource, PdfiumPageSource,
};

#[test]
fn production_source_wrapper_forwards_grid_boundaries_before_word_union() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("native-grid-dispatch.pdf");
    common::write_text_pdf(&input, &[("AB", 0)]);
    let pdf = PdfiumPageSource::open_with_pdfium_dll(
        &input,
        100.0,
        Some(&common::versioned_pdfium_dll()),
    )
    .unwrap();
    let original = pdf.extract_native_text(1).unwrap();
    let old = pdf
        .extract_native_words_for_rendered_page(1, 800, 600)
        .unwrap();
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].text, "AB");
    let middle = (old[0].bbox.x0 + old[0].bbox.x1) / 2.0;
    let cells = [
        BoundingBox::new(0.0, 0.0, middle, 600.0),
        BoundingBox::new(middle, 0.0, 800.0, 600.0),
    ];
    let source = NativePageSource::Pdf(pdf);
    let words = source
        .native_words_for_grid_cells(1, 800, 600, &cells)
        .unwrap()
        .unwrap();
    assert_eq!(
        words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
        ["A", "B"]
    );
    assert!(words[0].bbox.x1 <= middle);
    assert!(words[1].bbox.x0 >= middle);
    assert_eq!(
        source.native_text_for_page(1).unwrap().unwrap().text,
        original
    );
}
