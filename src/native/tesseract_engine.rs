//! Tesseract-backed OCR engine using leptess.

use std::cell::RefCell;
use std::ffi::CStr;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::{DynamicImage, GrayImage, ImageFormat};
use leptess::{leptonica::pix_read_mem, tesseract::TessApi, Variable};
use thiserror::Error;

use crate::dto::{BoundingBox, Word};
use crate::ocr::{OcrEngine, OcrLanguageProfile, OcrRequest, OcrSegmentation, OcrTextResult};

const TECHNICAL_CELL_MODEL_V1: &str = "eng_technical_eg79_v1";
const SIMPLIFIED_CHINESE_LANGUAGES: &str = "chi_sim+eng";

thread_local! {
    /// Each page worker owns one native Tesseract instance. The configuration
    /// key prevents an API initialized for one language/tessdata directory
    /// from being reused for another configuration on the same thread.
    static TESSERACT_APIS: RefCell<Vec<CachedTessApi>> = const { RefCell::new(Vec::new()) };
}

struct CachedTessApi {
    language: String,
    tessdata_path: PathBuf,
    cell_profile: bool,
    api: TessApi,
}

#[derive(Debug, Error)]
enum TesseractError {
    #[error("failed to encode image for Leptonica: {0}")]
    Encode(#[from] image::ImageError),
    #[error("failed to initialize Tesseract (language={language}, tessdata={tessdata}): {detail}. Ensure the requested traineddata files are available")]
    Init {
        language: String,
        tessdata: String,
        detail: String,
    },
    #[error("Tesseract recognition failed with status code {0}")]
    Recognition(i32),
    #[error("failed to configure Tesseract request: {0}")]
    Configure(String),
    #[error("invalid Tesseract TSV row: {0}")]
    Tsv(String),
}

/// A real OCR engine backed by the system Tesseract installation.
pub struct TesseractOcrEngine {
    lang: String,
    languages: Vec<String>,
    tessdata_path: PathBuf,
    technical_model: Option<String>,
}

impl TesseractOcrEngine {
    /// Initializes Tesseract once so missing DLLs, tessdata, or languages fail early.
    pub fn new(lang: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_with_tessdata(lang, None)
    }

    /// Initializes Tesseract eagerly using an explicit or resolved tessdata
    /// directory. This remains available to callers that want an early native
    /// runtime check.
    pub fn new_with_tessdata(
        lang: &str,
        tessdata_path: Option<&Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let engine = Self::new_lazy_with_tessdata(lang, tessdata_path)?;
        engine.with_api(&engine.lang, false, |_| Ok(()))?;
        Ok(engine)
    }

    /// Validates the requested languages and tessdata files without creating a
    /// Tesseract API instance. This lets a document consisting entirely of
    /// high-quality native PDF text finish without invoking Tesseract.
    pub fn new_lazy_with_tessdata(
        lang: &str,
        tessdata_path: Option<&Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let languages = parse_languages(lang)?;
        let tessdata_path = resolve_tessdata_path(tessdata_path)?;
        validate_language_data(&tessdata_path, &languages)?;
        let technical_model = tessdata_path
            .join(format!("{TECHNICAL_CELL_MODEL_V1}.traineddata"))
            .is_file()
            .then(|| TECHNICAL_CELL_MODEL_V1.to_string());
        Ok(Self {
            lang: languages.join("+"),
            languages,
            tessdata_path,
            technical_model,
        })
    }

    #[expect(
        clippy::manual_c_str_literals,
        reason = "Preserve the accepted OCR baseline during source-only publication."
    )]
    fn initialize(&self, language: &str) -> Result<TessApi, Box<dyn std::error::Error>> {
        // A routed language may be absent although the configured rus+eng
        // constructor succeeded. Fail clearly before Tesseract can partially
        // load a multilingual request and silently recognize English alone.
        if language.split('+').any(|part| part == "chi_sim") {
            validate_language_data(&self.tessdata_path, &parse_languages(language)?)?;
        }
        let tessdata = self.tessdata_path.to_str().ok_or_else(|| {
            Box::new(TesseractError::Init {
                language: language.to_string(),
                tessdata: self.tessdata_path.display().to_string(),
                detail: "path is not valid UTF-8".to_string(),
            }) as Box<dyn std::error::Error>
        })?;
        let mut api = TessApi::new(Some(tessdata), language).map_err(|e| {
            Box::new(TesseractError::Init {
                language: language.to_string(),
                tessdata: self.tessdata_path.display().to_string(),
                detail: e.to_string(),
            }) as Box<dyn std::error::Error>
        })?;
        // Tesseract's low-level recognizer otherwise writes recoverable
        // diagnostics to stderr. An embedding process may redirect stderr to
        // a bounded pipe and not drain it until completion, which can block a
        // multi-page job. Keep diagnostics local to the native runtime.
        api.raw
            .set_variable(
                Variable::DebugFile.as_cstr(),
                CStr::from_bytes_with_nul(b"NUL\0").expect("static NUL value"),
            )
            .map_err(|e| {
                Box::new(TesseractError::Init {
                    language: language.to_string(),
                    tessdata: self.tessdata_path.display().to_string(),
                    detail: format!("failed to redirect Tesseract diagnostics: {e}"),
                }) as Box<dyn std::error::Error>
            })?;
        Ok(api)
    }

    fn with_api<T>(
        &self,
        language: &str,
        cell_profile: bool,
        operation: impl FnOnce(&mut TessApi) -> Result<T, Box<dyn std::error::Error>>,
    ) -> Result<T, Box<dyn std::error::Error>> {
        TESSERACT_APIS.with(|slot| {
            let mut apis = slot.borrow_mut();
            let index = apis.iter().position(|cached| {
                cached.language == language
                    && cached.tessdata_path == self.tessdata_path
                    && cached.cell_profile == cell_profile
            });
            let index = match index {
                Some(index) => index,
                None => {
                    apis.push(CachedTessApi {
                        language: language.to_string(),
                        tessdata_path: self.tessdata_path.clone(),
                        cell_profile,
                        api: self.initialize(language)?,
                    });
                    apis.len() - 1
                }
            };
            operation(&mut apis[index].api)
        })
    }

    fn request_language(&self, profile: OcrLanguageProfile) -> &str {
        match profile {
            OcrLanguageProfile::SimplifiedChinese => SIMPLIFIED_CHINESE_LANGUAGES,
            OcrLanguageProfile::EnglishOnlyRequired => "eng",
            OcrLanguageProfile::EnglishOnly
                if self.languages.iter().any(|language| language == "eng") =>
            {
                "eng"
            }
            OcrLanguageProfile::RussianOnly
                if self.languages.iter().any(|language| language == "rus") =>
            {
                "rus"
            }
            OcrLanguageProfile::Configured
            | OcrLanguageProfile::EnglishOnly
            | OcrLanguageProfile::RussianOnly => &self.lang,
        }
    }

    fn recognize_with<T>(
        &self,
        image: &GrayImage,
        request: OcrRequest,
        extract: impl FnOnce(&mut TessApi) -> Result<T, Box<dyn std::error::Error>>,
    ) -> Result<T, Box<dyn std::error::Error>> {
        let language = self.request_language(request.language_profile);
        self.recognize_with_language(image, request, language, extract)
    }

    fn recognize_with_language<T>(
        &self,
        image: &GrayImage,
        request: OcrRequest,
        language: &str,
        extract: impl FnOnce(&mut TessApi) -> Result<T, Box<dyn std::error::Error>>,
    ) -> Result<T, Box<dyn std::error::Error>> {
        let encoded = encode_png(image)?;
        let pix = pix_read_mem(&encoded)?;
        let cell_profile = matches!(
            request.segmentation,
            OcrSegmentation::SingleLine
                | OcrSegmentation::SingleWordNumeric
                | OcrSegmentation::SingleLineNumeric
        );
        self.with_api(language, cell_profile, |api| {
            configure_request(api, request)?;
            api.set_image(&pix);
            api.set_source_resolution(request.dpi.clamp(70, 2_400) as i32);
            let status = api.recognize();
            if status != 0 {
                return Err(Box::new(TesseractError::Recognition(status)));
            }
            extract(api)
        })
    }

    pub fn language(&self) -> &str {
        &self.lang
    }

    pub fn tessdata_path(&self) -> &Path {
        &self.tessdata_path
    }
}

impl OcrEngine for TesseractOcrEngine {
    fn active_language_profile(&self) -> OcrLanguageProfile {
        if self.languages.iter().any(|language| language == "chi_sim") {
            OcrLanguageProfile::SimplifiedChinese
        } else {
            OcrLanguageProfile::Configured
        }
    }

    fn validate_language_profile(
        &self,
        profile: OcrLanguageProfile,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if matches!(
            profile,
            OcrLanguageProfile::SimplifiedChinese | OcrLanguageProfile::EnglishOnlyRequired
        ) {
            validate_language_data(
                &self.tessdata_path,
                &parse_languages(self.request_language(profile))?,
            )?;
        }
        Ok(())
    }

    fn begin_page(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Tesseract keeps adaptive classifier and layout state inside
        // TessBaseAPI. A document can contain many unrelated technical
        // drawings, so every worker starts each page with a clean instance.
        TESSERACT_APIS.with(|slot| {
            slot.borrow_mut().clear();
        });
        Ok(())
    }

    fn recognize_text(&self, image: &GrayImage) -> Result<String, Box<dyn std::error::Error>> {
        self.recognize_with(image, OcrRequest::default(), |api| Ok(api.get_utf8_text()?))
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

    fn recognize_text_and_words(
        &self,
        image: &GrayImage,
    ) -> Result<(OcrTextResult, Vec<Word>), Box<dyn std::error::Error>> {
        self.recognize_text_and_words_with_request(image, OcrRequest::default())
    }

    fn recognize_text_with_request(
        &self,
        image: &GrayImage,
        request: OcrRequest,
    ) -> Result<OcrTextResult, Box<dyn std::error::Error>> {
        self.recognize_with(image, request, |api| {
            let text = api.get_utf8_text()?;
            let words = parse_tsv_words(&api.get_tsv_text(0)?)?;
            Ok(OcrTextResult {
                text,
                confidence: mean_word_confidence(&words),
            })
        })
    }

    fn recognize_words_with_request(
        &self,
        image: &GrayImage,
        request: OcrRequest,
    ) -> Result<Vec<Word>, Box<dyn std::error::Error>> {
        self.recognize_with(image, request, |api| parse_tsv_words(&api.get_tsv_text(0)?))
    }

    fn recognize_technical_text_with_request(
        &self,
        image: &GrayImage,
        request: OcrRequest,
    ) -> Result<Option<OcrTextResult>, Box<dyn std::error::Error>> {
        let Some(language) = self.technical_model.as_deref() else {
            return Ok(None);
        };
        self.recognize_with_language(image, request, language, |api| {
            let text = api.get_utf8_text()?;
            let words = parse_tsv_words(&api.get_tsv_text(0)?)?;
            Ok(Some(OcrTextResult {
                text,
                confidence: mean_word_confidence(&words),
            }))
        })
    }

    fn recognize_text_and_words_with_request(
        &self,
        image: &GrayImage,
        request: OcrRequest,
    ) -> Result<(OcrTextResult, Vec<Word>), Box<dyn std::error::Error>> {
        self.recognize_with(image, request, |api| {
            let text = api.get_utf8_text()?;
            let words = parse_tsv_words(&api.get_tsv_text(0)?)?;
            let result = OcrTextResult {
                text,
                confidence: mean_word_confidence(&words),
            };
            Ok((result, words))
        })
    }
}

fn configure_request(
    api: &mut TessApi,
    request: OcrRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    let page_segmentation = match request.segmentation {
        OcrSegmentation::Automatic => b"3\0".as_slice(),
        OcrSegmentation::Sparse => b"11\0".as_slice(),
        OcrSegmentation::SingleLine => b"7\0".as_slice(),
        OcrSegmentation::SingleWordNumeric => b"8\0".as_slice(),
        OcrSegmentation::SingleLineNumeric => b"7\0".as_slice(),
    };
    let learning = if request.adaptive_learning {
        b"1\0".as_slice()
    } else {
        b"0\0".as_slice()
    };
    let spaces = if request.preserve_interword_spaces {
        b"1\0".as_slice()
    } else {
        b"0\0".as_slice()
    };
    let whitelist = if matches!(
        request.segmentation,
        OcrSegmentation::SingleWordNumeric | OcrSegmentation::SingleLineNumeric
    ) {
        b"0123456789.,-+/()\0".as_slice()
    } else {
        b"\0".as_slice()
    };
    for (variable, value) in [
        (Variable::TesseditPagesegMode, page_segmentation),
        (Variable::ClassifyEnableLearning, learning),
        (Variable::PreserveInterwordSpaces, spaces),
        (Variable::TesseditCharWhitelist, whitelist),
    ] {
        let value = CStr::from_bytes_with_nul(value).expect("static Tesseract option");
        api.raw
            .set_variable(variable.as_cstr(), value)
            .map_err(|error| {
                Box::new(TesseractError::Configure(error.to_string())) as Box<dyn std::error::Error>
            })?;
    }
    Ok(())
}

fn parse_languages(lang: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let raw = lang.split('+').map(str::trim).collect::<Vec<_>>();
    if raw.is_empty() || raw.iter().any(|code| !is_language_code(code)) {
        return Err(format!(
            "invalid OCR languages {lang:?}; use one or more traineddata names joined with '+' (for example rus+eng)"
        )
        .into());
    }
    let mut languages = raw.into_iter().map(ToOwned::to_owned).collect::<Vec<_>>();
    // Simplified Chinese documents contain Latin equipment and cable marks.
    // An explicit bare chi_sim choice must retain an English model as well.
    if languages.iter().any(|language| language == "chi_sim")
        && !languages.iter().any(|language| language == "eng")
    {
        languages.push("eng".to_string());
    }
    Ok(languages)
}

fn is_language_code(code: &str) -> bool {
    !code.is_empty()
        && code
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn resolve_tessdata_path(explicit: Option<&Path>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let candidate = match explicit {
        Some(path) => path.to_path_buf(),
        None => match std::env::var_os("TESSDATA_PREFIX") {
            Some(path) => PathBuf::from(path),
            None => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("runtime")
                .join("tessdata"),
        },
    };
    if !candidate.is_dir() {
        return Err(format!(
            "tessdata directory not found: {}. Pass an explicit tessdata path or set TESSDATA_PREFIX for this process",
            candidate.display()
        )
        .into());
    }
    Ok(candidate)
}

fn validate_language_data(
    tessdata_path: &Path,
    languages: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let missing = languages
        .iter()
        .map(|language| {
            (
                language,
                tessdata_path.join(format!("{language}.traineddata")),
            )
        })
        .filter_map(|(language, path)| (!path.is_file()).then(|| language.clone()))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "tessdata directory {} does not contain required language data: {}",
            tessdata_path.display(),
            missing.join(", ")
        )
        .into())
    }
}

fn mean_word_confidence(words: &[Word]) -> Option<f32> {
    (!words.is_empty()).then(|| {
        (words.iter().map(|word| word.confidence).sum::<f32>() / words.len() as f32).clamp(0.0, 1.0)
    })
}

fn encode_png(image: &GrayImage) -> Result<Vec<u8>, TesseractError> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(image.clone()).write_to(&mut bytes, ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

fn parse_tsv_words(tsv: &str) -> Result<Vec<Word>, Box<dyn std::error::Error>> {
    let mut words = Vec::new();
    for (line_number, line) in tsv.lines().enumerate().skip(1) {
        let fields: Vec<&str> = line.splitn(12, '\t').collect();
        if fields.len() != 12 {
            return Err(Box::new(TesseractError::Tsv(format!(
                "line {line_number} has {} fields, expected 12",
                fields.len()
            ))));
        }
        let text = fields[11].trim();
        if text.is_empty() {
            continue;
        }
        let left: f64 = fields[6].parse().map_err(|_| {
            Box::new(TesseractError::Tsv(format!(
                "line {line_number}: invalid left coordinate"
            ))) as Box<dyn std::error::Error>
        })?;
        let top: f64 = fields[7].parse().map_err(|_| {
            Box::new(TesseractError::Tsv(format!(
                "line {line_number}: invalid top coordinate"
            ))) as Box<dyn std::error::Error>
        })?;
        let width: f64 = fields[8].parse().map_err(|_| {
            Box::new(TesseractError::Tsv(format!(
                "line {line_number}: invalid width"
            ))) as Box<dyn std::error::Error>
        })?;
        let height: f64 = fields[9].parse().map_err(|_| {
            Box::new(TesseractError::Tsv(format!(
                "line {line_number}: invalid height"
            ))) as Box<dyn std::error::Error>
        })?;
        let confidence: f32 = fields[10].parse().map_err(|_| {
            Box::new(TesseractError::Tsv(format!(
                "line {line_number}: invalid confidence"
            ))) as Box<dyn std::error::Error>
        })?;
        if confidence < 0.0 {
            continue;
        }
        words.push(Word::new(
            text,
            BoundingBox::new(left, top, left + width, top + height),
            (confidence / 100.0).clamp(0.0, 1.0),
        ));
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tsv_words_extracts_text_and_bbox() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n1\t1\t1\t1\t1\t1\t10\t20\t50\t15\t85.5\thello\n1\t1\t1\t1\t1\t2\t70\t20\t60\t15\t92.0\tworld\n";
        let words = parse_tsv_words(tsv).unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "hello");
        assert_eq!(words[0].bbox.x0, 10.0);
        assert_eq!(words[0].bbox.y0, 20.0);
        assert_eq!(words[0].bbox.x1, 60.0); // 10 + 50
        assert_eq!(words[0].bbox.y1, 35.0); // 20 + 15
        assert!((words[0].confidence - 0.855).abs() < 0.001);
        assert_eq!(words[1].text, "world");
    }

    #[test]
    fn parse_tsv_words_skips_empty_text() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n1\t1\t1\t1\t1\t1\t10\t20\t50\t15\t85.5\t\n1\t1\t1\t1\t1\t2\t70\t20\t60\t15\t92.0\thi\n";
        let words = parse_tsv_words(tsv).unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, "hi");
    }

    #[test]
    fn parse_tsv_words_header_only_returns_empty() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n";
        let words = parse_tsv_words(tsv).unwrap();
        assert!(words.is_empty());
    }

    #[test]
    fn parse_tsv_words_rejects_bad_field_count() {
        let tsv = "level\tpage_num\n1\t1\n";
        let result = parse_tsv_words(tsv);
        assert!(result.is_err());
    }

    #[test]
    fn language_parser_accepts_explicit_russian_and_english() {
        assert_eq!(parse_languages("rus+eng").unwrap(), vec!["rus", "eng"]);
        assert!(parse_languages("rus+").is_err());
        assert!(parse_languages("rus+ bad language").is_err());
    }

    #[test]
    fn chinese_configuration_always_includes_english() {
        assert_eq!(parse_languages("chi_sim").unwrap(), vec!["chi_sim", "eng"]);
        assert_eq!(
            parse_languages("chi_sim+eng").unwrap(),
            vec!["chi_sim", "eng"]
        );
        assert_eq!(parse_languages("rus+eng").unwrap(), vec!["rus", "eng"]);
    }

    #[test]
    fn chinese_route_requires_both_models_without_changing_configured_languages() {
        let dir = tempfile::tempdir().unwrap();
        for language in ["rus", "eng"] {
            std::fs::write(
                dir.path().join(format!("{language}.traineddata")),
                b"fixture",
            )
            .unwrap();
        }
        let engine =
            TesseractOcrEngine::new_lazy_with_tessdata("rus+eng", Some(dir.path())).unwrap();
        assert!(engine
            .validate_language_profile(OcrLanguageProfile::Configured)
            .is_ok());
        let missing = engine
            .validate_language_profile(OcrLanguageProfile::SimplifiedChinese)
            .unwrap_err();
        assert!(missing.to_string().contains("chi_sim"));
        assert_eq!(
            engine.request_language(OcrLanguageProfile::SimplifiedChinese),
            "chi_sim+eng"
        );
        assert_eq!(
            engine.request_language(OcrLanguageProfile::Configured),
            "rus+eng"
        );
        std::fs::write(dir.path().join("chi_sim.traineddata"), b"fixture").unwrap();
        engine
            .validate_language_profile(OcrLanguageProfile::SimplifiedChinese)
            .unwrap();
        let chinese =
            TesseractOcrEngine::new_lazy_with_tessdata("chi_sim", Some(dir.path())).unwrap();
        assert_eq!(chinese.language(), "chi_sim+eng");
        assert_eq!(
            chinese.active_language_profile(),
            OcrLanguageProfile::SimplifiedChinese
        );
        assert_eq!(
            engine.active_language_profile(),
            OcrLanguageProfile::Configured
        );
        assert_eq!(
            chinese.request_language(OcrLanguageProfile::EnglishOnly),
            "eng"
        );
    }

    #[test]
    fn explicit_chinese_missing_model_is_an_error_even_for_lazy_constructor() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("eng.traineddata"), b"fixture").unwrap();
        let error = TesseractOcrEngine::new_lazy_with_tessdata("chi_sim", Some(dir.path()))
            .err()
            .unwrap();
        assert!(error.to_string().contains("chi_sim"));
        std::fs::write(dir.path().join("chi_sim.traineddata"), b"fixture").unwrap();
        std::fs::remove_file(dir.path().join("eng.traineddata")).unwrap();
        let error = TesseractOcrEngine::new_lazy_with_tessdata("chi_sim", Some(dir.path()))
            .err()
            .unwrap();
        assert!(error.to_string().contains("eng"));
    }

    #[test]
    fn chinese_scoped_english_route_works_when_global_configuration_is_russian_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rus.traineddata"), b"fixture").unwrap();
        let engine = TesseractOcrEngine::new_lazy_with_tessdata("rus", Some(dir.path())).unwrap();
        assert_eq!(
            engine.request_language(OcrLanguageProfile::EnglishOnly),
            "rus"
        );
        assert_eq!(
            engine.request_language(OcrLanguageProfile::EnglishOnlyRequired),
            "eng"
        );
        let error = engine
            .validate_language_profile(OcrLanguageProfile::EnglishOnlyRequired)
            .unwrap_err();
        assert!(error.to_string().contains("eng"));
        std::fs::write(dir.path().join("eng.traineddata"), b"fixture").unwrap();
        engine
            .validate_language_profile(OcrLanguageProfile::EnglishOnlyRequired)
            .unwrap();
        assert_eq!(engine.language(), "rus");
    }

    #[test]
    fn tsv_preserves_chinese_and_latin_cell_geometry_without_text_substitution() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n5\t1\t1\t1\t1\t1\t10\t20\t80\t15\t95\t控制电缆\n5\t1\t1\t1\t1\t2\t110\t20\t100\t15\t92\tRVV18*1.0\n5\t1\t1\t1\t2\t1\t10\t45\t90\t15\t93\tVVR3*25+2*10\n5\t1\t1\t1\t2\t2\t110\t45\t80\t15\t96\tM16X200\n";
        let words = parse_tsv_words(tsv).unwrap();
        assert_eq!(
            words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            vec!["控制电缆", "RVV18*1.0", "VVR3*25+2*10", "M16X200"]
        );
        assert_eq!(words[0].bbox, BoundingBox::new(10.0, 20.0, 90.0, 35.0));
        assert_eq!(words[2].bbox.y0, 45.0);
        assert_eq!(words[3].bbox.x0, 110.0);
    }

    #[test]
    fn request_language_uses_english_only_when_already_configured() {
        let engine = TesseractOcrEngine {
            lang: "rus+eng".to_string(),
            languages: vec!["rus".to_string(), "eng".to_string()],
            tessdata_path: PathBuf::from("fixture"),
            technical_model: None,
        };
        assert_eq!(
            engine.request_language(OcrLanguageProfile::Configured),
            "rus+eng"
        );
        assert_eq!(
            engine.request_language(OcrLanguageProfile::EnglishOnly),
            "eng"
        );
        assert_eq!(
            engine.request_language(OcrLanguageProfile::RussianOnly),
            "rus"
        );

        let russian_only = TesseractOcrEngine {
            lang: "rus".to_string(),
            languages: vec!["rus".to_string()],
            tessdata_path: PathBuf::from("fixture"),
            technical_model: None,
        };
        assert_eq!(
            russian_only.request_language(OcrLanguageProfile::EnglishOnly),
            "rus"
        );
        assert_eq!(
            russian_only.request_language(OcrLanguageProfile::RussianOnly),
            "rus"
        );
    }

    #[test]
    fn mean_confidence_is_bounded() {
        let words = vec![
            Word::new("a", BoundingBox::new(0.0, 0.0, 1.0, 1.0), 0.8),
            Word::new("b", BoundingBox::new(1.0, 0.0, 2.0, 1.0), 1.0),
        ];
        assert_eq!(mean_word_confidence(&words), Some(0.9));
        assert_eq!(mean_word_confidence(&[]), None);
    }

    #[test]
    fn language_data_validation_requires_every_requested_model() {
        let dir = tempfile::tempdir().expect("temporary tessdata directory");
        std::fs::write(dir.path().join("eng.traineddata"), b"fixture")
            .expect("write English fixture");
        std::fs::write(dir.path().join("rus.traineddata"), b"fixture")
            .expect("write Russian fixture");
        let languages = parse_languages("rus+eng").expect("parse languages");
        validate_language_data(dir.path(), &languages).expect("both files exist");

        let missing = parse_languages("rus+deu").expect("parse languages");
        let error = validate_language_data(dir.path(), &missing).unwrap_err();
        assert!(error.to_string().contains("deu"));
    }

    #[test]
    fn versioned_technical_model_is_optional_and_does_not_change_page_languages() {
        let dir = tempfile::tempdir().expect("temporary tessdata directory");
        std::fs::write(dir.path().join("eng.traineddata"), b"fixture")
            .expect("write English fixture");
        std::fs::write(dir.path().join("rus.traineddata"), b"fixture")
            .expect("write Russian fixture");

        let without = TesseractOcrEngine::new_lazy_with_tessdata("rus+eng", Some(dir.path()))
            .expect("configured languages remain sufficient");
        assert_eq!(without.language(), "rus+eng");
        assert_eq!(without.technical_model, None);

        std::fs::write(
            dir.path()
                .join(format!("{TECHNICAL_CELL_MODEL_V1}.traineddata")),
            b"separate fixture",
        )
        .expect("write technical fixture");
        let with = TesseractOcrEngine::new_lazy_with_tessdata("rus+eng", Some(dir.path()))
            .expect("technical data is additive");
        assert_eq!(with.language(), "rus+eng");
        assert_eq!(
            with.technical_model.as_deref(),
            Some(TECHNICAL_CELL_MODEL_V1)
        );
    }
}
