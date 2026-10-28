//! OCR engine abstraction.

use image::GrayImage;

use crate::dto::Word;

/// Plain-text OCR output plus an optional confidence estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrTextResult {
    pub text: String,
    pub confidence: Option<f32>,
}

/// Page-layout profile supplied to OCR engines without changing the stable
/// document model or exported JSON contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrSegmentation {
    /// Automatic page segmentation for complete pages.
    Automatic,
    /// Sparse text used only by orientation preflight.
    Sparse,
    /// A single text line, used for bounded table-cell fallback.
    SingleLine,
    /// One numeric/code-like cell with a digits-and-punctuation whitelist.
    SingleWordNumeric,
    /// One numeric line with the same whitelist; retained alongside the
    /// single-word profile because punctuation segmentation differs.
    SingleLineNumeric,
}

/// Optional per-request language selection. The configured document language
/// remains authoritative; narrowly-scoped table-cell fallbacks may use an
/// already-configured English model without changing page OCR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OcrLanguageProfile {
    #[default]
    Configured,
    /// Use only `eng` when it is already present in the configured language
    /// list. Engines without request-level language support safely ignore it.
    EnglishOnly,
    /// Strict English cell recognition within an explicitly enabled Chinese
    /// route, even when the global configured route has no English component.
    /// Unlike `EnglishOnly`, absence/unsupported capability is an error.
    EnglishOnlyRequired,
    /// Use only `rus` when it is already present in the configured language
    /// list. This remains scoped to one bounded table cell.
    RussianOnly,
    /// Simplified Chinese plus English, for an explicitly selected or reliably
    /// detected Chinese page. Requires both models; never silently falls back.
    SimplifiedChinese,
}

/// Per-recognition facts that native engines need for deterministic quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OcrRequest {
    pub dpi: u32,
    pub segmentation: OcrSegmentation,
    pub adaptive_learning: bool,
    pub preserve_interword_spaces: bool,
    pub language_profile: OcrLanguageProfile,
}

impl OcrRequest {
    pub fn page(dpi: u32) -> Self {
        Self {
            dpi,
            segmentation: OcrSegmentation::Automatic,
            adaptive_learning: true,
            preserve_interword_spaces: true,
            language_profile: OcrLanguageProfile::Configured,
        }
    }

    pub fn orientation(dpi: u32) -> Self {
        Self {
            dpi,
            segmentation: OcrSegmentation::Sparse,
            adaptive_learning: false,
            preserve_interword_spaces: false,
            language_profile: OcrLanguageProfile::Configured,
        }
    }

    pub fn single_line(dpi: u32) -> Self {
        Self::single_line_with_language(dpi, OcrLanguageProfile::Configured)
    }

    pub fn single_line_with_language(dpi: u32, language_profile: OcrLanguageProfile) -> Self {
        Self {
            dpi,
            segmentation: OcrSegmentation::SingleLine,
            // A table page may contain hundreds of unrelated short cells.
            // Keeping Tesseract's adaptive classifier active lets an early
            // noisy/grid-only crop bias later identifiers and manufacturer
            // names. Cell OCR must be deterministic and page OCR retains the
            // established adaptive behaviour through `page()`.
            adaptive_learning: false,
            preserve_interword_spaces: false,
            language_profile,
        }
    }

    pub fn numeric_cell(dpi: u32) -> Self {
        Self {
            dpi,
            segmentation: OcrSegmentation::SingleWordNumeric,
            adaptive_learning: false,
            preserve_interword_spaces: false,
            language_profile: OcrLanguageProfile::Configured,
        }
    }

    pub fn numeric_line(dpi: u32) -> Self {
        Self {
            dpi,
            segmentation: OcrSegmentation::SingleLineNumeric,
            adaptive_learning: false,
            preserve_interword_spaces: false,
            language_profile: OcrLanguageProfile::Configured,
        }
    }
}

impl Default for OcrRequest {
    fn default() -> Self {
        Self::page(300)
    }
}

/// Two-tier OCR interface:
/// - `recognize_text` returns just the textual content of a page;
/// - `recognize_words` returns the per-word geometry needed to
///   reconstruct tables.
pub trait OcrEngine {
    /// Classifies only explicitly recoverable recognition/output failures for
    /// one optional cell request. Unknown, initialization and system errors
    /// remain fatal. Returned codes must be fixed identifiers, never OCR text.
    fn recoverable_cell_error_reason(
        &self,
        _error: &(dyn std::error::Error + 'static),
    ) -> Option<&'static str> {
        None
    }

    /// Effective page profile, used to keep language-sensitive table repair
    /// from treating Chinese text as noise in a Latin technical identifier.
    fn active_language_profile(&self) -> OcrLanguageProfile {
        OcrLanguageProfile::Configured
    }

    /// Checks an opt-in language capability without running recognition.
    /// Legacy/custom engines reject Chinese until they implement that route.
    fn validate_language_profile(
        &self,
        profile: OcrLanguageProfile,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if matches!(
            profile,
            OcrLanguageProfile::SimplifiedChinese | OcrLanguageProfile::EnglishOnlyRequired
        ) {
            return Err(
                "this OCR engine does not support the requested explicit language route".into(),
            );
        }
        Ok(())
    }

    /// Marks the beginning of a logical document page. Engines retaining
    /// mutable native state may release per-page adaptive state here. The
    /// default preserves compatibility for stateless/custom engines.
    fn begin_page(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn recognize_text(&self, image: &GrayImage) -> Result<String, Box<dyn std::error::Error>>;

    /// Returns text and, when the engine can provide it, a page-level
    /// confidence estimate. Existing engines remain source-compatible through
    /// this default implementation.
    fn recognize_text_with_confidence(
        &self,
        image: &GrayImage,
    ) -> Result<OcrTextResult, Box<dyn std::error::Error>> {
        Ok(OcrTextResult {
            text: self.recognize_text(image)?,
            confidence: None,
        })
    }

    fn recognize_words(&self, image: &GrayImage) -> Result<Vec<Word>, Box<dyn std::error::Error>>;

    /// Recognizes text with an explicit layout/DPI profile. The compatible
    /// default delegates to the original trait method for custom engines.
    fn recognize_text_with_request(
        &self,
        image: &GrayImage,
        request: OcrRequest,
    ) -> Result<OcrTextResult, Box<dyn std::error::Error>> {
        self.validate_language_profile(request.language_profile)?;
        self.recognize_text_with_confidence(image)
    }

    /// Recognizes positioned words with an explicit layout/DPI profile.
    fn recognize_words_with_request(
        &self,
        image: &GrayImage,
        request: OcrRequest,
    ) -> Result<Vec<Word>, Box<dyn std::error::Error>> {
        self.validate_language_profile(request.language_profile)?;
        self.recognize_words(image)
    }

    /// Uses an optional, separately versioned technical-cell model.
    ///
    /// The default deliberately returns `None`: repeating the configured
    /// page model would not be independent evidence and must not authorize a
    /// replacement of existing OCR text. Native engines may override this
    /// capability when an isolated technical traineddata artifact is present.
    fn recognize_technical_text_with_request(
        &self,
        image: &GrayImage,
        request: OcrRequest,
    ) -> Result<Option<OcrTextResult>, Box<dyn std::error::Error>> {
        let _ = (image, request);
        Ok(None)
    }

    /// Returns page text and words using one explicit native request.
    fn recognize_text_and_words_with_request(
        &self,
        image: &GrayImage,
        request: OcrRequest,
    ) -> Result<(OcrTextResult, Vec<Word>), Box<dyn std::error::Error>> {
        Ok((
            self.recognize_text_with_request(image, request)?,
            self.recognize_words_with_request(image, request)?,
        ))
    }

    /// Returns page text, confidence, and per-word geometry from one logical
    /// recognition request. Engines that can expose both outputs from a
    /// single native recognition pass should override this method. The
    /// default preserves compatibility with existing implementations.
    fn recognize_text_and_words(
        &self,
        image: &GrayImage,
    ) -> Result<(OcrTextResult, Vec<Word>), Box<dyn std::error::Error>> {
        Ok((
            self.recognize_text_with_confidence(image)?,
            self.recognize_words(image)?,
        ))
    }
}

/// A page-scoped language adapter. Its selected language applies to full-page
/// OCR, orientation and configured cell requests. Explicit Latin/Russian cell
/// requests and the independent technical model retain their own languages.
pub struct LanguageRoutedOcrEngine<'a, E: OcrEngine + ?Sized> {
    inner: &'a E,
    profile: OcrLanguageProfile,
}

impl<'a, E: OcrEngine + ?Sized> LanguageRoutedOcrEngine<'a, E> {
    pub fn new(
        inner: &'a E,
        profile: OcrLanguageProfile,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        inner.validate_language_profile(profile)?;
        Ok(Self { inner, profile })
    }

    fn route(&self, mut request: OcrRequest) -> OcrRequest {
        if request.language_profile == OcrLanguageProfile::Configured {
            request.language_profile = self.profile;
        } else if request.language_profile == OcrLanguageProfile::EnglishOnly
            && self.active_language_profile() == OcrLanguageProfile::SimplifiedChinese
        {
            request.language_profile = OcrLanguageProfile::EnglishOnlyRequired;
        }
        request
    }
}

impl<E: OcrEngine + ?Sized> OcrEngine for LanguageRoutedOcrEngine<'_, E> {
    fn recoverable_cell_error_reason(
        &self,
        error: &(dyn std::error::Error + 'static),
    ) -> Option<&'static str> {
        self.inner.recoverable_cell_error_reason(error)
    }

    fn active_language_profile(&self) -> OcrLanguageProfile {
        if self.profile == OcrLanguageProfile::Configured {
            self.inner.active_language_profile()
        } else {
            self.profile
        }
    }

    fn validate_language_profile(
        &self,
        profile: OcrLanguageProfile,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner
            .validate_language_profile(if profile == OcrLanguageProfile::Configured {
                self.profile
            } else if profile == OcrLanguageProfile::EnglishOnly
                && self.active_language_profile() == OcrLanguageProfile::SimplifiedChinese
            {
                OcrLanguageProfile::EnglishOnlyRequired
            } else {
                profile
            })
    }

    fn begin_page(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.begin_page()
    }

    fn recognize_text(&self, image: &GrayImage) -> Result<String, Box<dyn std::error::Error>> {
        Ok(self
            .recognize_text_with_request(image, OcrRequest::default())?
            .text)
    }

    fn recognize_text_with_confidence(
        &self,
        image: &GrayImage,
    ) -> Result<OcrTextResult, Box<dyn std::error::Error>> {
        self.recognize_text_with_request(image, OcrRequest::default())
    }

    fn recognize_words(&self, image: &GrayImage) -> Result<Vec<Word>, Box<dyn std::error::Error>> {
        self.recognize_words_with_request(image, OcrRequest::default())
    }

    fn recognize_text_with_request(
        &self,
        image: &GrayImage,
        request: OcrRequest,
    ) -> Result<OcrTextResult, Box<dyn std::error::Error>> {
        self.inner
            .recognize_text_with_request(image, self.route(request))
    }

    fn recognize_words_with_request(
        &self,
        image: &GrayImage,
        request: OcrRequest,
    ) -> Result<Vec<Word>, Box<dyn std::error::Error>> {
        self.inner
            .recognize_words_with_request(image, self.route(request))
    }

    fn recognize_text_and_words_with_request(
        &self,
        image: &GrayImage,
        request: OcrRequest,
    ) -> Result<(OcrTextResult, Vec<Word>), Box<dyn std::error::Error>> {
        self.inner
            .recognize_text_and_words_with_request(image, self.route(request))
    }

    fn recognize_text_and_words(
        &self,
        image: &GrayImage,
    ) -> Result<(OcrTextResult, Vec<Word>), Box<dyn std::error::Error>> {
        self.recognize_text_and_words_with_request(image, OcrRequest::default())
    }

    fn recognize_technical_text_with_request(
        &self,
        image: &GrayImage,
        request: OcrRequest,
    ) -> Result<Option<OcrTextResult>, Box<dyn std::error::Error>> {
        self.inner
            .recognize_technical_text_with_request(image, request)
    }
}

/// Trivial OCR engine that reports the image dimensions and returns an
/// empty word list. Used by tests and stub builds.
pub struct StubOcrEngine {
    label: String,
}

impl StubOcrEngine {
    pub fn new() -> Self {
        Self {
            label: "stub".to_string(),
        }
    }

    pub fn with_label(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl Default for StubOcrEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl OcrEngine for StubOcrEngine {
    fn recognize_text(&self, image: &GrayImage) -> Result<String, Box<dyn std::error::Error>> {
        Ok(format!(
            "[{}] processed {}x{} image",
            self.label,
            image.width(),
            image.height()
        ))
    }

    fn recognize_words(&self, image: &GrayImage) -> Result<Vec<Word>, Box<dyn std::error::Error>> {
        let _ = image;
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;
    use std::cell::Cell;

    #[test]
    fn stub_text_mentions_dimensions() {
        let img = GrayImage::from_pixel(10, 4, Luma([255]));
        let engine = StubOcrEngine::new();
        let text = engine.recognize_text(&img).unwrap();
        assert!(text.contains("10x4"));
    }

    #[test]
    fn stub_words_is_empty() {
        let img = GrayImage::from_pixel(3, 3, Luma([0]));
        let engine = StubOcrEngine::new();
        let words = engine.recognize_words(&img).unwrap();
        assert!(words.is_empty());
    }

    #[test]
    fn combined_recognition_has_a_compatible_default() {
        struct CountingEngine {
            text_calls: Cell<usize>,
            word_calls: Cell<usize>,
        }

        impl OcrEngine for CountingEngine {
            fn recognize_text(
                &self,
                _image: &GrayImage,
            ) -> Result<String, Box<dyn std::error::Error>> {
                self.text_calls.set(self.text_calls.get() + 1);
                Ok("page text".to_string())
            }

            fn recognize_words(
                &self,
                _image: &GrayImage,
            ) -> Result<Vec<Word>, Box<dyn std::error::Error>> {
                self.word_calls.set(self.word_calls.get() + 1);
                Ok(Vec::new())
            }
        }

        let image = GrayImage::from_pixel(2, 2, Luma([255]));
        let engine = CountingEngine {
            text_calls: Cell::new(0),
            word_calls: Cell::new(0),
        };

        let (text, words) = engine.recognize_text_and_words(&image).unwrap();

        assert_eq!(text.text, "page text");
        assert_eq!(text.confidence, None);
        assert!(words.is_empty());
        assert_eq!(engine.text_calls.get(), 1);
        assert_eq!(engine.word_calls.get(), 1);
    }

    #[test]
    fn cell_profile_does_not_learn_from_previous_cells() {
        let request = OcrRequest::single_line(300);
        assert_eq!(request.segmentation, OcrSegmentation::SingleLine);
        assert!(!request.adaptive_learning);
        assert!(!request.preserve_interword_spaces);
        assert_eq!(request.dpi, 300);
        assert_eq!(request.language_profile, OcrLanguageProfile::Configured);

        assert!(OcrRequest::page(300).adaptive_learning);
    }

    #[test]
    fn cell_profile_can_request_english_without_changing_page_default() {
        let request = OcrRequest::single_line_with_language(200, OcrLanguageProfile::EnglishOnly);
        assert_eq!(request.language_profile, OcrLanguageProfile::EnglishOnly);
        assert_eq!(
            OcrRequest::page(200).language_profile,
            OcrLanguageProfile::Configured
        );
    }

    #[test]
    fn numeric_cell_profiles_are_isolated_and_do_not_adapt() {
        let word = OcrRequest::numeric_cell(300);
        let line = OcrRequest::numeric_line(300);
        assert_eq!(word.segmentation, OcrSegmentation::SingleWordNumeric);
        assert_eq!(line.segmentation, OcrSegmentation::SingleLineNumeric);
        assert!(!word.adaptive_learning);
        assert!(!line.adaptive_learning);
        assert_eq!(word.language_profile, OcrLanguageProfile::Configured);
        assert_eq!(line.language_profile, OcrLanguageProfile::Configured);
    }

    #[test]
    fn legacy_engine_has_no_implicit_technical_model() {
        let engine = StubOcrEngine::new();
        let image = GrayImage::from_pixel(4, 4, Luma([255]));
        let result = engine
            .recognize_technical_text_with_request(&image, OcrRequest::single_line(300))
            .unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn legacy_engines_reject_opt_in_chinese_instead_of_silently_ignoring_it() {
        let engine = StubOcrEngine::new();
        let image = GrayImage::from_pixel(4, 4, Luma([255]));
        let mut request = OcrRequest::page(300);
        request.language_profile = OcrLanguageProfile::SimplifiedChinese;
        assert!(engine.recognize_text_with_request(&image, request).is_err());
        assert!(engine
            .recognize_words_with_request(&image, request)
            .is_err());
        assert!(
            LanguageRoutedOcrEngine::new(&engine, OcrLanguageProfile::SimplifiedChinese).is_err()
        );
        assert!(LanguageRoutedOcrEngine::new(&engine, OcrLanguageProfile::Configured).is_ok());
    }

    #[test]
    fn routed_engine_preserves_cell_profiles_and_independent_technical_model() {
        use std::cell::RefCell;
        #[derive(Default)]
        struct RecordingEngine {
            requests: RefCell<Vec<OcrRequest>>,
            technical_requests: RefCell<Vec<OcrRequest>>,
            combined_calls: Cell<usize>,
        }
        impl OcrEngine for RecordingEngine {
            fn validate_language_profile(
                &self,
                _profile: OcrLanguageProfile,
            ) -> Result<(), Box<dyn std::error::Error>> {
                Ok(())
            }
            fn recognize_text(
                &self,
                _image: &GrayImage,
            ) -> Result<String, Box<dyn std::error::Error>> {
                panic!("must keep request")
            }
            fn recognize_words(
                &self,
                _image: &GrayImage,
            ) -> Result<Vec<Word>, Box<dyn std::error::Error>> {
                panic!("must keep request")
            }
            fn recognize_text_with_request(
                &self,
                _image: &GrayImage,
                request: OcrRequest,
            ) -> Result<OcrTextResult, Box<dyn std::error::Error>> {
                self.requests.borrow_mut().push(request);
                Ok(OcrTextResult {
                    text: "控制电缆 RVV18*1.0".to_string(),
                    confidence: Some(0.95),
                })
            }
            fn recognize_words_with_request(
                &self,
                _image: &GrayImage,
                request: OcrRequest,
            ) -> Result<Vec<Word>, Box<dyn std::error::Error>> {
                self.requests.borrow_mut().push(request);
                Ok(Vec::new())
            }
            fn recognize_text_and_words_with_request(
                &self,
                image: &GrayImage,
                request: OcrRequest,
            ) -> Result<(OcrTextResult, Vec<Word>), Box<dyn std::error::Error>> {
                self.combined_calls.set(self.combined_calls.get() + 1);
                Ok((
                    self.recognize_text_with_request(image, request)?,
                    Vec::new(),
                ))
            }
            fn recognize_technical_text_with_request(
                &self,
                _image: &GrayImage,
                request: OcrRequest,
            ) -> Result<Option<OcrTextResult>, Box<dyn std::error::Error>> {
                self.technical_requests.borrow_mut().push(request);
                Ok(None)
            }
        }
        let inner = RecordingEngine::default();
        let image = GrayImage::from_pixel(4, 4, Luma([255]));
        let routed =
            LanguageRoutedOcrEngine::new(&inner, OcrLanguageProfile::SimplifiedChinese).unwrap();
        assert_eq!(
            routed.active_language_profile(),
            OcrLanguageProfile::SimplifiedChinese
        );
        let (result, _) = routed
            .recognize_text_and_words_with_request(&image, OcrRequest::page(400))
            .unwrap();
        assert_eq!(result.text, "控制电缆 RVV18*1.0");
        routed
            .recognize_text_with_request(&image, OcrRequest::single_line(300))
            .unwrap();
        routed
            .recognize_text_with_request(
                &image,
                OcrRequest::single_line_with_language(300, OcrLanguageProfile::EnglishOnly),
            )
            .unwrap();
        routed
            .recognize_text_with_request(
                &image,
                OcrRequest::single_line_with_language(300, OcrLanguageProfile::RussianOnly),
            )
            .unwrap();
        routed
            .recognize_technical_text_with_request(&image, OcrRequest::single_line(300))
            .unwrap();
        assert_eq!(
            inner.combined_calls.get(),
            1,
            "must not duplicate the native recognition pass"
        );
        let requests = inner.requests.borrow();
        assert_eq!(
            requests[0].language_profile,
            OcrLanguageProfile::SimplifiedChinese
        );
        assert_eq!(requests[0].dpi, 400);
        assert!(requests[0].adaptive_learning);
        assert_eq!(
            requests[1].language_profile,
            OcrLanguageProfile::SimplifiedChinese
        );
        assert!(!requests[1].adaptive_learning);
        assert_eq!(
            requests[2].language_profile,
            OcrLanguageProfile::EnglishOnlyRequired
        );
        assert_eq!(
            requests[3].language_profile,
            OcrLanguageProfile::RussianOnly
        );
        assert_eq!(
            inner.technical_requests.borrow()[0],
            OcrRequest::single_line(300)
        );
        drop(requests);
        inner
            .recognize_text_with_request(&image, OcrRequest::page(300))
            .unwrap();
        assert_eq!(
            inner.requests.borrow().last().unwrap().language_profile,
            OcrLanguageProfile::Configured,
            "routing is scoped to the adapter, not mutable engine state"
        );
        let configured =
            LanguageRoutedOcrEngine::new(&inner, OcrLanguageProfile::Configured).unwrap();
        configured
            .recognize_text_with_request(
                &image,
                OcrRequest::single_line_with_language(300, OcrLanguageProfile::EnglishOnly),
            )
            .unwrap();
        assert_eq!(
            inner.requests.borrow().last().unwrap().language_profile,
            OcrLanguageProfile::EnglishOnly,
            "default English-only fallback semantics must remain unchanged"
        );
    }
}
