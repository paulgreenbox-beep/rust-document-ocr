//! Opt-in, page-scoped Simplified Chinese routing. Detection inspects an
//! existing native PDF text layer; it never adds an OCR probe to a page.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::ocr::OcrLanguageProfile;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum LanguageRouting {
    /// Keep the caller's configured languages (normally rus+eng).
    #[default]
    Configured,
    /// Use chi_sim+eng only when reliable native-text evidence is present.
    AutoChinese,
    /// Explicitly select chi_sim+eng for every page, including scanned pages.
    SimplifiedChinese,
}

/// The default path returns immediately without scanning the text. An explicit
/// chi_sim language selection is routed as Chinese so page and cell policies
/// consistently preserve Han text and include the English technical model.
pub fn select_language_profile(
    policy: LanguageRouting,
    configured_languages: &str,
    native_text: Option<&str>,
) -> OcrLanguageProfile {
    if configured_languages
        .split('+')
        .any(|part| part.trim() == "chi_sim")
    {
        return OcrLanguageProfile::SimplifiedChinese;
    }
    match policy {
        LanguageRouting::Configured => OcrLanguageProfile::Configured,
        LanguageRouting::SimplifiedChinese => OcrLanguageProfile::SimplifiedChinese,
        LanguageRouting::AutoChinese => {
            if native_text.is_some_and(has_reliable_simplified_chinese) {
                OcrLanguageProfile::SimplifiedChinese
            } else {
                OcrLanguageProfile::Configured
            }
        }
    }
}

/// Language string recorded for a selected page, not a global configuration
/// mutation. Chinese recognition always retains English technical text.
pub fn effective_languages(configured: &str, profile: OcrLanguageProfile) -> String {
    match profile {
        OcrLanguageProfile::SimplifiedChinese => "chi_sim+eng".to_string(),
        OcrLanguageProfile::EnglishOnly | OcrLanguageProfile::EnglishOnlyRequired => {
            "eng".to_string()
        }
        OcrLanguageProfile::RussianOnly => "rus".to_string(),
        OcrLanguageProfile::Configured => configured.to_string(),
    }
}

pub fn is_han(character: char) -> bool {
    matches!(character as u32,
        0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff |
        0x20000..=0x2fa1f | 0x30000..=0x323af)
}

pub fn contains_han(text: &str) -> bool {
    text.chars().any(is_han)
}

/// Conservative evidence suited to technical PDFs: at least six Han
/// characters, three distinct Han characters, a run of three, and a common
/// Simplified-Chinese-specific character. Kana, Hangul, characteristic
/// traditional forms and damaged encoding veto routing. This is deliberately
/// not a universal language classifier: common-only Han and raster-only pages
/// remain on the configured route and need explicit selection.
pub fn has_reliable_simplified_chinese(text: &str) -> bool {
    const SIMPLIFIED_SIGNAL: &str = "线缆桥单电设备柜图规号数备连驱动轴带开关输盘总长压项阀护软钢铜铝丝径称类组装墙网层弯头实过热绝缘经点处来为这从应与间门马风进选扩断时产测验标准侧号仪钉";
    const TRADITIONAL_VETO: &str = "纜線橋單電設備櫃圖規號數連驅動軸帶開關輸盤總長壓項閥護軟鋼銅鋁絲徑稱類組裝牆網層彎頭實過熱絕緣經點處來為這從應與間門馬風進選擴斷時產測驗標準側儀釘";
    let mut han_count = 0;
    let mut distinct = std::collections::BTreeSet::new();
    let mut current_run = 0;
    let mut longest_run = 0;
    let mut simplified = false;
    for character in text.chars() {
        let code = character as u32;
        if character == '\u{fffd}'
            || matches!(code, 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd)
            || matches!(code, 0x3040..=0x30ff | 0x31f0..=0x31ff | 0xff66..=0xff9d)
            || matches!(code, 0x1100..=0x11ff | 0x3130..=0x318f | 0xac00..=0xd7af)
            || TRADITIONAL_VETO.contains(character)
        {
            return false;
        }
        if is_han(character) {
            han_count += 1;
            distinct.insert(character);
            current_run += 1;
            longest_run = longest_run.max(current_run);
            simplified |= SIMPLIFIED_SIGNAL.contains(character);
        } else {
            current_run = 0;
        }
    }
    han_count >= 6 && distinct.len() >= 3 && longest_run >= 3 && simplified
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_route_does_not_change_with_chinese_text() {
        assert_eq!(
            select_language_profile(LanguageRouting::Configured, "rus+eng", Some("线缆桥架清单")),
            OcrLanguageProfile::Configured
        );
        assert_eq!(
            effective_languages("rus+eng", OcrLanguageProfile::Configured),
            "rus+eng"
        );
    }

    #[test]
    fn explicit_chinese_routes_scan_without_native_text_with_english() {
        for (policy, configured) in [
            (LanguageRouting::SimplifiedChinese, "rus+eng"),
            (LanguageRouting::Configured, "chi_sim"),
            (LanguageRouting::Configured, "chi_sim+eng"),
        ] {
            let profile = select_language_profile(policy, configured, None);
            assert_eq!(profile, OcrLanguageProfile::SimplifiedChinese);
            assert_eq!(effective_languages(configured, profile), "chi_sim+eng");
        }
    }

    #[test]
    fn auto_accepts_realistic_simplified_technical_headings_and_mixed_text() {
        for text in [
            "线缆桥架清单",
            "电缆型号 数量 规格 VVR3*25+2*10 RVV18*1.0 M16X200",
        ] {
            assert!(has_reliable_simplified_chinese(text), "{text}");
            assert_eq!(
                select_language_profile(LanguageRouting::AutoChinese, "rus+eng", Some(text)),
                OcrLanguageProfile::SimplifiedChinese
            );
        }
    }

    #[test]
    fn auto_rejects_insufficient_ambiguous_and_damaged_evidence() {
        for text in [
            "Cable list VVR3*25+2*10",
            "Кабель силовой ВВГнг 3х2,5",
            "电缆",
            "电电电电电电",
            "",
            "控制工程工作表",
            "線纜橋架清單",
            "电缆型号 数量 规格 カタログ",
            "电缆型号 数量 한국어",
            "电缆型号 数量 规格\u{fffd}",
            "电缆型号 数量 规格\u{e012}",
        ] {
            assert!(!has_reliable_simplified_chinese(text), "{text}");
            assert_eq!(
                select_language_profile(LanguageRouting::AutoChinese, "rus+eng", Some(text)),
                OcrLanguageProfile::Configured
            );
        }
        assert_eq!(
            select_language_profile(LanguageRouting::AutoChinese, "rus+eng", None),
            OcrLanguageProfile::Configured
        );
    }

    #[test]
    fn han_predicate_preserves_mixed_table_cells() {
        assert!(contains_han("控制电缆 RVV18*1.0"));
        assert!(!contains_han("VVR3*25+2*10 / M16X200"));
        assert!(!contains_han("Кабель"));
    }
}
