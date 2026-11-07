//! Both experimental feature configurations must keep signature dispatch and
//! the existing one-primary-page contract; no OCR/model runtime is needed.
#![cfg(feature = "native")]

use std::io::Write;

use ocr_pipeline::{
    input_format::{InputFormat, InputLimits},
    native::input_source::{NativePageSource, OpenedPageSource},
    page_source::PageSource,
};

#[test]
fn heif_signature_uses_the_explicitly_selected_decoder_without_suffix() {
    let bytes = include_bytes!("../tools/heic-rs-evaluation/fixtures/ocr-public/form-fixture.heic");
    let mut input = tempfile::NamedTempFile::new().unwrap();
    input.write_all(bytes).unwrap();
    input.flush().unwrap();
    let opened = OpenedPageSource::open(input.path(), 300.0, None, InputLimits::default())
        .expect("synthetic HEIC signature dispatch");
    assert_eq!(opened.format, InputFormat::Heif);
    assert!(opened.warnings.is_empty());
    let NativePageSource::Heif(mut source) = opened.source else {
        panic!("HEIF signature must not select PDF/raster/DjVu");
    };
    let actual_type = std::any::type_name_of_val(&source);
    if cfg!(feature = "heic-rs-input") {
        assert!(actual_type.ends_with("heic_rs_source::HeicRsPageSource"));
    } else {
        assert!(actual_type.ends_with("heif_source::HeifPageSource"));
    }
    assert_eq!(source.total_pages().unwrap(), 1);
    let page = source.render_page(1).unwrap();
    assert_eq!((page.image.width(), page.image.height()), (1240, 1754));
    assert!(page.image.as_raw().iter().any(|&pixel| pixel < 128));
    assert!(source.render_page(2).is_err());
}
