use std::{error::Error, sync::Mutex};

use image::{GrayImage, Luma};
use ocr_pipeline::{
    LanguageRouting, NativeTextLayer, OcrEngine, OcrLanguageProfile, OcrOptions, OcrRequest,
    OcrTextResult, PageSource, Pipeline, RenderedPage, TableMode, Word,
};

struct Source(Vec<Option<&'static str>>);
impl PageSource for Source {
    fn total_pages(&self) -> Result<u32, Box<dyn Error>> {
        Ok(self.0.len() as u32)
    }
    fn render_page(&mut self, page: u32) -> Result<RenderedPage, Box<dyn Error>> {
        Ok(RenderedPage::new(
            page,
            GrayImage::from_pixel(160, 80, Luma([255])),
            300.0,
        ))
    }
    fn native_text_for_page(&self, page: u32) -> Result<Option<NativeTextLayer>, Box<dyn Error>> {
        // Partial text evidence is reliable for script selection, but too
        // incomplete to bypass OCR. No synthetic recognition is scored here.
        Ok(self.0[page as usize - 1].map(|text| NativeTextLayer::new(text.into(), 1000)))
    }
}

#[derive(Default)]
struct RecordingEngine(Mutex<Vec<OcrLanguageProfile>>);
impl OcrEngine for RecordingEngine {
    fn validate_language_profile(&self, _: OcrLanguageProfile) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn recognize_text(&self, _: &GrayImage) -> Result<String, Box<dyn Error>> {
        panic!("pipeline must retain request language")
    }
    fn recognize_words(&self, _: &GrayImage) -> Result<Vec<Word>, Box<dyn Error>> {
        panic!("pipeline must retain request language")
    }
    fn recognize_text_with_request(
        &self,
        _: &GrayImage,
        request: OcrRequest,
    ) -> Result<OcrTextResult, Box<dyn Error>> {
        self.0.lock().unwrap().push(request.language_profile);
        Ok(OcrTextResult {
            text: "线缆桥架清单 VVR3*25+2*10".into(),
            confidence: Some(0.95),
        })
    }
    fn recognize_words_with_request(
        &self,
        _: &GrayImage,
        request: OcrRequest,
    ) -> Result<Vec<Word>, Box<dyn Error>> {
        self.0.lock().unwrap().push(request.language_profile);
        Ok(vec![])
    }
}

#[test]
fn auto_routes_each_page_independently_in_serial_and_parallel() {
    for workers in [1, 3] {
        let mut source = Source(vec![
            Some("线缆桥架清单"),
            Some("Cable schedule 2026"),
            None,
        ]);
        let engine = RecordingEngine::default();
        let outcome = Pipeline::new(&mut source, &engine, 100_000, None)
            .with_table_mode(TableMode::Off)
            .with_language_routing(LanguageRouting::AutoChinese)
            .run_with_warnings_parallel(workers)
            .unwrap();
        let languages: Vec<_> = outcome
            .page_processing
            .iter()
            .map(|p| p.language.as_str())
            .collect();
        assert_eq!(languages, ["chi_sim+eng", "rus+eng", "rus+eng"]);
        let calls = engine.0.lock().unwrap();
        assert!(calls.contains(&OcrLanguageProfile::SimplifiedChinese));
        assert!(calls.contains(&OcrLanguageProfile::Configured));
    }
}

#[test]
fn configured_default_never_invokes_chinese_even_with_han_evidence() {
    let mut source = Source(vec![Some("线缆桥架清单")]);
    let engine = RecordingEngine::default();
    let outcome = Pipeline::new(&mut source, &engine, 100_000, None)
        .with_table_mode(TableMode::Off)
        .run_with_warnings()
        .unwrap();
    assert_eq!(outcome.page_processing[0].language, "rus+eng");
    assert!(engine
        .0
        .lock()
        .unwrap()
        .iter()
        .all(|p| *p == OcrLanguageProfile::Configured));
}

#[test]
fn old_serialized_options_preserve_configured_route_and_default_languages() {
    let mut value = serde_json::to_value(OcrOptions::default()).unwrap();
    value.as_object_mut().unwrap().remove("language_routing");
    let restored: OcrOptions = serde_json::from_value(value).unwrap();
    assert_eq!(restored.language_routing, LanguageRouting::Configured);
    assert_eq!(restored.effective_languages().unwrap(), "rus+eng");
}

#[test]
fn chinese_native_text_is_usable_without_false_unexpected_script_warning() {
    let text = "线缆桥架清单电缆型号设备名称数量规格".repeat(3);
    let layer = NativeTextLayer::new(text.clone(), text.chars().count());
    let chinese =
        ocr_pipeline::page_analysis::analyze_native_text_for_languages(&layer, "chi_sim+eng");
    assert!(chinese.is_usable);
    assert_eq!(chinese.supported_script_ratio, 1.0);
    let legacy = ocr_pipeline::page_analysis::analyze_native_text_for_languages(&layer, "rus+eng");
    assert!(!legacy.is_usable);
    let quality = ocr_pipeline::assess_page_quality(
        &text,
        "chi_sim+eng",
        ocr_pipeline::ProcessingMode::NativeText,
        None,
        Some(&chinese),
        None,
    );
    assert_eq!(quality.signals.unexpected_script_ratio, 0.0);
}
