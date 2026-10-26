//! Deterministic, local semantic classification for reconstructed tables.
//!
//! This module never rewrites OCR text or changes a table's structural kind.
//! It derives an additive, probabilistic projection from headers, column value
//! patterns, geometry, OCR confidence, and detector confidence.

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::dto::{TableDetectorType, TableMatrix, TableType};

/// Probabilistic subject-domain type, independent from structural `kind`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SemanticTableType {
    MaterialSpecification,
    EquipmentSchedule,
    CableJournal,
    WorkEstimate,
    CommercialOffer,
    DrawingTitleBlock,
    GenericTable,
    #[default]
    #[serde(other)]
    Unknown,
}

/// A semantic role inferred for one physical table column.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ColumnRole {
    Position,
    Code,
    Name,
    Designation,
    Manufacturer,
    Unit,
    Quantity,
    Length,
    Price,
    Amount,
    Vat,
    Currency,
    LaborHours,
    CableMark,
    Conductors,
    CrossSection,
    From,
    To,
    #[default]
    #[serde(other)]
    Unknown,
}

/// Why one semantic score changed. Evidence contains stable rule identifiers,
/// not user document text.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    StructuralKind,
    DetectorConfidence,
    OcrConfidence,
    Geometry,
    HeaderAlias,
    ColumnPattern,
    SemanticRule,
}

/// One bounded contribution to semantic classification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassificationEvidence {
    pub kind: EvidenceKind,
    pub rule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    pub score: f32,
}

/// Role assigned to a zero-based physical column. Low-confidence columns are
/// retained as `unknown` instead of being guessed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnRoleAssignment {
    pub column: usize,
    pub role: ColumnRole,
    pub confidence: f32,
}

/// Complete additive semantic projection for one table.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TableClassification {
    pub semantic_type: SemanticTableType,
    pub semantic_confidence: f32,
    pub header_rows: Vec<usize>,
    pub column_roles: Vec<ColumnRoleAssignment>,
    pub classification_evidence: Vec<ClassificationEvidence>,
}

const ROLE_CONFIDENCE_THRESHOLD: f32 = 0.60;
const SPECIFIC_TYPE_THRESHOLD: f32 = 0.66;
const SPECIFIC_TYPE_MARGIN: f32 = 0.055;

const ALL_ROLES: &[ColumnRole] = &[
    ColumnRole::Position,
    ColumnRole::Code,
    ColumnRole::Name,
    ColumnRole::Designation,
    ColumnRole::Manufacturer,
    ColumnRole::Unit,
    ColumnRole::Quantity,
    ColumnRole::Length,
    ColumnRole::Price,
    ColumnRole::Amount,
    ColumnRole::Vat,
    ColumnRole::Currency,
    ColumnRole::LaborHours,
    ColumnRole::CableMark,
    ColumnRole::Conductors,
    ColumnRole::CrossSection,
    ColumnRole::From,
    ColumnRole::To,
];

/// Classify a filled table without mutating its structural representation.
///
/// `page_size` must use the same pixel coordinate space as `matrix.bbox`.
/// When unavailable, classification simply omits page-relative evidence.
#[expect(
    clippy::useless_vec,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
pub fn classify_table(matrix: &TableMatrix, page_size: Option<(u32, u32)>) -> TableClassification {
    let row_count = matrix.rows.len();
    let column_count = matrix.rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut evidence = Vec::new();

    let detector_score = finite_unit(matrix.confidence);
    let ocr_score = mean_populated_cell_confidence(matrix);
    evidence.push(ClassificationEvidence {
        kind: EvidenceKind::DetectorConfidence,
        rule: match matrix.detector_type {
            TableDetectorType::Grid => "detector:ruled_grid",
            TableDetectorType::Borderless => "detector:borderless_alignment",
            TableDetectorType::WordAlignmentFallback => "detector:legacy_word_alignment",
        }
        .to_string(),
        row: None,
        column: None,
        score: detector_score,
    });
    evidence.push(ClassificationEvidence {
        kind: EvidenceKind::OcrConfidence,
        rule: "ocr:mean_populated_cell_confidence".to_string(),
        row: None,
        column: None,
        score: ocr_score,
    });
    evidence.push(ClassificationEvidence {
        kind: EvidenceKind::Geometry,
        rule: format!("geometry:{row_count}x{column_count}"),
        row: None,
        column: None,
        score: geometry_score(row_count, column_count),
    });

    if matrix.table_type == TableType::TitleBlock {
        evidence.push(ClassificationEvidence {
            kind: EvidenceKind::StructuralKind,
            rule: "structural_kind:title_block".to_string(),
            row: None,
            column: None,
            score: 1.0,
        });
        if let (Some(bbox), Some((page_width, page_height))) = (&matrix.bbox, page_size) {
            let location = lower_right_score(bbox, page_width, page_height);
            evidence.push(ClassificationEvidence {
                kind: EvidenceKind::Geometry,
                rule: "location:lower_right_drawing_frame".to_string(),
                row: None,
                column: None,
                score: location,
            });
        }
        return TableClassification {
            semantic_type: SemanticTableType::DrawingTitleBlock,
            semantic_confidence: (0.86 + detector_score * 0.08 + ocr_score * 0.04).clamp(0.0, 0.98),
            header_rows: Vec::new(),
            column_roles: unknown_columns(column_count),
            classification_evidence: evidence,
        };
    }

    if row_count == 0 || column_count == 0 {
        return TableClassification {
            semantic_type: SemanticTableType::Unknown,
            semantic_confidence: 0.0,
            header_rows: Vec::new(),
            column_roles: unknown_columns(column_count),
            classification_evidence: evidence,
        };
    }

    let header_scan_rows = row_count.min(3);
    let mut header_matches = vec![Vec::<RoleScore>::new(); column_count];
    let mut header_rows = Vec::new();
    for row in 0..header_scan_rows {
        let mut matched_columns = 0;
        let mut best_score = 0.0_f32;
        for (column, matches) in header_matches.iter_mut().enumerate() {
            let text = matrix
                .rows
                .get(row)
                .and_then(|cells| cells.get(column))
                .map(|cell| cell.text.as_str())
                .unwrap_or("");
            let role = best_header_role(text);
            best_score = best_score.max(role.score);
            if role.score >= ROLE_CONFIDENCE_THRESHOLD {
                matched_columns += 1;
                matches.push(RoleScore {
                    role: role.role,
                    score: role.score,
                    row: Some(row),
                });
            }
        }
        if matched_columns >= 2 || (matched_columns == 1 && best_score >= 0.90 && column_count <= 2)
        {
            header_rows.push(row);
        }
    }

    // If a later scanned row clearly contains the column headings, earlier
    // one-cell title rows remain semantic hints but are not treated as headers.
    if header_rows.len() > 1 {
        let best = header_rows
            .iter()
            .copied()
            .max_by_key(|row| {
                header_matches
                    .iter()
                    .filter(|matches| matches.iter().any(|item| item.row == Some(*row)))
                    .count()
            })
            .unwrap_or(header_rows[0]);
        header_rows.retain(|row| *row == best);
    }

    let mut column_roles = Vec::with_capacity(column_count);
    for (column, matches) in header_matches.iter().enumerate() {
        let data_values = column_values(matrix, column, &header_rows);
        let mut best = RoleScore::unknown();
        for role in ALL_ROLES {
            let header = matches
                .iter()
                .filter(|candidate| candidate.role == *role)
                .map(|candidate| candidate.score)
                .fold(0.0_f32, f32::max);
            let pattern = data_role_score(*role, &data_values);
            let combined = if header > 0.0 {
                (header * 0.78 + pattern * 0.22).min(1.0)
            } else {
                (pattern * 0.82).min(1.0)
            };
            if combined > best.score {
                best = RoleScore {
                    role: *role,
                    score: combined,
                    row: matches
                        .iter()
                        .find(|candidate| candidate.role == *role)
                        .and_then(|candidate| candidate.row),
                };
            }
        }

        let (role, confidence) = if best.score >= ROLE_CONFIDENCE_THRESHOLD {
            (best.role, finite_unit(best.score))
        } else {
            (ColumnRole::Unknown, finite_unit(best.score))
        };
        column_roles.push(ColumnRoleAssignment {
            column,
            role,
            confidence,
        });
        if role != ColumnRole::Unknown {
            if let Some(row) = best.row {
                evidence.push(ClassificationEvidence {
                    kind: EvidenceKind::HeaderAlias,
                    rule: format!("column_role:{}", role_wire_name(role)),
                    row: Some(row),
                    column: Some(column),
                    score: confidence,
                });
            } else {
                evidence.push(ClassificationEvidence {
                    kind: EvidenceKind::ColumnPattern,
                    rule: format!("column_role:{}", role_wire_name(role)),
                    row: None,
                    column: Some(column),
                    score: confidence,
                });
            }
        }
    }

    let heading_text = (0..header_scan_rows)
        .flat_map(|row| {
            matrix
                .rows
                .get(row)
                .into_iter()
                .flat_map(|cells| cells.iter())
        })
        .map(|cell| cell.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let normalized_heading = normalize_ocr_header(&heading_text);
    let known_roles = column_roles
        .iter()
        .filter(|assignment| assignment.role != ColumnRole::Unknown)
        .count();
    let header_quality = if column_count == 0 {
        0.0
    } else {
        finite_unit(known_roles as f32 / column_count as f32)
    };
    let structural_quality =
        (geometry_score(row_count, column_count) * 0.45 + detector_score * 0.35 + ocr_score * 0.20)
            .clamp(0.0, 1.0);

    let mut candidates = vec![
        (
            SemanticTableType::CableJournal,
            cable_journal_score(&column_roles, &normalized_heading, structural_quality),
            "semantic:cable_journal",
        ),
        (
            SemanticTableType::CommercialOffer,
            commercial_offer_score(&column_roles, &normalized_heading, structural_quality),
            "semantic:commercial_offer",
        ),
        (
            SemanticTableType::WorkEstimate,
            work_estimate_score(&column_roles, &normalized_heading, structural_quality),
            "semantic:work_estimate",
        ),
        (
            SemanticTableType::EquipmentSchedule,
            equipment_schedule_score(&column_roles, &normalized_heading, structural_quality),
            "semantic:equipment_schedule",
        ),
        (
            SemanticTableType::MaterialSpecification,
            material_specification_score(&column_roles, &normalized_heading, structural_quality),
            "semantic:material_specification",
        ),
    ];
    candidates.sort_by(|left, right| right.1.total_cmp(&left.1));
    let best = candidates[0];
    let runner_up = candidates.get(1).map_or(0.0, |candidate| candidate.1);

    let (semantic_type, semantic_confidence, semantic_rule) =
        if best.1 >= SPECIFIC_TYPE_THRESHOLD && best.1 - runner_up >= SPECIFIC_TYPE_MARGIN {
            (best.0, finite_unit(best.1), best.2)
        } else if row_count >= 2
            && column_count >= 2
            && (known_roles >= 2
                || structural_quality >= 0.52
                || matrix.detector_type != TableDetectorType::WordAlignmentFallback)
        {
            let generic_confidence =
                (0.42 + structural_quality * 0.28 + header_quality * 0.20).clamp(0.45, 0.79);
            (
                SemanticTableType::GenericTable,
                generic_confidence,
                "semantic:generic_table",
            )
        } else {
            (
                SemanticTableType::Unknown,
                (structural_quality * 0.45).min(0.49),
                "semantic:unknown",
            )
        };

    evidence.push(ClassificationEvidence {
        kind: EvidenceKind::SemanticRule,
        rule: semantic_rule.to_string(),
        row: None,
        column: None,
        score: semantic_confidence,
    });

    TableClassification {
        semantic_type,
        semantic_confidence: finite_unit(semantic_confidence),
        header_rows,
        column_roles,
        classification_evidence: evidence,
    }
}

#[derive(Debug, Clone, Copy)]
struct RoleScore {
    role: ColumnRole,
    score: f32,
    row: Option<usize>,
}

impl RoleScore {
    const fn unknown() -> Self {
        Self {
            role: ColumnRole::Unknown,
            score: 0.0,
            row: None,
        }
    }
}

fn best_header_role(text: &str) -> RoleScore {
    if text.trim().is_empty() {
        return RoleScore::unknown();
    }
    let normalized = normalize_ocr_header(text);
    let mut best = RoleScore::unknown();
    for role in ALL_ROLES {
        for alias in role_aliases(*role) {
            let score = alias_score(&normalized, alias);
            if score > best.score {
                best = RoleScore {
                    role: *role,
                    score,
                    row: None,
                };
            }
        }
    }
    if text.contains('№') && 0.94 > best.score {
        best = RoleScore {
            role: ColumnRole::Position,
            score: 0.94,
            row: None,
        };
    }
    best
}

fn role_aliases(role: ColumnRole) -> &'static [&'static str] {
    match role {
        ColumnRole::Position => &["поз", "позиция", "номер", "item no", "no"],
        ColumnRole::Code => &[
            "код",
            "артикул",
            "каталожный номер",
            "catalog no",
            "part no",
            "sku",
        ],
        ColumnRole::Name => &[
            "наименование",
            "наименование оборудования",
            "оборудование",
            "описание",
            "наименование и техническая характеристика",
            "name",
            "description",
            "item name",
        ],
        ColumnRole::Designation => &[
            "обозначение",
            "тип марка",
            "тип",
            "designation",
            "model",
            "type",
        ],
        ColumnRole::Manufacturer => &[
            "производитель",
            "изготовитель",
            "завод изготовитель",
            "manufacturer",
            "vendor",
            "brand",
        ],
        ColumnRole::Unit => &["ед изм", "единица измерения", "единица", "unit", "uom"],
        ColumnRole::Quantity => &["кол во", "количество", "кол", "qty", "quantity"],
        ColumnRole::Length => &["длина", "протяженность", "length"],
        ColumnRole::Price => &[
            "цена",
            "цена за единицу",
            "стоимость единицы",
            "unit price",
            "price",
        ],
        ColumnRole::Amount => &["сумма", "стоимость", "всего", "итого", "amount", "total"],
        ColumnRole::Vat => &["ндс", "в т ч ндс", "vat"],
        ColumnRole::Currency => &["валюта", "currency"],
        ColumnRole::LaborHours => &[
            "чел ч",
            "трудозатраты",
            "затраты труда",
            "labor hours",
            "man hours",
        ],
        ColumnRole::CableMark => &[
            "марка кабеля",
            "тип кабеля",
            "кабель",
            "cable mark",
            "cable type",
        ],
        ColumnRole::Conductors => &["число жил", "количество жил", "жил", "conductors", "cores"],
        ColumnRole::CrossSection => &[
            "сечение",
            "сечение жил",
            "мм2",
            "cross section",
            "cross-section",
        ],
        ColumnRole::From => &["откуда", "начало", "источник", "from", "source"],
        ColumnRole::To => &[
            "куда",
            "конец",
            "приемник",
            "назначение",
            "to",
            "destination",
        ],
        ColumnRole::Unknown => &[],
    }
}

fn normalize_ocr_header(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    for character in value.to_lowercase().chars() {
        match character {
            'ё' => normalized.push('е'),
            '0' => normalized.push('о'),
            '1' | '|' => normalized.push('i'),
            '№' => normalized.push_str(" no "),
            value if value.is_alphanumeric() => normalized.push(value),
            _ => normalized.push(' '),
        }
    }
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized
        .replace("наименованне", "наименование")
        .replace("обозначенне", "обозначение")
        .replace("к ол во", "кол во")
        .replace("к ол", "кол")
        .replace("ед нзм", "ед изм")
}

fn alias_score(normalized: &str, alias: &str) -> f32 {
    if normalized == alias {
        return 1.0;
    }
    if normalized.contains(alias) {
        return 0.96;
    }
    let alias_tokens = alias.split_whitespace().collect::<Vec<_>>();
    let value_tokens = normalized.split_whitespace().collect::<Vec<_>>();
    if alias_tokens.is_empty() || value_tokens.is_empty() {
        return 0.0;
    }

    if alias_tokens.len() == 1 {
        return value_tokens
            .iter()
            .map(|token| string_similarity(token, alias_tokens[0]))
            .fold(0.0_f32, f32::max);
    }

    value_tokens
        .windows(alias_tokens.len())
        .map(|window| {
            window
                .iter()
                .zip(&alias_tokens)
                .map(|(left, right)| string_similarity(left, right))
                .sum::<f32>()
                / alias_tokens.len() as f32
        })
        .fold(0.0_f32, f32::max)
}

fn string_similarity(left: &str, right: &str) -> f32 {
    let max_len = left.chars().count().max(right.chars().count());
    if max_len == 0 {
        return 1.0;
    }
    let distance = edit_distance(left, right);
    (1.0 - distance as f32 / max_len as f32).clamp(0.0, 1.0)
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (left_index, left_character) in left.chars().enumerate() {
        let mut current = vec![left_index + 1; right.len() + 1];
        for (right_index, right_character) in right.iter().enumerate() {
            current[right_index + 1] = (current[right_index] + 1)
                .min(previous[right_index + 1] + 1)
                .min(previous[right_index] + usize::from(left_character != *right_character));
        }
        previous = current;
    }
    previous[right.len()]
}

fn column_values<'a>(
    matrix: &'a TableMatrix,
    column: usize,
    header_rows: &[usize],
) -> Vec<&'a str> {
    matrix
        .rows
        .iter()
        .enumerate()
        .filter(|(row, _)| !header_rows.contains(row))
        .filter_map(|(_, row)| row.get(column))
        .map(|cell| cell.text.trim())
        .filter(|text| !text.is_empty())
        .collect()
}

fn data_role_score(role: ColumnRole, values: &[&str]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    match role {
        ColumnRole::Position => sequential_integer_score(values),
        ColumnRole::Code => pattern_ratio(values, is_code) * 0.82,
        ColumnRole::Name => {
            let text_ratio = pattern_ratio(values, |value| {
                let letters = value
                    .chars()
                    .filter(|character| character.is_alphabetic())
                    .count();
                letters >= 4 && value.chars().count() >= 6
            });
            text_ratio * 0.72
        }
        ColumnRole::Designation => {
            pattern_ratio(values, |value| designation_regex().is_match(value)) * 0.70
        }
        ColumnRole::Manufacturer => 0.0,
        ColumnRole::Unit => pattern_ratio(values, is_unit) * 0.95,
        ColumnRole::Quantity => pattern_ratio(values, is_plain_number) * 0.79,
        ColumnRole::Length => pattern_ratio(values, |value| length_regex().is_match(value)) * 0.90,
        ColumnRole::Price | ColumnRole::Amount => {
            pattern_ratio(values, |value| money_regex().is_match(value)) * 0.74
        }
        ColumnRole::Vat => pattern_ratio(values, |value| vat_regex().is_match(value)) * 0.98,
        ColumnRole::Currency => {
            pattern_ratio(values, |value| currency_regex().is_match(value)) * 0.98
        }
        ColumnRole::LaborHours => {
            pattern_ratio(values, |value| labor_regex().is_match(value)) * 0.98
        }
        ColumnRole::CableMark => {
            pattern_ratio(values, |value| cable_mark_regex().is_match(value)) * 0.96
        }
        ColumnRole::Conductors => {
            pattern_ratio(values, |value| conductors_regex().is_match(value)) * 0.92
        }
        ColumnRole::CrossSection => {
            pattern_ratio(values, |value| cross_section_regex().is_match(value)) * 0.96
        }
        ColumnRole::From | ColumnRole::To | ColumnRole::Unknown => 0.0,
    }
}

fn pattern_ratio(values: &[&str], predicate: impl Fn(&str) -> bool) -> f32 {
    values.iter().filter(|value| predicate(value)).count() as f32 / values.len() as f32
}

fn sequential_integer_score(values: &[&str]) -> f32 {
    let integers = values
        .iter()
        .filter_map(|value| value.trim().trim_end_matches('.').parse::<i64>().ok())
        .collect::<Vec<_>>();
    if integers.len() < 2 {
        return 0.0;
    }
    let monotonic = integers.windows(2).filter(|pair| pair[1] > pair[0]).count();
    let coverage = integers.len() as f32 / values.len() as f32;
    let order = monotonic as f32 / integers.len().saturating_sub(1) as f32;
    (coverage * 0.55 + order * 0.45).min(1.0)
}

fn is_plain_number(value: &str) -> bool {
    numeric_regex().is_match(value.trim())
}

fn is_code(value: &str) -> bool {
    let value = value.trim();
    code_regex().is_match(value)
        && value.chars().any(|character| character.is_alphabetic())
        && value.chars().any(|character| character.is_ascii_digit())
}

fn is_unit(value: &str) -> bool {
    unit_regex().is_match(&normalize_ocr_header(value))
}

fn numeric_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"^[+-]?\d+(?:[.,]\d+)?$").expect("numeric regex"))
}

fn unit_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?iu)^(?:шт|компл(?:ект)?|м|м2|м3|м²|кг|т|л|pcs?|set|kg|m|m2|m3|m²)$")
            .expect("unit regex")
    })
}

fn money_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?iu)^(?:[$€₽]\s*)?\d[\d\s]*(?:[.,]\d{2})?(?:\s*(?:руб|rub|usd|eur|₽|€|\$))?$")
            .expect("money regex")
    })
}

fn currency_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"(?iu)(?:\bруб\b|\brub\b|\busd\b|\beur\b|₽|€|\$)").unwrap())
}

fn vat_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"(?iu)(?:\bндс\b|\bvat\b|\b20\s*%)").unwrap())
}

fn labor_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?iu)(?:чел\s*[-.]?\s*ч|человеко\s*час|man\s*hours?|labor\s*hours?)").unwrap()
    })
}

fn length_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"(?iu)^\d+(?:[.,]\d+)?\s*(?:м|m|mm|мм)$").unwrap())
}

fn cable_mark_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(
            r"(?iu)(?:ввг|аввг|кввг|пвс|кг|nym|utp|ftp|frls|нг(?:-?ls)?|cable)[-()a-zа-я0-9+./]*",
        )
        .unwrap()
    })
}

fn conductors_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"(?iu)(?:^\d{1,2}$|\d{1,2}\s*(?:жил|cores?))").unwrap())
}

fn cross_section_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(
            r"(?iu)(?:\d{1,2}\s*[xх×]\s*\d+(?:[.,]\d+)?|\d+(?:[.,]\d+)?\s*(?:мм2|mm2|мм²|mm²))",
        )
        .unwrap()
    })
}

fn code_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"(?iu)^[a-zа-я0-9][a-zа-я0-9._/-]{2,}$").unwrap())
}

fn designation_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"(?iu)^[a-zа-я]{1,8}[-./]?[a-zа-я0-9-]{2,}$").unwrap())
}

fn role_confidence(assignments: &[ColumnRoleAssignment], role: ColumnRole) -> f32 {
    assignments
        .iter()
        .filter(|assignment| assignment.role == role)
        .map(|assignment| assignment.confidence)
        .fold(0.0_f32, f32::max)
}

fn contains_hint(heading: &str, hints: &[&str]) -> f32 {
    if hints.iter().any(|hint| heading.contains(hint)) {
        1.0
    } else {
        0.0
    }
}

fn required_role_score(assignments: &[ColumnRoleAssignment], roles: &[ColumnRole]) -> f32 {
    if roles.is_empty() {
        return 0.0;
    }
    roles
        .iter()
        .map(|role| role_confidence(assignments, *role))
        .sum::<f32>()
        / roles.len() as f32
}

fn optional_role_score(assignments: &[ColumnRoleAssignment], roles: &[ColumnRole]) -> f32 {
    roles
        .iter()
        .map(|role| role_confidence(assignments, *role))
        .fold(0.0_f32, f32::max)
}

fn cable_journal_score(roles: &[ColumnRoleAssignment], heading: &str, structure: f32) -> f32 {
    let core = required_role_score(
        roles,
        &[ColumnRole::CableMark, ColumnRole::From, ColumnRole::To],
    );
    if core < 0.55 {
        return 0.0;
    }
    let optional = optional_role_score(
        roles,
        &[
            ColumnRole::Length,
            ColumnRole::CrossSection,
            ColumnRole::Conductors,
        ],
    );
    let hint = contains_hint(
        heading,
        &["кабельн журнал", "кабельный журнал", "cable journal"],
    );
    (core * 0.62 + optional * 0.16 + hint * 0.16 + structure * 0.06).min(1.0)
}

fn commercial_offer_score(roles: &[ColumnRoleAssignment], heading: &str, structure: f32) -> f32 {
    let core = required_role_score(
        roles,
        &[
            ColumnRole::Name,
            ColumnRole::Quantity,
            ColumnRole::Price,
            ColumnRole::Amount,
        ],
    );
    if core < 0.56 {
        return 0.0;
    }
    let optional = optional_role_score(
        roles,
        &[
            ColumnRole::Vat,
            ColumnRole::Currency,
            ColumnRole::Manufacturer,
        ],
    );
    let hint = contains_hint(
        heading,
        &["коммерческое предложение", "quotation", "commercial offer"],
    );
    (core * 0.62 + optional * 0.14 + hint * 0.18 + structure * 0.06).min(1.0)
}

fn work_estimate_score(roles: &[ColumnRoleAssignment], heading: &str, structure: f32) -> f32 {
    let base = required_role_score(
        roles,
        &[ColumnRole::Name, ColumnRole::Unit, ColumnRole::Quantity],
    );
    let cost = optional_role_score(
        roles,
        &[
            ColumnRole::LaborHours,
            ColumnRole::Price,
            ColumnRole::Amount,
        ],
    );
    if base < 0.56 || cost < 0.58 {
        return 0.0;
    }
    let hint = contains_hint(
        heading,
        &["смета", "ведомость работ", "estimate", "bill of quantities"],
    );
    (base * 0.48 + cost * 0.25 + hint * 0.20 + structure * 0.07).min(1.0)
}

fn equipment_schedule_score(roles: &[ColumnRoleAssignment], heading: &str, structure: f32) -> f32 {
    let base = required_role_score(roles, &[ColumnRole::Name, ColumnRole::Quantity]);
    let equipment =
        optional_role_score(roles, &[ColumnRole::Designation, ColumnRole::Manufacturer]);
    if base < 0.58 || equipment < 0.58 {
        return 0.0;
    }
    let hint = contains_hint(
        heading,
        &[
            "ведомость оборудования",
            "equipment schedule",
            "equipment list",
        ],
    );
    (base * 0.43 + equipment * 0.28 + hint * 0.22 + structure * 0.07).min(1.0)
}

fn material_specification_score(
    roles: &[ColumnRoleAssignment],
    heading: &str,
    structure: f32,
) -> f32 {
    let base = required_role_score(
        roles,
        &[ColumnRole::Name, ColumnRole::Unit, ColumnRole::Quantity],
    );
    let identity = optional_role_score(
        roles,
        &[
            ColumnRole::Code,
            ColumnRole::Designation,
            ColumnRole::Manufacturer,
        ],
    );
    if base < 0.56 || identity < 0.50 {
        return 0.0;
    }
    let hint = contains_hint(
        heading,
        &[
            "спецификац",
            "материал",
            "material specification",
            "bill of materials",
        ],
    );
    (base * 0.52 + identity * 0.21 + hint * 0.20 + structure * 0.07).min(1.0)
}

fn geometry_score(rows: usize, columns: usize) -> f32 {
    let row_score = (rows as f32 / 5.0).min(1.0);
    let column_score = (columns as f32 / 4.0).min(1.0);
    (row_score * 0.45 + column_score * 0.55).clamp(0.0, 1.0)
}

fn lower_right_score(bbox: &crate::dto::BoundingBox, page_width: u32, page_height: u32) -> f32 {
    let page_width = f64::from(page_width.max(1));
    let page_height = f64::from(page_height.max(1));
    let right = (bbox.x1 / page_width).clamp(0.0, 1.0);
    let bottom = (bbox.y1 / page_height).clamp(0.0, 1.0);
    let left = (bbox.x0 / page_width).clamp(0.0, 1.0);
    let top = (bbox.y0 / page_height).clamp(0.0, 1.0);
    finite_unit(((right + bottom + left + top) / 4.0) as f32)
}

fn unknown_columns(columns: usize) -> Vec<ColumnRoleAssignment> {
    (0..columns)
        .map(|column| ColumnRoleAssignment {
            column,
            role: ColumnRole::Unknown,
            confidence: 0.0,
        })
        .collect()
}

fn finite_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn mean_populated_cell_confidence(matrix: &TableMatrix) -> f32 {
    let confidences = matrix
        .rows
        .iter()
        .flatten()
        .filter(|cell| !cell.text.trim().is_empty())
        .map(|cell| finite_unit(cell.confidence))
        .collect::<Vec<_>>();
    if confidences.is_empty() {
        0.0
    } else {
        finite_unit(confidences.iter().sum::<f32>() / confidences.len() as f32)
    }
}

fn role_wire_name(role: ColumnRole) -> &'static str {
    match role {
        ColumnRole::Position => "position",
        ColumnRole::Code => "code",
        ColumnRole::Name => "name",
        ColumnRole::Designation => "designation",
        ColumnRole::Manufacturer => "manufacturer",
        ColumnRole::Unit => "unit",
        ColumnRole::Quantity => "quantity",
        ColumnRole::Length => "length",
        ColumnRole::Price => "price",
        ColumnRole::Amount => "amount",
        ColumnRole::Vat => "vat",
        ColumnRole::Currency => "currency",
        ColumnRole::LaborHours => "labor_hours",
        ColumnRole::CableMark => "cable_mark",
        ColumnRole::Conductors => "conductors",
        ColumnRole::CrossSection => "cross_section",
        ColumnRole::From => "from",
        ColumnRole::To => "to",
        ColumnRole::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_table, ColumnRole, SemanticTableType};
    use crate::dto::{BoundingBox, TableCell, TableDetectorType, TableMatrix, TableType};

    fn matrix(rows: &[&[&str]]) -> TableMatrix {
        let mut result = TableMatrix::grid(
            rows.iter()
                .enumerate()
                .map(|(row, values)| {
                    values
                        .iter()
                        .enumerate()
                        .map(|(column, text)| {
                            let mut cell = TableCell::grid(
                                row as u32,
                                column as u32,
                                BoundingBox::new(
                                    column as f64 * 100.0,
                                    row as f64 * 30.0,
                                    (column + 1) as f64 * 100.0,
                                    (row + 1) as f64 * 30.0,
                                ),
                            );
                            cell.text = (*text).to_string();
                            cell.confidence = 0.92;
                            cell
                        })
                        .collect()
                })
                .collect(),
            BoundingBox::new(
                0.0,
                0.0,
                rows.first().map_or(0, |row| row.len()) as f64 * 100.0,
                rows.len() as f64 * 30.0,
            ),
            0.90,
        );
        result.detector_type = TableDetectorType::Grid;
        result
    }

    fn assert_role(classification: &super::TableClassification, role: ColumnRole) {
        assert!(
            classification
                .column_roles
                .iter()
                .any(|assignment| assignment.role == role),
            "missing role {role:?}: {:?}",
            classification.column_roles
        );
    }

    #[test]
    fn classifies_material_specification() {
        let table = matrix(&[
            &["Поз.", "Код", "Наименование", "Ед. изм.", "Кол-во"],
            &["1", "AX-100", "Шкаф управления", "шт", "2"],
            &["2", "BX-200", "Датчик давления", "шт", "4"],
        ]);
        let result = classify_table(&table, Some((1000, 700)));
        assert_eq!(
            result.semantic_type,
            SemanticTableType::MaterialSpecification
        );
        assert_role(&result, ColumnRole::Name);
        assert_role(&result, ColumnRole::Quantity);
    }

    #[test]
    fn classifies_cable_journal() {
        let table = matrix(&[
            &["Марка кабеля", "Откуда", "Куда", "Длина", "Сечение"],
            &["ВВГнг-LS", "ШУ-1", "М-1", "24 м", "3x2,5"],
            &["КВВГ", "ШУ-2", "К-2", "18 м", "5x1,5"],
        ]);
        let result = classify_table(&table, Some((1000, 700)));
        assert_eq!(result.semantic_type, SemanticTableType::CableJournal);
        assert_role(&result, ColumnRole::CableMark);
        assert_role(&result, ColumnRole::From);
        assert_role(&result, ColumnRole::To);
    }

    #[test]
    fn classifies_work_estimate() {
        let table = matrix(&[
            &[
                "Наименование работ",
                "Ед. изм.",
                "Количество",
                "Чел.-ч",
                "Сумма",
            ],
            &["Монтаж шкафа", "шт", "2", "4,5", "12000,00"],
            &["Прокладка кабеля", "м", "50", "8,0", "25000,00"],
        ]);
        let result = classify_table(&table, Some((1000, 700)));
        assert_eq!(result.semantic_type, SemanticTableType::WorkEstimate);
        assert_role(&result, ColumnRole::LaborHours);
    }

    #[test]
    fn classifies_commercial_offer() {
        let table = matrix(&[
            &["Наименование", "Количество", "Цена", "Сумма", "НДС"],
            &["Контроллер", "2", "15000,00 RUB", "30000,00 RUB", "20% НДС"],
            &["Модуль", "4", "2500,00 RUB", "10000,00 RUB", "20% НДС"],
        ]);
        let result = classify_table(&table, Some((1000, 700)));
        assert_eq!(result.semantic_type, SemanticTableType::CommercialOffer);
        assert_role(&result, ColumnRole::Price);
        assert_role(&result, ColumnRole::Amount);
    }

    #[test]
    fn classifies_equipment_schedule() {
        let table = matrix(&[
            &[
                "Ведомость оборудования",
                "Обозначение",
                "Производитель",
                "Количество",
            ],
            &["Насос", "P-101", "Vendor A", "2"],
            &["Клапан", "V-202", "Vendor B", "3"],
        ]);
        let result = classify_table(&table, Some((1000, 700)));
        assert_eq!(result.semantic_type, SemanticTableType::EquipmentSchedule);
        assert_role(&result, ColumnRole::Manufacturer);
    }

    #[test]
    fn noisy_headers_are_normalized_without_rewriting_cells() {
        let table = matrix(&[
            &["П0з.", "Код", "Наименованне", "Ед. нзм.", "К0л-в0"],
            &["1", "AX-100", "Шкаф управления", "шт", "2"],
            &["2", "BX-200", "Датчик давления", "шт", "4"],
        ]);
        let result = classify_table(&table, None);
        assert_eq!(
            result.semantic_type,
            SemanticTableType::MaterialSpecification
        );
        assert_eq!(table.rows[0][2].text, "Наименованне");
    }

    #[test]
    fn title_block_semantics_do_not_change_structural_kind() {
        let mut table = matrix(&[
            &["Лист", "Стадия", "Объект"],
            &["1", "Р", "Проект"],
            &["Изм", "Дата", "Подпись"],
        ]);
        table.table_type = TableType::TitleBlock;
        let result = classify_table(&table, Some((1000, 700)));
        assert_eq!(result.semantic_type, SemanticTableType::DrawingTitleBlock);
        assert_eq!(table.table_type, TableType::TitleBlock);
    }

    #[test]
    fn weak_legacy_matrix_is_unknown_instead_of_guessed() {
        let table = TableMatrix::new(vec![vec![TableCell::new("note")]]);
        let result = classify_table(&table, None);
        assert_eq!(result.semantic_type, SemanticTableType::Unknown);
        assert!(result.semantic_confidence < 0.5);
    }
}
