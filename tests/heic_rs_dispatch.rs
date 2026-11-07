//! Both experimental feature configurations must keep signature dispatch and
//! the existing one-primary-page contract; no OCR/model runtime is needed.
#![cfg(feature = "native-common")]

use std::io::Write;

use ocr_pipeline::{
    input_format::{InputFormat, InputLimits},
    native::input_source::OpenedPageSource,
    page_source::PageSource,
};

#[cfg(any(feature = "heif-input", feature = "heic-rs-input"))]
use ocr_pipeline::native::input_source::NativePageSource;

#[cfg(any(feature = "heif-input", feature = "heic-rs-input"))]
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

#[test]
fn shared_native_stack_dispatches_png_without_a_heif_decoder() {
    let mut input = tempfile::NamedTempFile::new().unwrap();
    image::DynamicImage::ImageLuma8(image::GrayImage::from_pixel(3, 2, image::Luma([200])))
        .write_to(&mut input, image::ImageFormat::Png)
        .unwrap();
    input.flush().unwrap();
    let mut opened = OpenedPageSource::open(input.path(), 300.0, None, InputLimits::default())
        .expect("shared PNG source should not need a HEIF decoder");
    assert_eq!(opened.format, InputFormat::Png);
    assert!(opened.warnings.is_empty());
    assert_eq!(opened.source.total_pages().unwrap(), 1);
    assert_eq!(
        opened.source.render_page(1).unwrap().image.dimensions(),
        (3, 2)
    );
}

#[cfg(not(any(feature = "heif-input", feature = "heic-rs-input")))]
#[test]
fn common_only_heif_signature_is_unsupported_without_loading_a_decoder() {
    use ocr_pipeline::{input_format::InputSourceError, native::input_source::NativeInputError};

    let mut input = tempfile::NamedTempFile::new().unwrap();
    input
        .write_all(include_bytes!(
            "../tools/heic-rs-evaluation/fixtures/flat-64.heic"
        ))
        .unwrap();
    input.flush().unwrap();
    let error = match OpenedPageSource::open(input.path(), 300.0, None, InputLimits::default()) {
        Ok(_) => panic!("native-common alone must not expose a HEIF decoder"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "unsupported_input_format");
    assert!(matches!(
        error,
        NativeInputError::Source(InputSourceError::UnsupportedInputFormat { path })
            if path == input.path()
    ));
}

#[cfg(all(feature = "heif-input", feature = "heic-rs-input"))]
#[test]
fn mixed_features_prioritize_heic_rs_and_preserve_its_rejection_without_fallback() {
    use ocr_pipeline::{input_format::InputSourceError, native::input_source::NativeInputError};

    let mut bytes =
        include_bytes!("../tools/heic-rs-evaluation/fixtures/extended/flat-alpha128-64.heic")
            .to_vec();
    let link = bytes.windows(4).position(|kind| kind == b"auxl").unwrap();
    bytes[link..link + 4].copy_from_slice(b"prem");
    let mut input = tempfile::NamedTempFile::new().unwrap();
    input.write_all(&bytes).unwrap();
    input.flush().unwrap();
    let error = match OpenedPageSource::open(input.path(), 300.0, None, InputLimits::default()) {
        Ok(_) => panic!("the selected heic-rs decoder must reject unsupported premultiplied alpha"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "heif_decode_failed");
    assert!(matches!(
        error,
        NativeInputError::Source(InputSourceError::HeifDecodeFailed { detail, .. })
            if detail.contains("unsupported_premultiplied_alpha")
    ));
}

#[cfg(feature = "heif-input")]
#[test]
fn legacy_heif_type_paths_reexport_the_shared_contract() {
    use ocr_pipeline::native::{heif_source, heif_types};

    let limits: heif_types::HeifSourceLimits = heif_source::HeifSourceLimits::default();
    assert_eq!(limits, heif_types::HeifSourceLimits::default());
    assert_eq!(heif_source::DEFAULT_HEIF_DPI, heif_types::DEFAULT_HEIF_DPI);
    assert_eq!(
        heif_source::DEFAULT_HEIF_MAX_FILE_BYTES,
        heif_types::DEFAULT_HEIF_MAX_FILE_BYTES
    );
    assert_eq!(
        heif_source::DEFAULT_HEIF_MAX_PIXELS,
        heif_types::DEFAULT_HEIF_MAX_PIXELS
    );
    assert_eq!(
        heif_source::DEFAULT_HEIF_MAX_FRAMES,
        heif_types::DEFAULT_HEIF_MAX_FRAMES
    );
    let warning: heif_types::HeifSourceWarning =
        heif_source::HeifSourceWarning::AdditionalTopLevelImagesIgnored {
            total_images: 2,
            ignored_images: 1,
        };
    assert!(matches!(
        warning,
        heif_types::HeifSourceWarning::AdditionalTopLevelImagesIgnored {
            total_images: 2,
            ignored_images: 1,
        }
    ));
    let error: heif_types::HeifSourceError =
        heif_source::HeifSourceError::InvalidPageIndex { page_num: 2 };
    assert_eq!(error.code(), "invalid_page_index");
}
