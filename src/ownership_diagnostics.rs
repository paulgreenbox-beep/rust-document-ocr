//! Shadow diagnostics for the ownership of positioned words on one page.

use std::{borrow::Cow, collections::HashSet};

use serde::Serialize;

use crate::dto::{BoundingBox, TableType};

pub(crate) struct TableOwnershipEvidence {
    pub table_id: usize,
    pub table_type: TableType,
    pub bbox: Option<BoundingBox>,
    // Page-local identifiers from a shared source word stream. An empty
    // vector proves the table received no words; None means IDs are missing.
    pub word_ids: Option<Vec<usize>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TableOwnershipStatus {
    ObservedConflict,
    ObservedNoConflict,
    NotObservable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TableOwnershipReason {
    SharedSourceWordsInOverlappingDataTables,
    NoOverlappingDataTables,
    NoSharedSourceWords,
    MissingOrInvalidTableGeometry,
    MissingSourceWordIds,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TableOwnershipConflictPair {
    /// Zero-based indices of the two structural tables on this page.
    pub table_ids: [usize; 2],
    pub shared_words: usize,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct PageTableOwnership {
    pub page_number: u32,
    pub status: TableOwnershipStatus,
    pub conflict_pairs: usize,
    pub unique_conflicting_words: usize,
    /// For each conflicting source word, the count of distinct tables with
    /// at least one confirmed ownership conflict for that word, minus one.
    /// Repeated pairs and repeated IDs within a table add no memberships.
    pub additional_memberships: usize,
    /// Every eligible pair could be ruled out or tested using source IDs.
    pub evidence_complete: bool,
    pub reasons: Vec<TableOwnershipReason>,
    pub pairs: Vec<TableOwnershipConflictPair>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TableOwnershipReport {
    pub schema_version: u32,
    pub pages: Vec<PageTableOwnership>,
}

pub(crate) fn diagnose_page(
    page_number: u32,
    tables: &[TableOwnershipEvidence],
) -> PageTableOwnership {
    let mut candidates = tables
        .iter()
        .filter(|table| table.table_type == TableType::DataTable)
        .collect::<Vec<_>>();
    candidates.sort_unstable_by_key(|table| table.table_id);

    let mut pairs = Vec::new();
    let mut conflicting_words = HashSet::new();
    let mut memberships = HashSet::new();
    let mut any_overlap = false;
    let mut missing_geometry = false;
    let mut missing_word_ids = false;
    let mut sorted_ids = vec![None; candidates.len()];

    for (left_index, left) in candidates.iter().enumerate() {
        for (right_index, right) in candidates.iter().enumerate().skip(left_index + 1) {
            if left.table_id == right.table_id {
                continue;
            }
            let (Some(left_bbox), Some(right_bbox)) = (
                valid_bbox(left.bbox.as_ref()),
                valid_bbox(right.bbox.as_ref()),
            ) else {
                missing_geometry = true;
                continue;
            };
            if !overlaps(left_bbox, right_bbox) {
                continue;
            }
            any_overlap = true;
            let (Some(left_words), Some(right_words)) =
                (left.word_ids.as_deref(), right.word_ids.as_deref())
            else {
                missing_word_ids = true;
                continue;
            };

            let (before_right, from_right) = sorted_ids.split_at_mut(right_index);
            let left_words = before_right[left_index].get_or_insert_with(|| sorted(left_words));
            let right_words = from_right[0].get_or_insert_with(|| sorted(right_words));
            let table_ids = [left.table_id, right.table_id];
            let shared_words = for_each_shared_word(left_words, right_words, |word_id| {
                conflicting_words.insert(word_id);
                memberships.insert((word_id, table_ids[0]));
                memberships.insert((word_id, table_ids[1]));
            });
            if shared_words > 0 {
                pairs.push(TableOwnershipConflictPair {
                    table_ids,
                    shared_words,
                });
            }
        }
    }

    let evidence_complete = !missing_geometry && !missing_word_ids;
    let mut reasons = Vec::new();
    let status = if !pairs.is_empty() {
        reasons.push(TableOwnershipReason::SharedSourceWordsInOverlappingDataTables);
        TableOwnershipStatus::ObservedConflict
    } else if evidence_complete {
        reasons.push(if any_overlap {
            TableOwnershipReason::NoSharedSourceWords
        } else {
            TableOwnershipReason::NoOverlappingDataTables
        });
        TableOwnershipStatus::ObservedNoConflict
    } else {
        TableOwnershipStatus::NotObservable
    };
    if missing_geometry {
        reasons.push(TableOwnershipReason::MissingOrInvalidTableGeometry);
    }
    if missing_word_ids {
        reasons.push(TableOwnershipReason::MissingSourceWordIds);
    }

    PageTableOwnership {
        page_number,
        status,
        conflict_pairs: pairs.len(),
        unique_conflicting_words: conflicting_words.len(),
        additional_memberships: memberships.len() - conflicting_words.len(),
        evidence_complete,
        reasons,
        pairs,
    }
}

fn valid_bbox(bbox: Option<&BoundingBox>) -> Option<&BoundingBox> {
    bbox.filter(|bbox| {
        [bbox.x0, bbox.y0, bbox.x1, bbox.y1]
            .into_iter()
            .all(f64::is_finite)
            && bbox.x0 < bbox.x1
            && bbox.y0 < bbox.y1
    })
}

fn overlaps(left: &BoundingBox, right: &BoundingBox) -> bool {
    left.x0 < right.x1 && right.x0 < left.x1 && left.y0 < right.y1 && right.y0 < left.y1
}

fn sorted(ids: &[usize]) -> Cow<'_, [usize]> {
    if ids.windows(2).all(|pair| pair[0] <= pair[1]) {
        Cow::Borrowed(ids)
    } else {
        let mut sorted_ids = ids.to_vec();
        sorted_ids.sort_unstable();
        Cow::Owned(sorted_ids)
    }
}

fn for_each_shared_word(
    mut left: &[usize],
    mut right: &[usize],
    mut observe: impl FnMut(usize),
) -> usize {
    use std::cmp::Ordering;

    let mut shared_words = 0;
    while let (Some(&a), Some(&b)) = (left.first(), right.first()) {
        match a.cmp(&b) {
            Ordering::Less => left = &left[1..],
            Ordering::Greater => right = &right[1..],
            Ordering::Equal => {
                observe(a);
                shared_words += 1;
                left = &left[left.iter().take_while(|&&id| id == a).count()..];
                right = &right[right.iter().take_while(|&&id| id == b).count()..];
            }
        }
    }
    shared_words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(id: usize, x0: f64, x1: f64, word_ids: &[usize]) -> TableOwnershipEvidence {
        TableOwnershipEvidence {
            table_id: id,
            table_type: TableType::DataTable,
            bbox: Some(BoundingBox::new(x0, 0.0, x1, 10.0)),
            word_ids: Some(word_ids.to_vec()),
        }
    }

    #[test]
    fn overlapping_tables_share_actual_source_ids() {
        let report = diagnose_page(
            4,
            &[table(9, 0.0, 10.0, &[3, 2]), table(2, 5.0, 15.0, &[2, 7])],
        );
        assert_eq!(report.page_number, 4);
        assert_eq!(report.status, TableOwnershipStatus::ObservedConflict);
        assert!(report.evidence_complete);
        assert_eq!(
            (
                report.conflict_pairs,
                report.unique_conflicting_words,
                report.additional_memberships
            ),
            (1, 1, 1)
        );
        assert_eq!(
            report.pairs,
            [TableOwnershipConflictPair {
                table_ids: [2, 9],
                shared_words: 1
            }]
        );
        assert_eq!(
            report.reasons,
            [TableOwnershipReason::SharedSourceWordsInOverlappingDataTables]
        );
    }

    #[test]
    fn distinct_source_ids_do_not_conflict_even_for_equal_text() {
        // Both source words could read "12". The diagnostic only accepts IDs.
        let report = diagnose_page(1, &[table(0, 0.0, 10.0, &[11]), table(1, 5.0, 15.0, &[12])]);
        assert_eq!(report.status, TableOwnershipStatus::ObservedNoConflict);
        assert_eq!(report.reasons, [TableOwnershipReason::NoSharedSourceWords]);
    }

    #[test]
    fn touching_boundaries_have_no_positive_area() {
        let report = diagnose_page(1, &[table(0, 0.0, 10.0, &[7]), table(1, 10.0, 20.0, &[7])]);
        assert_eq!(report.status, TableOwnershipStatus::ObservedNoConflict);
        assert_eq!(
            report.reasons,
            [TableOwnershipReason::NoOverlappingDataTables]
        );
    }

    #[test]
    fn disjoint_tables_have_no_conflict() {
        let report = diagnose_page(1, &[table(0, 0.0, 10.0, &[7]), table(1, 11.0, 20.0, &[7])]);
        assert_eq!(report.status, TableOwnershipStatus::ObservedNoConflict);
    }

    #[test]
    fn title_blocks_do_not_participate() {
        let mut title_block = table(1, 5.0, 15.0, &[7]);
        title_block.table_type = TableType::TitleBlock;
        let report = diagnose_page(1, &[table(0, 0.0, 10.0, &[7]), title_block]);
        assert_eq!(report.status, TableOwnershipStatus::ObservedNoConflict);
        assert_eq!(report.conflict_pairs, 0);
    }

    #[test]
    fn a_merged_cell_stays_within_its_own_table() {
        // A span's slots, even if the same membership is reported twice,
        // belong to one table and cannot manufacture a second owner.
        let report = diagnose_page(
            1,
            &[table(0, 0.0, 10.0, &[7, 7]), table(1, 5.0, 15.0, &[8])],
        );
        assert_eq!(report.status, TableOwnershipStatus::ObservedNoConflict);
        assert_eq!(report.additional_memberships, 0);
    }

    #[test]
    fn missing_ids_prevent_an_observed_negative() {
        let mut unobserved = table(1, 5.0, 15.0, &[]);
        unobserved.word_ids = None;
        let report = diagnose_page(1, &[table(0, 0.0, 10.0, &[7]), unobserved]);
        assert_eq!(report.status, TableOwnershipStatus::NotObservable);
        assert_eq!(report.reasons, [TableOwnershipReason::MissingSourceWordIds]);
        assert!(!report.evidence_complete);
    }

    #[test]
    fn three_tables_count_unique_words_and_memberships() {
        let report = diagnose_page(
            1,
            &[
                table(0, 0.0, 10.0, &[7, 7]),
                table(1, 1.0, 11.0, &[7]),
                table(2, 2.0, 12.0, &[7, 7]),
            ],
        );
        assert_eq!(report.status, TableOwnershipStatus::ObservedConflict);
        assert_eq!(
            (
                report.conflict_pairs,
                report.unique_conflicting_words,
                report.additional_memberships
            ),
            (3, 1, 2)
        );
        assert!(report.pairs.iter().all(|pair| pair.shared_words == 1));
    }

    #[test]
    fn confirmed_conflict_survives_incomplete_evidence() {
        let mut unobserved = table(2, 2.0, 12.0, &[7]);
        unobserved.bbox = None;
        let report = diagnose_page(
            1,
            &[
                table(0, 0.0, 10.0, &[7]),
                table(1, 1.0, 11.0, &[7]),
                unobserved,
            ],
        );
        assert_eq!(report.status, TableOwnershipStatus::ObservedConflict);
        assert_eq!(
            report.reasons,
            [
                TableOwnershipReason::SharedSourceWordsInOverlappingDataTables,
                TableOwnershipReason::MissingOrInvalidTableGeometry,
            ]
        );
        assert!(!report.evidence_complete);
        assert_eq!(report.additional_memberships, 1);
    }

    #[test]
    fn known_empty_ids_differ_from_missing_ids() {
        let report = diagnose_page(1, &[table(0, 0.0, 10.0, &[]), table(1, 5.0, 15.0, &[7])]);
        assert_eq!(report.status, TableOwnershipStatus::ObservedNoConflict);
        assert!(report.evidence_complete);
    }
}
