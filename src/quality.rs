//! Intrinsic page-quality diagnostics that never require reference text.
//!
//! CER/WER belong to the offline regression scorer because production cannot
//! know the ground truth. This module exposes only local, explainable signals
//! and never rewrites recognized document content.

use serde::{Deserialize, Serialize};

use crate::page_analysis::{
    expected_scripts, is_cyrillic, is_latin, latin1_cyrillic_mojibake_evidence, NativeTextLayout,
    NativeTextQuality, ProcessingMode,
};

pub const PAGE_QUALITY_PROFILE: &str = "engineering_ru_en_v1";
pub const PAGE_QUALITY_PROFILE_VERSION: u32 = 1;
const UNRELIABLE_OCR_CONFIDENCE: f32 = 0.60;
const REVIEW_OCR_CONFIDENCE: f32 = 0.85;
const UNEXPECTED_SCRIPT_RATIO: f32 = 0.20;
const MIN_SCRIPT_LETTERS: usize = 32;
const MIN_RELIABLE_NON_WHITESPACE: usize = 24;
const MIN_COLLAPSED_TEXT_CHARACTERS: usize = 160;
const MIN_LAYOUT_ROWS_FOR_COLLAPSE: usize = 3;
const MIN_LINE_STRUCTURE_RATIO: f32 = 0.20;
const MIN_FRAGMENTED_RUN: usize = 4;
const MIN_FRAGMENTED_WORD_RATIO: f32 = 0.20;
const SEVERE_FRAGMENTED_CHARACTERS: usize = 32;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PageQualityStatus {
    #[default]
    Reliable,
    NeedsReview,
    Unreliable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageQualityIssue {
    pub code: String,
    pub severity: PageQualityStatus,
    pub observed: f32,
    pub threshold: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PageQualitySignals {
    pub character_count: usize,
    pub word_count: usize,
    pub nonempty_line_count: usize,
    pub cyrillic_characters: usize,
    pub latin_characters: usize,
    pub other_alphabetic_characters: usize,
    pub unexpected_script_ratio: f32,
    pub replacement_characters: usize,
    pub control_characters: usize,
    pub private_use_characters: usize,
    #[serde(default)]
    pub suspected_mojibake_characters: usize,
    #[serde(default)]
    pub suspected_mojibake_runs: usize,
    #[serde(default)]
    pub longest_suspected_mojibake_run: usize,
    #[serde(default)]
    pub suspected_mojibake_ratio: f32,
    #[serde(default)]
    pub has_strong_mojibake: bool,
    pub fragmented_letter_runs: usize,
    pub fragmented_letter_characters: usize,
    pub longest_fragmented_run: usize,
    pub mixed_script_word_ratio: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_structure_ratio: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageQualityReport {
    pub profile: String,
    pub profile_version: u32,
    pub status: PageQualityStatus,
    pub score: f32,
    pub signals: PageQualitySignals,
    pub issues: Vec<PageQualityIssue>,
}

pub fn assess_page_quality(
    text: &str,
    languages: &str,
    processing_mode: ProcessingMode,
    ocr_confidence: Option<f32>,
    native_quality: Option<&NativeTextQuality>,
    native_layout: Option<&NativeTextLayout>,
) -> PageQualityReport {
    let uses_native_text = matches!(
        processing_mode,
        ProcessingMode::NativeText | ProcessingMode::NativeTextWithGrid
    );
    // Layout extracted from a rejected native layer must not grade the
    // canonical OCR result.
    let effective_native_layout = if uses_native_text {
        native_layout
    } else {
        None
    };
    let mut signals = collect_signals(text, languages, effective_native_layout);
    if uses_native_text {
        if let Some(native) = native_quality {
            signals.replacement_characters = native.replacement_characters;
            signals.control_characters = native.control_characters;
            signals.suspected_mojibake_characters = native.suspected_mojibake_characters;
            signals.suspected_mojibake_runs = native.suspected_mojibake_runs;
            signals.longest_suspected_mojibake_run = native.longest_suspected_mojibake_run;
            signals.suspected_mojibake_ratio = native.suspected_mojibake_ratio;
            signals.has_strong_mojibake = native.has_strong_mojibake;
        }
    }

    let mut issues = Vec::new();
    let alphabetic = signals.cyrillic_characters
        + signals.latin_characters
        + signals.other_alphabetic_characters;
    if alphabetic >= MIN_SCRIPT_LETTERS
        && signals.unexpected_script_ratio >= UNEXPECTED_SCRIPT_RATIO
    {
        issues.push(issue(
            "unexpected_script_ratio",
            PageQualityStatus::Unreliable,
            signals.unexpected_script_ratio,
            UNEXPECTED_SCRIPT_RATIO,
        ));
    }
    let non_whitespace_characters = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .count();
    if non_whitespace_characters < 8 {
        issues.push(issue(
            "insufficient_text",
            PageQualityStatus::Unreliable,
            non_whitespace_characters as f32,
            8.0,
        ));
    } else if non_whitespace_characters < MIN_RELIABLE_NON_WHITESPACE {
        issues.push(issue(
            "short_text",
            PageQualityStatus::NeedsReview,
            non_whitespace_characters as f32,
            MIN_RELIABLE_NON_WHITESPACE as f32,
        ));
    }
    if signals.replacement_characters > 0 {
        issues.push(issue(
            "replacement_characters",
            PageQualityStatus::NeedsReview,
            signals.replacement_characters as f32,
            0.0,
        ));
    }
    if signals.control_characters > 0 {
        issues.push(issue(
            "control_characters",
            PageQualityStatus::NeedsReview,
            signals.control_characters as f32,
            0.0,
        ));
    }
    if signals.private_use_characters > 0 {
        issues.push(issue(
            "private_use_characters",
            PageQualityStatus::NeedsReview,
            signals.private_use_characters as f32,
            0.0,
        ));
    }
    let (allow_cyrillic, _, enforce_script) = expected_scripts(languages);
    if uses_native_text && enforce_script && allow_cyrillic && signals.has_strong_mojibake {
        issues.push(issue(
            "latin1_cyrillic_mojibake",
            PageQualityStatus::Unreliable,
            signals.suspected_mojibake_ratio,
            0.20,
        ));
    }
    if matches!(
        processing_mode,
        ProcessingMode::Ocr | ProcessingMode::OcrWithGrid
    ) {
        if ocr_confidence.is_some_and(|confidence| confidence < UNRELIABLE_OCR_CONFIDENCE) {
            issues.push(issue(
                "low_ocr_confidence",
                PageQualityStatus::Unreliable,
                ocr_confidence.unwrap_or_default(),
                UNRELIABLE_OCR_CONFIDENCE,
            ));
        } else if ocr_confidence.is_some_and(|confidence| confidence < REVIEW_OCR_CONFIDENCE) {
            issues.push(issue(
                "low_ocr_confidence",
                PageQualityStatus::NeedsReview,
                ocr_confidence.unwrap_or_default(),
                REVIEW_OCR_CONFIDENCE,
            ));
        }
    }
    if signals.longest_fragmented_run >= MIN_FRAGMENTED_RUN
        && signals.word_count > 0
        && (signals.fragmented_letter_characters >= SEVERE_FRAGMENTED_CHARACTERS
            || signals.fragmented_letter_characters as f32 / signals.word_count as f32
                >= MIN_FRAGMENTED_WORD_RATIO)
    {
        issues.push(issue(
            "fragmented_letter_runs",
            PageQualityStatus::NeedsReview,
            signals.longest_fragmented_run as f32,
            MIN_FRAGMENTED_RUN as f32,
        ));
    }
    let line_structure_is_collapsed = effective_native_layout.is_some_and(|layout| {
        layout.y_alignment_clusters >= MIN_LAYOUT_ROWS_FOR_COLLAPSE
            && (signals.nonempty_line_count <= 1
                || signals
                    .line_structure_ratio
                    .is_some_and(|ratio| ratio < MIN_LINE_STRUCTURE_RATIO))
    });
    if signals.character_count >= MIN_COLLAPSED_TEXT_CHARACTERS && line_structure_is_collapsed {
        issues.push(issue(
            "collapsed_line_structure",
            PageQualityStatus::NeedsReview,
            signals.line_structure_ratio.unwrap_or_default(),
            MIN_LINE_STRUCTURE_RATIO,
        ));
    }

    let status = issues
        .iter()
        .map(|issue| issue.severity)
        .max_by_key(|status| match status {
            PageQualityStatus::Reliable => 0,
            PageQualityStatus::NeedsReview => 1,
            PageQualityStatus::Unreliable => 2,
        })
        .unwrap_or(PageQualityStatus::Reliable);
    let score = match status {
        PageQualityStatus::Reliable => 1.0,
        PageQualityStatus::NeedsReview => 0.65,
        PageQualityStatus::Unreliable => 0.20,
    };
    PageQualityReport {
        profile: PAGE_QUALITY_PROFILE.to_string(),
        profile_version: PAGE_QUALITY_PROFILE_VERSION,
        status,
        score,
        signals,
        issues,
    }
}

fn issue(
    code: &str,
    severity: PageQualityStatus,
    observed: f32,
    threshold: f32,
) -> PageQualityIssue {
    PageQualityIssue {
        code: code.to_string(),
        severity,
        observed,
        threshold,
    }
}

fn collect_signals(
    text: &str,
    languages: &str,
    native_layout: Option<&NativeTextLayout>,
) -> PageQualitySignals {
    let mut signals = PageQualitySignals {
        character_count: text.chars().count(),
        nonempty_line_count: text.lines().filter(|line| !line.trim().is_empty()).count(),
        ..PageQualitySignals::default()
    };
    for character in text.chars() {
        if is_cyrillic(character) {
            signals.cyrillic_characters += 1;
        } else if is_latin(character) {
            signals.latin_characters += 1;
        } else if character.is_alphabetic() {
            signals.other_alphabetic_characters += 1;
        }
        if character.is_control() && !matches!(character, '\n' | '\r' | '\t') {
            signals.control_characters += 1;
        }
        if character == '\u{fffd}' {
            signals.replacement_characters += 1;
        }
        if is_private_use(character) {
            signals.private_use_characters += 1;
        }
    }
    let mojibake = latin1_cyrillic_mojibake_evidence(text);
    signals.suspected_mojibake_characters = mojibake.characters;
    signals.suspected_mojibake_runs = mojibake.runs;
    signals.longest_suspected_mojibake_run = mojibake.longest_run;
    signals.suspected_mojibake_ratio = mojibake.alphabetic_ratio;
    signals.has_strong_mojibake = mojibake.is_strong;

    let words = text.split_whitespace().collect::<Vec<_>>();
    signals.word_count = words.len();
    let (allow_cyrillic, allow_latin, enforce_script) = expected_scripts(languages);
    let han_characters = if languages
        .split('+')
        .any(|language| language.trim() == "chi_sim")
    {
        text.chars()
            .filter(|character| crate::language::is_han(*character))
            .count()
    } else {
        0
    };
    let supported = usize::from(allow_cyrillic) * signals.cyrillic_characters
        + usize::from(allow_latin) * signals.latin_characters
        + han_characters;
    let alphabetic = signals.cyrillic_characters
        + signals.latin_characters
        + signals.other_alphabetic_characters;
    signals.unexpected_script_ratio = if alphabetic == 0 || !enforce_script {
        0.0
    } else {
        1.0 - supported as f32 / alphabetic as f32
    };

    let mut mixed_script_words = 0usize;
    for word in &words {
        let has_cyrillic = word.chars().any(is_cyrillic);
        let has_latin = word.chars().any(is_latin);
        mixed_script_words += usize::from(has_cyrillic && has_latin);
    }
    collect_fragmentation_signals(text, &mut signals);
    signals.mixed_script_word_ratio = if words.is_empty() {
        0.0
    } else {
        mixed_script_words as f32 / words.len() as f32
    };
    signals.line_structure_ratio = native_layout
        .filter(|layout| layout.y_alignment_clusters > 0)
        .map(|layout| {
            (signals.nonempty_line_count as f32 / layout.y_alignment_clusters as f32)
                .clamp(0.0, 1.0)
        });
    signals
}

fn collect_fragmentation_signals(text: &str, signals: &mut PageQualitySignals) {
    for line in text.lines() {
        let mut current_run = 0usize;
        let mut current_script = None;
        for word in line.split_whitespace() {
            let script = single_supported_letter_script(word);
            if script.is_some() && (current_run == 0 || script == current_script) {
                current_run += 1;
                current_script = script;
                signals.longest_fragmented_run = signals.longest_fragmented_run.max(current_run);
            } else {
                finish_fragmented_run(current_run, signals);
                current_run = usize::from(script.is_some());
                current_script = script;
                signals.longest_fragmented_run = signals.longest_fragmented_run.max(current_run);
            }
        }
        finish_fragmented_run(current_run, signals);
    }
}

fn single_supported_letter_script(word: &str) -> Option<u8> {
    let mut characters = word.chars();
    let character = characters.next()?;
    if characters.next().is_some() {
        return None;
    }
    if is_cyrillic(character) {
        Some(1)
    } else if is_latin(character) {
        Some(2)
    } else {
        None
    }
}

fn finish_fragmented_run(run: usize, signals: &mut PageQualitySignals) {
    if run >= MIN_FRAGMENTED_RUN {
        signals.fragmented_letter_runs += 1;
        signals.fragmented_letter_characters += run;
    }
}

fn is_private_use(character: char) -> bool {
    matches!(
        character as u32,
        0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd
    )
}

#[cfg(test)]
mod tests {
    use super::{assess_page_quality, PageQualityStatus};
    use crate::page_analysis::{NativeTextLayout, NativeTextQuality, ProcessingMode};

    #[test]
    fn reordered_or_collapsed_native_layout_requires_review() {
        let layout = NativeTextLayout {
            text_block_count: 20,
            x_alignment_clusters: 4,
            y_alignment_clusters: 12,
        };
        let report = assess_page_quality(
            &"Проектная строка с данными 42 ".repeat(12),
            "rus+eng",
            ProcessingMode::NativeText,
            None,
            None,
            Some(&layout),
        );

        assert_eq!(report.status, PageQualityStatus::NeedsReview);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "collapsed_line_structure"));
    }

    #[test]
    fn low_confidence_ocr_is_reported_without_replacing_text() {
        let text = "Распознанный текст 42";
        let report =
            assess_page_quality(text, "rus+eng", ProcessingMode::Ocr, Some(0.62), None, None);

        assert_eq!(report.status, PageQualityStatus::NeedsReview);
        assert_eq!(report.signals.character_count, text.chars().count());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "low_ocr_confidence"));
    }

    #[test]
    fn fragmented_and_unexpected_scripts_are_intrinsic_signals() {
        let text = format!("{} M e a n W e l l", "\u{ac00}".repeat(64));
        let report = assess_page_quality(
            &text,
            "rus+eng",
            ProcessingMode::NativeTextWithGrid,
            None,
            None,
            None,
        );

        assert_eq!(report.status, PageQualityStatus::Unreliable);
        assert!(report.signals.unexpected_script_ratio > 0.80);
        assert!(report.signals.longest_fragmented_run >= 8);
    }

    #[test]
    fn rejected_native_metrics_do_not_contaminate_canonical_ocr_quality() {
        let rejected_native = NativeTextQuality {
            replacement_characters: 12,
            control_characters: 7,
            ..NativeTextQuality::default()
        };
        let report = assess_page_quality(
            "Clean OCR result with enough text 42",
            "rus+eng",
            ProcessingMode::Ocr,
            Some(0.95),
            Some(&rejected_native),
            None,
        );

        assert_eq!(report.signals.replacement_characters, 0);
        assert_eq!(report.signals.control_characters, 0);
        assert_eq!(report.status, PageQualityStatus::Reliable);
    }

    #[test]
    fn dense_cp1251_as_latin1_native_text_is_unreliable() {
        let text = concat!(
            "\u{00ce}\u{00e1}\u{00f9}\u{00e8}\u{00e5} ",
            "\u{00e4}\u{00e0}\u{00ed}\u{00ed}\u{00fb}\u{00e5} 42"
        );
        let report = assess_page_quality(
            text,
            "rus+eng",
            ProcessingMode::NativeText,
            None,
            None,
            None,
        );

        assert_eq!(report.status, PageQualityStatus::Unreliable);
        assert_eq!(report.signals.suspected_mojibake_characters, 11);
        assert!(report.signals.has_strong_mojibake);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "latin1_cyrillic_mojibake"));
    }

    #[test]
    fn isolated_accents_in_manufacturer_names_remain_reliable() {
        let report = assess_page_quality(
            "Schneider Electric, M\u{00fc}ller and Caf\u{00e9} equipment model 42",
            "rus+eng",
            ProcessingMode::NativeText,
            None,
            None,
            None,
        );

        assert_eq!(report.status, PageQualityStatus::Reliable);
        assert!(!report.signals.has_strong_mojibake);
        assert!(!report
            .issues
            .iter()
            .any(|issue| issue.code == "latin1_cyrillic_mojibake"));
    }

    #[test]
    fn rejected_native_layout_does_not_grade_canonical_ocr_text() {
        let rejected_layout = NativeTextLayout {
            text_block_count: 40,
            x_alignment_clusters: 8,
            y_alignment_clusters: 24,
        };
        let text = "Clean OCR engineering result with stable words and values 42 ".repeat(8);
        let report = assess_page_quality(
            &text,
            "rus+eng",
            ProcessingMode::Ocr,
            Some(0.95),
            None,
            Some(&rejected_layout),
        );

        assert_eq!(report.status, PageQualityStatus::Reliable);
        assert!(report.signals.line_structure_ratio.is_none());
        assert!(!report
            .issues
            .iter()
            .any(|issue| issue.code == "collapsed_line_structure"));
    }

    #[test]
    fn canonical_unicode_artifacts_require_review() {
        let text = "Readable native text 42 \u{fffd}\u{e000}\u{0001}";
        let native = NativeTextQuality {
            replacement_characters: 1,
            control_characters: 1,
            ..NativeTextQuality::default()
        };
        let report = assess_page_quality(
            text,
            "rus+eng",
            ProcessingMode::NativeText,
            None,
            Some(&native),
            None,
        );

        assert_eq!(report.status, PageQualityStatus::NeedsReview);
        assert_eq!(report.signals.replacement_characters, 1);
        assert_eq!(report.signals.control_characters, 1);
        assert_eq!(report.signals.private_use_characters, 1);
        for code in [
            "replacement_characters",
            "control_characters",
            "private_use_characters",
        ] {
            assert!(report.issues.iter().any(|issue| issue.code == code));
        }
    }

    #[test]
    fn low_line_structure_ratio_requires_review_but_half_coverage_is_reliable() {
        let collapsed_layout = NativeTextLayout {
            text_block_count: 24,
            x_alignment_clusters: 4,
            y_alignment_clusters: 20,
        };
        let collapsed = assess_page_quality(
            &(0..2)
                .map(|_| "Проектная строка с большим количеством данных 42 ".repeat(3))
                .collect::<Vec<_>>()
                .join("\n"),
            "rus+eng",
            ProcessingMode::NativeText,
            None,
            None,
            Some(&collapsed_layout),
        );
        assert!(collapsed
            .issues
            .iter()
            .any(|issue| issue.code == "collapsed_line_structure"));

        let adequate_layout = NativeTextLayout {
            text_block_count: 10,
            x_alignment_clusters: 3,
            y_alignment_clusters: 10,
        };
        let adequate = assess_page_quality(
            &(0..5)
                .map(|_| "Проектная строка с большим количеством данных 42")
                .collect::<Vec<_>>()
                .join("\n"),
            "rus+eng",
            ProcessingMode::NativeText,
            None,
            None,
            Some(&adequate_layout),
        );
        assert!(!adequate
            .issues
            .iter()
            .any(|issue| issue.code == "collapsed_line_structure"));
    }

    #[test]
    fn ocr_confidence_has_review_and_unreliable_bands() {
        let status = |confidence| {
            assess_page_quality(
                "Распознанный текст страницы с достаточным объёмом 42",
                "rus+eng",
                ProcessingMode::Ocr,
                Some(confidence),
                None,
                None,
            )
            .status
        };
        assert_eq!(status(0.59), PageQualityStatus::Unreliable);
        assert_eq!(status(0.60), PageQualityStatus::NeedsReview);
        assert_eq!(status(0.84), PageQualityStatus::NeedsReview);
        assert_eq!(status(0.85), PageQualityStatus::Reliable);
    }

    #[test]
    fn short_text_is_reviewed_without_reclassifying_empty_text() {
        let status = |length| {
            assess_page_quality(
                &"A".repeat(length),
                "rus+eng",
                ProcessingMode::NativeText,
                None,
                None,
                None,
            )
            .status
        };
        assert_eq!(status(7), PageQualityStatus::Unreliable);
        assert_eq!(status(8), PageQualityStatus::NeedsReview);
        assert_eq!(status(23), PageQualityStatus::NeedsReview);
        assert_eq!(status(24), PageQualityStatus::Reliable);
    }

    #[test]
    fn fragmented_runs_do_not_cross_lines_punctuation_or_scripts() {
        for text in [
            "Normal engineering context 42 M e\na n more context",
            "Normal engineering context 42 M e, a n more context",
            "Normal engineering context 42 M е a n more context",
        ] {
            let report = assess_page_quality(
                text,
                "rus+eng",
                ProcessingMode::NativeText,
                None,
                None,
                None,
            );
            assert_eq!(report.signals.fragmented_letter_runs, 0);
        }
    }

    #[test]
    fn severe_same_line_fragmentation_requires_review_even_at_low_ratio() {
        let fragmented = (0..32).map(|_| "A").collect::<Vec<_>>().join(" ");
        let text = format!("{} {}", fragmented, "normal ".repeat(200));
        let report = assess_page_quality(
            &text,
            "rus+eng",
            ProcessingMode::NativeText,
            None,
            None,
            None,
        );
        assert_eq!(report.status, PageQualityStatus::NeedsReview);
        assert_eq!(report.signals.fragmented_letter_characters, 32);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "fragmented_letter_runs"));

        let initials = assess_page_quality(
            "Technical initials A B C remain ordinary project data 42",
            "rus+eng",
            ProcessingMode::NativeText,
            None,
            None,
            None,
        );
        assert_eq!(initials.status, PageQualityStatus::Reliable);
    }
}
