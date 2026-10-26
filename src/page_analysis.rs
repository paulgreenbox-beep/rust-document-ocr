//! Stable, serializable page-processing diagnostics.
//!
//! These types deliberately sit outside the legacy DTO model. They are an
//! additive part of the schema-v1 integration envelope and do not change the
//! pre-v1 `result.json` shape used by existing CLI consumers.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Text returned by a PDF's native text layer.
///
/// `pdfium_character_count` is kept separately from Rust's Unicode character
/// count because PDFium may report a different number for ligatures or
/// malformed content. Both values are useful when deciding whether OCR is
/// necessary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTextLayer {
    pub text: String,
    pub pdfium_character_count: usize,
}

impl NativeTextLayer {
    pub fn new(text: String, pdfium_character_count: usize) -> Self {
        Self {
            text,
            pdfium_character_count,
        }
    }
}

/// Geometry-only evidence extracted from PDFium's native text segments.
///
/// This deliberately retains counts rather than document text or individual
/// rectangles. It is an inexpensive supporting signal for routing searchable
/// pages through table analysis; final acceptance still requires either a
/// positive ruled-grid result or a high-confidence positioned-word layout.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeTextLayout {
    /// Number of rectangular native-text segments reported by PDFium.
    pub text_block_count: usize,
    /// Repeated left/right alignment positions shared by at least three blocks.
    pub x_alignment_clusters: usize,
    /// Repeated top/bottom alignment positions shared by at least three blocks.
    pub y_alignment_clusters: usize,
}

impl NativeTextLayout {
    /// Builds deterministic layout evidence from PDF-space text block bounds.
    ///
    /// Bounds are `(left, bottom, right, top)` in PDF points. Nearby positions
    /// are grouped into four-point buckets so small font/layout variations do
    /// not erase an otherwise clear column or row alignment.
    pub fn from_block_bounds(bounds: impl IntoIterator<Item = (f32, f32, f32, f32)>) -> Self {
        const ALIGNMENT_BUCKET_POINTS: f32 = 4.0;
        const MIN_BLOCKS_PER_CLUSTER: usize = 3;

        let mut layout = Self::default();
        let mut x_positions = Vec::new();
        let mut y_positions = Vec::new();

        for (left, bottom, right, top) in bounds {
            if !left.is_finite() || !bottom.is_finite() || !right.is_finite() || !top.is_finite() {
                continue;
            }
            layout.text_block_count += 1;
            x_positions.extend([left, right]);
            y_positions.extend([bottom, top]);
        }

        layout.x_alignment_clusters =
            alignment_cluster_count(x_positions, ALIGNMENT_BUCKET_POINTS, MIN_BLOCKS_PER_CLUSTER);
        layout.y_alignment_clusters =
            alignment_cluster_count(y_positions, ALIGNMENT_BUCKET_POINTS, MIN_BLOCKS_PER_CLUSTER);
        layout
    }

    /// Returns whether native text has recurring two-dimensional alignment.
    ///
    /// This is only a cheap gate for the local borderless detector. It never
    /// changes source text, creates cells, or routes a searchable page through
    /// Tesseract by itself.
    pub fn has_coordinate_clusters(&self) -> bool {
        self.text_block_count >= 3
            && self.x_alignment_clusters >= 2
            && self.y_alignment_clusters >= 2
    }
}

fn alignment_cluster_count(
    positions: impl IntoIterator<Item = f32>,
    bucket_size: f32,
    minimum_samples: usize,
) -> usize {
    let mut buckets = BTreeMap::<i32, usize>::new();
    for position in positions {
        let bucket = (position / bucket_size)
            .round()
            .clamp(i32::MIN as f32, i32::MAX as f32) as i32;
        *buckets.entry(bucket).or_default() += 1;
    }
    buckets
        .into_values()
        .filter(|count| *count >= minimum_samples)
        .count()
}

/// Deterministic quality metrics for a PDF text layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct NativeTextQuality {
    pub pdfium_character_count: usize,
    pub unicode_character_count: usize,
    #[serde(default)]
    pub unicode_coverage: f32,
    pub non_whitespace_characters: usize,
    pub printable_characters: usize,
    pub printable_ratio: f32,
    pub replacement_characters: usize,
    #[serde(default)]
    pub replacement_ratio: f32,
    pub control_characters: usize,
    #[serde(default)]
    pub cyrillic_characters: usize,
    #[serde(default)]
    pub latin_characters: usize,
    #[serde(default)]
    pub other_alphabetic_characters: usize,
    #[serde(default = "default_supported_script_ratio")]
    pub supported_script_ratio: f32,
    /// Characters in dense Latin-1 runs that resemble Cyrillic CP1251 bytes
    /// decoded as Latin-1 (for example, `\u{00ce}\u{00e1}\u{00f9}\u{00e8}\u{00e5}`).
    #[serde(default)]
    pub suspected_mojibake_characters: usize,
    #[serde(default)]
    pub suspected_mojibake_runs: usize,
    #[serde(default)]
    pub longest_suspected_mojibake_run: usize,
    #[serde(default)]
    pub suspected_mojibake_ratio: f32,
    /// Strong evidence requires multiple dense runs or one unusually long
    /// run; isolated accented names do not set this flag.
    #[serde(default)]
    pub has_strong_mojibake: bool,
    pub contains_cyrillic: bool,
    pub contains_latin: bool,
    pub contains_digits: bool,
    pub is_usable: bool,
    /// A high-coverage layer with a very small number of missing mappings.
    /// It is retained only for comparison with OCR, never accepted blindly.
    #[serde(default)]
    pub is_near_usable: bool,
}

fn default_supported_script_ratio() -> f32 {
    // Older schema-v1 payloads predate script-aware diagnostics. Treat the
    // absent diagnostic as neutral rather than falsely reporting 0% support.
    1.0
}

/// Replaces PDF control glyphs that cannot be represented meaningfully in
/// JSON, DOCX, or plain text while preserving real line and tab separators.
///
/// Some CAD generators emit small C0 control codes as spacing glyphs. They
/// must not invalidate an otherwise complete searchable text layer, and they
/// must not leak into exported documents.
pub fn sanitize_native_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() && !matches!(character, '\n' | '\r' | '\t') {
                ' '
            } else {
                character
            }
        })
        .collect()
}

/// Analyses whether native text is sufficiently readable to avoid raster OCR.
///
/// A layer must be mostly printable, contain useful letters or digits, and
/// cover at least 75% of PDFium's reported characters. A small number of CAD
/// control glyphs is tolerated; production PDF extraction resolves their runs
/// according to bounded-text coverage.
pub fn analyze_native_text(layer: &NativeTextLayer) -> NativeTextQuality {
    analyze_native_text_for_languages(layer, "")
}

/// Analyses a native layer against the scripts requested for OCR.
///
/// Known Latin/Cyrillic language codes activate a conservative unexpected-
/// script gate. Unknown language packs retain the historical script-agnostic
/// behavior rather than rejecting legitimate multilingual documents.
pub fn analyze_native_text_for_languages(
    layer: &NativeTextLayer,
    languages: &str,
) -> NativeTextQuality {
    let mut quality = NativeTextQuality {
        pdfium_character_count: layer.pdfium_character_count,
        ..NativeTextQuality::default()
    };

    for character in layer.text.chars() {
        quality.unicode_character_count += 1;
        if !character.is_whitespace() {
            quality.non_whitespace_characters += 1;
        }
        if is_printable(character) {
            quality.printable_characters += 1;
        }
        if character == '\u{fffd}' {
            quality.replacement_characters += 1;
        }
        if character.is_control() && !matches!(character, '\n' | '\r' | '\t') {
            quality.control_characters += 1;
        }
        if is_cyrillic(character) {
            quality.cyrillic_characters += 1;
        } else if is_latin(character) {
            quality.latin_characters += 1;
        } else if character.is_alphabetic() {
            quality.other_alphabetic_characters += 1;
        }
        quality.contains_cyrillic |= is_cyrillic(character);
        quality.contains_latin |= is_latin(character);
        quality.contains_digits |= character.is_ascii_digit();
    }

    quality.printable_ratio = if quality.unicode_character_count == 0 {
        0.0
    } else {
        quality.printable_characters as f32 / quality.unicode_character_count as f32
    };
    quality.unicode_coverage = if quality.pdfium_character_count == 0 {
        0.0
    } else {
        quality.unicode_character_count as f32 / quality.pdfium_character_count as f32
    };
    quality.replacement_ratio = if quality.unicode_character_count == 0 {
        0.0
    } else {
        quality.replacement_characters as f32 / quality.unicode_character_count as f32
    };
    let control_ratio = if quality.unicode_character_count == 0 {
        0.0
    } else {
        quality.control_characters as f32 / quality.unicode_character_count as f32
    };
    let han_characters = if languages
        .split('+')
        .any(|language| language.trim() == "chi_sim")
    {
        layer
            .text
            .chars()
            .filter(|character| crate::language::is_han(*character))
            .count()
    } else {
        0
    };
    let has_semantic_content = quality.contains_cyrillic
        || quality.contains_latin
        || quality.contains_digits
        || han_characters > 0;
    let (allow_cyrillic, allow_latin, enforce_script) = expected_scripts(languages);
    let supported_characters = usize::from(allow_cyrillic) * quality.cyrillic_characters
        + usize::from(allow_latin) * quality.latin_characters
        + han_characters;
    let alphabetic_characters = quality.cyrillic_characters
        + quality.latin_characters
        + quality.other_alphabetic_characters;
    let mojibake = latin1_cyrillic_mojibake_evidence(&layer.text);
    quality.suspected_mojibake_characters = mojibake.characters;
    quality.suspected_mojibake_runs = mojibake.runs;
    quality.longest_suspected_mojibake_run = mojibake.longest_run;
    quality.suspected_mojibake_ratio = mojibake.alphabetic_ratio;
    quality.has_strong_mojibake = mojibake.is_strong;
    quality.supported_script_ratio = if alphabetic_characters == 0 || !enforce_script {
        1.0
    } else {
        supported_characters as f32 / alphabetic_characters as f32
    };
    let script_is_plausible =
        !enforce_script || alphabetic_characters < 32 || quality.supported_script_ratio >= 0.80;
    quality.is_usable = quality.non_whitespace_characters >= 8
        && quality.printable_ratio >= 0.90
        && quality.replacement_characters == 0
        && control_ratio <= 0.05
        && quality.unicode_coverage >= 0.75
        && has_semantic_content
        && script_is_plausible;
    quality.is_near_usable = !quality.is_usable
        && quality.non_whitespace_characters >= 8
        && quality.printable_ratio >= 0.98
        && quality.unicode_coverage >= 0.95
        && quality.replacement_characters <= 16
        && quality.replacement_ratio <= 0.005
        && control_ratio <= 0.05
        && has_semantic_content
        && script_is_plausible;
    quality
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct MojibakeEvidence {
    pub characters: usize,
    pub runs: usize,
    pub longest_run: usize,
    pub alphabetic_ratio: f32,
    pub is_strong: bool,
}

/// Detects the characteristic shape of CP1251 Cyrillic bytes interpreted as
/// Latin-1. The detector intentionally ignores short runs: ordinary English
/// and isolated accented manufacturer names must remain valid native text.
pub(crate) fn latin1_cyrillic_mojibake_evidence(text: &str) -> MojibakeEvidence {
    const MIN_CANDIDATE_RUN: usize = 4;
    const MIN_STRONG_CHARACTERS: usize = 10;
    const MIN_STRONG_RUNS: usize = 2;
    const MIN_SINGLE_STRONG_RUN: usize = 12;
    const MIN_ALPHABETIC_RATIO: f32 = 0.20;

    let alphabetic_characters = text
        .chars()
        .filter(|character| character.is_alphabetic())
        .count();
    let mut evidence = MojibakeEvidence::default();
    let mut run = 0usize;
    let finish_run = |run: usize, evidence: &mut MojibakeEvidence| {
        if run >= MIN_CANDIDATE_RUN {
            evidence.characters += run;
            evidence.runs += 1;
            evidence.longest_run = evidence.longest_run.max(run);
        }
    };

    for character in text.chars() {
        if is_latin1_cyrillic_byte(character) {
            run += 1;
        } else {
            finish_run(run, &mut evidence);
            run = 0;
        }
    }
    finish_run(run, &mut evidence);

    evidence.alphabetic_ratio = if alphabetic_characters == 0 {
        0.0
    } else {
        evidence.characters as f32 / alphabetic_characters as f32
    };
    evidence.is_strong = evidence.alphabetic_ratio >= MIN_ALPHABETIC_RATIO
        && ((evidence.characters >= MIN_STRONG_CHARACTERS && evidence.runs >= MIN_STRONG_RUNS)
            || evidence.longest_run >= MIN_SINGLE_STRONG_RUN);
    evidence
}

fn is_latin1_cyrillic_byte(character: char) -> bool {
    ('\u{00c0}'..='\u{00ff}').contains(&character) && character.is_alphabetic()
}

pub(crate) fn is_cyrillic(character: char) -> bool {
    ('\u{0400}'..='\u{052f}').contains(&character)
        || ('\u{2de0}'..='\u{2dff}').contains(&character)
        || ('\u{a640}'..='\u{a69f}').contains(&character)
}

pub(crate) fn is_latin(character: char) -> bool {
    character.is_ascii_alphabetic()
        || ('\u{00c0}'..='\u{024f}').contains(&character)
        || ('\u{1e00}'..='\u{1eff}').contains(&character)
}

pub(crate) fn expected_scripts(languages: &str) -> (bool, bool, bool) {
    let mut allow_cyrillic = false;
    let mut allow_latin = false;
    let mut recognized = false;
    for language in languages
        .split(['+', ',', ';'])
        .map(str::trim)
        .filter(|language| !language.is_empty())
    {
        match language {
            "rus" | "ukr" | "bel" | "bul" | "mkd" | "srp" | "kaz" | "kir" | "tat" => {
                allow_cyrillic = true;
                recognized = true;
            }
            "eng" | "deu" | "fra" | "spa" | "ita" | "por" | "nld" | "pol" | "ces" | "slk"
            | "ron" | "hun" | "tur" | "aze" | "uzb" => {
                allow_latin = true;
                recognized = true;
            }
            _ => {}
        }
    }
    (allow_cyrillic, allow_latin, recognized)
}

fn is_printable(character: char) -> bool {
    !character.is_control() || matches!(character, '\n' | '\r' | '\t')
}

/// The source chosen for a processed page.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingMode {
    NativeText,
    /// Native text remains the canonical page text while OCR-derived grid
    /// geometry and cell content are attached as an additional result.
    #[serde(rename = "native_text+grid", alias = "grid_ocr")]
    NativeTextWithGrid,
    #[default]
    Ocr,
    /// A raster-only page was OCRed and also contains reconstructed tables.
    #[serde(rename = "ocr+grid")]
    OcrWithGrid,
}

/// Orientation applied to a raster page before grid detection and OCR.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct PageOrientation {
    /// Clockwise rotation in degrees. Only 0, 90, 180, and 270 are emitted.
    pub angle: u16,
    /// Confidence based on the orientation OCR preflight. `0.0` means no
    /// reliable raster-orientation decision was required or available.
    pub confidence: f32,
}

/// Dimensions of the actual raster image passed through OCR processing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RenderSize {
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
}

/// Page-wide accounting for OCR words used by table reconstruction.
///
/// Counts are based on unique OCR words, so overlapping grid candidates do
/// not make `assigned_words` exceed `recognized_words`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageWordAssignmentMetrics {
    pub recognized_words: u32,
    pub assigned_words: u32,
    pub words_outside_tables: u32,
    #[serde(default)]
    pub duplicate_assignments: u32,
}

/// Wall-clock timings for one page. Values are milliseconds and deliberately
/// remain additive diagnostics rather than routing inputs.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct PageStageTimings {
    pub native_text_extraction_ms: f64,
    pub native_text_layout_ms: f64,
    pub thumbnail_render_ms: f64,
    pub full_render_ms: f64,
    pub grid_detection_ms: f64,
    pub orientation_ms: f64,
    pub preprocessing_ms: f64,
    pub tesseract_ms: f64,
    pub assignment_ms: f64,
    pub total_ms: f64,
}

/// Per-page diagnostics carried by the stable schema-v1 result envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageProcessingInfo {
    pub page_number: u32,
    pub processing_mode: ProcessingMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_text_quality: Option<NativeTextQuality>,
    /// Supporting native-text geometry used by the NativeText/Grid OCR route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_text_layout: Option<NativeTextLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ocr_confidence: Option<f32>,
    pub orientation: PageOrientation,
    /// Requested Tesseract language string. This is retained even for
    /// `native_text` pages so the run configuration remains reproducible.
    pub language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_size: Option<RenderSize>,
    /// Present only when table reconstruction consumed OCR word boxes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_assignment: Option<PageWordAssignmentMetrics>,
    /// Additive intrinsic quality diagnostics. Older schema-v1 consumers may
    /// ignore this field; no reference text is required or embedded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<crate::quality::PageQualityReport>,
    #[serde(default)]
    pub timings: PageStageTimings,
}

#[cfg(test)]
mod tests {
    use super::{
        analyze_native_text, analyze_native_text_for_languages, sanitize_native_text,
        NativeTextLayer, NativeTextLayout, PageProcessingInfo, ProcessingMode,
    };

    #[test]
    fn old_schema_v1_page_processing_without_quality_still_deserializes() {
        let old_json = r#"
        {
          "page_number": 1,
          "processing_mode": "native_text",
          "native_text_quality": {
            "pdfium_character_count": 42,
            "unicode_character_count": 42,
            "non_whitespace_characters": 36,
            "printable_characters": 42,
            "printable_ratio": 1.0,
            "replacement_characters": 0,
            "control_characters": 0,
            "contains_cyrillic": true,
            "contains_latin": false,
            "contains_digits": true,
            "is_usable": true
          },
          "orientation": { "angle": 0, "confidence": 1.0 },
          "language": "rus+eng"
        }
        "#;

        let processing: PageProcessingInfo =
            serde_json::from_str(old_json).expect("old schema-v1 diagnostics must deserialize");
        let native = processing
            .native_text_quality
            .expect("old native quality must be retained");
        assert!(processing.quality.is_none());
        assert_eq!(native.unicode_coverage, 0.0);
        assert_eq!(native.supported_script_ratio, 1.0);
        assert_eq!(native.suspected_mojibake_characters, 0);
        assert_eq!(native.suspected_mojibake_runs, 0);
        assert_eq!(native.longest_suspected_mojibake_run, 0);
        assert_eq!(native.suspected_mojibake_ratio, 0.0);
        assert!(!native.has_strong_mojibake);
        assert!(!native.is_near_usable);
        assert!(native.is_usable);
    }

    #[test]
    fn processing_modes_use_v1_2_wire_names_and_accept_v1_1_grid_ocr() {
        assert_eq!(
            serde_json::to_string(&ProcessingMode::NativeTextWithGrid).unwrap(),
            "\"native_text+grid\""
        );
        assert_eq!(
            serde_json::to_string(&ProcessingMode::OcrWithGrid).unwrap(),
            "\"ocr+grid\""
        );
        assert_eq!(
            serde_json::from_str::<ProcessingMode>("\"grid_ocr\"").unwrap(),
            ProcessingMode::NativeTextWithGrid
        );
    }

    #[test]
    fn readable_russian_and_english_layer_is_usable() {
        let layer = NativeTextLayer::new(
            "Проект Alpha 42: readable native text layer for OCR Core.".to_string(),
            55,
        );
        let quality = analyze_native_text(&layer);
        assert!(quality.is_usable);
        assert!(quality.contains_cyrillic);
        assert!(quality.contains_latin);
        assert!(quality.contains_digits);
        assert_eq!(quality.pdfium_character_count, 55);
    }

    #[test]
    fn empty_or_garbled_layer_is_not_usable() {
        let empty = analyze_native_text(&NativeTextLayer::new(String::new(), 0));
        assert!(!empty.is_usable);

        let garbage = analyze_native_text(&NativeTextLayer::new(
            format!("{}{}{}{}{}", '\u{fffd}', '\u{0001}', 'a', 'b', 'c'),
            5,
        ));
        assert!(!garbage.is_usable);
        assert_eq!(garbage.replacement_characters, 1);
        assert_eq!(garbage.control_characters, 1);
    }

    #[test]
    fn sparse_cad_control_glyphs_do_not_discard_a_complete_layer() {
        let layer = NativeTextLayer::new(
            format!("Specification{} equipment and materials 42", '\u{0001}'),
            41,
        );
        let quality = analyze_native_text(&layer);
        assert!(quality.is_usable);
        assert_eq!(quality.control_characters, 1);
    }

    #[test]
    fn incomplete_unicode_extraction_is_not_declared_complete() {
        let layer = NativeTextLayer::new(
            "Readable fragment with enough printable characters".to_string(),
            200,
        );
        assert!(!analyze_native_text(&layer).is_usable);
    }

    #[test]
    fn rus_eng_gate_rejects_massive_unexpected_script_noise() {
        let text = format!("{} Project 42", "\u{ac00}".repeat(80));
        let quality = analyze_native_text_for_languages(
            &NativeTextLayer::new(text.clone(), text.chars().count()),
            "rus+eng",
        );

        assert!(!quality.is_usable);
        assert!(!quality.is_near_usable);
        assert!(quality.supported_script_ratio < 0.20);
        assert_eq!(quality.other_alphabetic_characters, 80);
    }

    #[test]
    fn a_few_technical_greek_symbols_do_not_reject_russian_english_text() {
        let text = "Кабель Alpha 42 Ω Δ φ specification".repeat(4);
        let quality = analyze_native_text_for_languages(
            &NativeTextLayer::new(text.clone(), text.chars().count()),
            "rus+eng",
        );

        assert!(quality.is_usable);
        assert!(quality.supported_script_ratio > 0.80);
    }

    #[test]
    fn rus_eng_gate_flags_dense_cp1251_text_without_forcing_lossy_ocr() {
        // CP1251 bytes for two Cyrillic words interpreted as Latin-1.
        let text = concat!(
            "\u{00ce}\u{00e1}\u{00f9}\u{00e8}\u{00e5} ",
            "\u{00e4}\u{00e0}\u{00ed}\u{00ed}\u{00fb}\u{00e5} 42"
        )
        .to_string();
        let quality = analyze_native_text_for_languages(
            &NativeTextLayer::new(text.clone(), text.chars().count()),
            "rus+eng",
        );

        assert_eq!(quality.suspected_mojibake_characters, 11);
        assert_eq!(quality.suspected_mojibake_runs, 2);
        assert_eq!(quality.longest_suspected_mojibake_run, 6);
        assert!(quality.has_strong_mojibake);
        // Detection is a quality-gate signal, not permission to discard a
        // complete searchable layer in favour of potentially lossy OCR.
        assert!(quality.is_usable);
        assert!(!quality.is_near_usable);
    }

    #[test]
    fn ordinary_english_and_isolated_accents_are_not_mojibake() {
        let text = "Schneider Electric, M\u{00fc}ller, Caf\u{00e9}; equipment model 42";
        let quality = analyze_native_text_for_languages(
            &NativeTextLayer::new(text.to_string(), text.chars().count()),
            "rus+eng",
        );

        assert_eq!(quality.suspected_mojibake_characters, 0);
        assert_eq!(quality.suspected_mojibake_runs, 0);
        assert!(!quality.has_strong_mojibake);
        assert!(quality.is_usable);
    }

    #[test]
    fn one_short_dense_accented_token_is_not_strong_evidence() {
        let text = "\u{00c0}\u{00c9}\u{00ce}\u{00d6}\u{00dc} engineering equipment 42";
        let quality = analyze_native_text_for_languages(
            &NativeTextLayer::new(text.to_string(), text.chars().count()),
            "rus+eng",
        );

        assert_eq!(quality.suspected_mojibake_characters, 5);
        assert_eq!(quality.suspected_mojibake_runs, 1);
        assert!(!quality.has_strong_mojibake);
        assert!(quality.is_usable);
    }

    #[test]
    fn rare_missing_mappings_are_near_usable_but_require_arbitration() {
        let mut text = "Проектная ведомость equipment 42 ".repeat(60);
        text.push_str(&"\u{fffd}".repeat(5));
        let quality = analyze_native_text_for_languages(
            &NativeTextLayer::new(text.clone(), text.chars().count() + 91),
            "rus+eng",
        );

        assert!(!quality.is_usable);
        assert!(quality.is_near_usable);
        assert!(quality.unicode_coverage >= 0.95);
        assert!(quality.replacement_ratio <= 0.005);
    }

    #[test]
    fn accepts_complete_text_after_control_run_normalization() {
        let text = "Readable engineering table text 42";
        let unicode_characters = text.chars().count();
        let layer = NativeTextLayer::new(
            text.to_string(),
            unicode_characters + unicode_characters / 3,
        );

        assert!(analyze_native_text(&layer).is_usable);
    }

    #[test]
    fn short_but_complete_searchable_label_is_preferred_to_noisy_ocr() {
        let text = "Drawing title 84".to_string();
        let quality =
            analyze_native_text(&NativeTextLayer::new(text.clone(), text.chars().count()));
        assert!(quality.is_usable);
    }

    #[test]
    fn sanitization_preserves_layout_whitespace_and_replaces_other_controls() {
        assert_eq!(sanitize_native_text("A\u{0001}B\nC\tD\rE"), "A B\nC\tD\rE");
    }

    #[test]
    fn layout_counts_repeated_coordinate_clusters() {
        let layout = NativeTextLayout::from_block_bounds([
            (10.0, 10.0, 30.0, 20.0),
            (40.0, 10.0, 60.0, 20.0),
            (70.0, 10.0, 90.0, 20.0),
            (10.0, 30.0, 30.0, 40.0),
            (40.0, 30.0, 60.0, 40.0),
            (70.0, 30.0, 90.0, 40.0),
            (10.0, 50.0, 30.0, 60.0),
            (40.0, 50.0, 60.0, 60.0),
            (70.0, 50.0, 90.0, 60.0),
        ]);

        assert_eq!(layout.text_block_count, 9);
        assert!(layout.x_alignment_clusters >= 2);
        assert!(layout.y_alignment_clusters >= 2);
        assert!(layout.has_coordinate_clusters());
    }
}
