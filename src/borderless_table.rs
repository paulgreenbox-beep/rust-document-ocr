//! Conservative reconstruction of tables whose cells have no drawn borders.
//!
//! The ruled-grid detector always has priority. This module is intended only
//! as the `GridWithFallback` word-layout stage after ruled detection returns
//! no grids. Every threshold is relative to page/word geometry rather than a
//! fixed DPI.

use crate::dto::{BoundingBox, TableCell, TableMatrix, Word};
use crate::table_semantics::{classify_table, ColumnRole};

const MIN_ROWS: usize = 3;
const MIN_COLUMNS: usize = 3;
const MAX_COLUMNS: usize = 16;
const MIN_CONFIDENCE: f32 = 0.70;

#[derive(Debug, Clone)]
struct PhysicalLine {
    words: Vec<Word>,
    center_y: f64,
    y0: f64,
    y1: f64,
}

#[derive(Debug, Clone)]
struct Chunk {
    x0: f64,
    x1: f64,
}

#[derive(Debug, Clone)]
struct Anchor {
    position_sum: f64,
    samples: usize,
    rows: Vec<usize>,
}

impl Anchor {
    fn position(&self) -> f64 {
        self.position_sum / self.samples.max(1) as f64
    }

    fn add(&mut self, position: f64, row: usize) {
        self.position_sum += position;
        self.samples += 1;
        if self.rows.last().copied() != Some(row) {
            self.rows.push(row);
        }
    }
}

/// Detect one or more high-confidence borderless tables from positioned words.
///
/// The function does not inspect pixels and never synthesizes OCR content.
/// Returned cells contain only source words assigned once by geometry.
pub fn detect_borderless_tables(
    words: &[Word],
    page_width: u32,
    page_height: u32,
) -> Vec<TableMatrix> {
    let words = sanitize_words(words, page_width, page_height);
    if words.len() < MIN_ROWS * MIN_COLUMNS {
        return Vec::new();
    }
    let median_height = median(
        words
            .iter()
            .map(|word| word.bbox.y1 - word.bbox.y0)
            .collect(),
    )
    .max(1.0);
    let lines = cluster_lines(words, median_height);
    let components = structured_components(&lines, median_height, page_width);
    components
        .into_iter()
        .filter_map(|component| {
            reconstruct_component(&component, page_width, page_height, median_height)
        })
        .collect()
}

fn sanitize_words(words: &[Word], page_width: u32, page_height: u32) -> Vec<Word> {
    let page_width = f64::from(page_width.max(1));
    let page_height = f64::from(page_height.max(1));
    let mut filtered = words
        .iter()
        .filter(|word| {
            !word.text.trim().is_empty()
                && valid_box(&word.bbox)
                && word.bbox.x1 > 0.0
                && word.bbox.y1 > 0.0
                && word.bbox.x0 < page_width
                && word.bbox.y0 < page_height
        })
        .cloned()
        .collect::<Vec<_>>();
    filtered.sort_by(|left, right| {
        box_center_y(&left.bbox)
            .total_cmp(&box_center_y(&right.bbox))
            .then_with(|| left.bbox.x0.total_cmp(&right.bbox.x0))
    });

    let mut unique: Vec<Word> = Vec::with_capacity(filtered.len());
    for word in filtered {
        let duplicate = unique.iter().rev().take(12).any(|existing| {
            existing.text == word.text && overlap_ratio(&existing.bbox, &word.bbox) >= 0.90
        });
        if !duplicate {
            unique.push(word);
        }
    }
    unique
}

fn cluster_lines(words: Vec<Word>, median_height: f64) -> Vec<PhysicalLine> {
    let mut lines: Vec<PhysicalLine> = Vec::new();
    for word in words {
        let center = box_center_y(&word.bbox);
        let candidate = lines.iter_mut().rev().take(3).find(|line| {
            vertical_overlap_ratio(&line_bbox(line), &word.bbox) >= 0.35
                || (center - line.center_y).abs() <= median_height * 0.62
        });
        if let Some(line) = candidate {
            line.y0 = line.y0.min(word.bbox.y0);
            line.y1 = line.y1.max(word.bbox.y1);
            line.words.push(word);
            line.center_y = line
                .words
                .iter()
                .map(|item| box_center_y(&item.bbox))
                .sum::<f64>()
                / line.words.len() as f64;
        } else {
            let y0 = word.bbox.y0;
            let y1 = word.bbox.y1;
            lines.push(PhysicalLine {
                words: vec![word],
                center_y: center,
                y0,
                y1,
            });
        }
    }
    for line in &mut lines {
        line.words
            .sort_by(|left, right| left.bbox.x0.total_cmp(&right.bbox.x0));
    }
    lines.sort_by(|left, right| left.center_y.total_cmp(&right.center_y));
    lines
}

fn structured_components(
    lines: &[PhysicalLine],
    median_height: f64,
    page_width: u32,
) -> Vec<Vec<PhysicalLine>> {
    let gap_threshold = (median_height * 1.8).max(f64::from(page_width) * 0.016);
    let mut candidates = Vec::new();
    let mut current = Vec::new();
    let mut previous_center = None;

    for line in lines {
        let chunks = chunks_for_line(line, gap_threshold);
        let row_can_participate = chunks.len() >= 2;
        let close_to_previous = previous_center
            .is_none_or(|previous: f64| line.center_y - previous <= median_height * 3.8);
        if row_can_participate && close_to_previous {
            current.push(line.clone());
        } else {
            if component_has_shape(&current, gap_threshold) {
                candidates.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
            if row_can_participate {
                current.push(line.clone());
            }
        }
        previous_center = Some(line.center_y);
    }
    if component_has_shape(&current, gap_threshold) {
        candidates.push(current);
    }
    candidates
}

fn component_has_shape(lines: &[PhysicalLine], gap_threshold: f64) -> bool {
    lines.len() >= MIN_ROWS
        && lines
            .iter()
            .filter(|line| chunks_for_line(line, gap_threshold).len() >= MIN_COLUMNS)
            .count()
            >= 2
}

fn reconstruct_component(
    lines: &[PhysicalLine],
    page_width: u32,
    page_height: u32,
    median_height: f64,
) -> Option<TableMatrix> {
    if lines.len() < MIN_ROWS {
        return None;
    }
    let gap_threshold = (median_height * 1.8).max(f64::from(page_width) * 0.016);
    let anchor_tolerance = (median_height * 1.35).max(f64::from(page_width) * 0.010);
    let mut anchors: Vec<Anchor> = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        for chunk in chunks_for_line(line, gap_threshold) {
            if let Some(anchor) = anchors
                .iter_mut()
                .filter(|anchor| (anchor.position() - chunk.x0).abs() <= anchor_tolerance)
                .min_by(|left, right| {
                    (left.position() - chunk.x0)
                        .abs()
                        .total_cmp(&(right.position() - chunk.x0).abs())
                })
            {
                anchor.add(chunk.x0, row);
            } else {
                anchors.push(Anchor {
                    position_sum: chunk.x0,
                    samples: 1,
                    rows: vec![row],
                });
            }
        }
    }

    let minimum_support = ((lines.len() as f64 * 0.55).ceil() as usize).max(2);
    anchors.retain(|anchor| anchor.rows.len() >= minimum_support);
    anchors.sort_by(|left, right| left.position().total_cmp(&right.position()));
    merge_close_anchors(&mut anchors, anchor_tolerance);
    if !(MIN_COLUMNS..=MAX_COLUMNS).contains(&anchors.len()) {
        return None;
    }

    let positions = anchors.iter().map(Anchor::position).collect::<Vec<_>>();
    let table_bbox = union_words(lines.iter().flat_map(|line| &line.words))?;
    let x_boundaries = column_boundaries(&positions, &table_bbox);
    let y_boundaries = row_boundaries(lines, &table_bbox);
    if x_boundaries.len() != positions.len() + 1 || y_boundaries.len() != lines.len() + 1 {
        return None;
    }

    let mut assigned: Vec<Vec<Vec<&Word>>> = vec![vec![Vec::new(); positions.len()]; lines.len()];
    let mut source_words = 0_usize;
    for (row, line) in lines.iter().enumerate() {
        for word in &line.words {
            let center_x = (word.bbox.x0 + word.bbox.x1) / 2.0;
            let column = interval_index(center_x, &x_boundaries)?;
            assigned[row][column].push(word);
            source_words += 1;
        }
    }

    let mut non_empty = 0_usize;
    let mut assigned_words = 0_usize;
    let mut rows = Vec::with_capacity(lines.len());
    for (row, cells) in assigned.into_iter().enumerate() {
        let mut output_row = Vec::with_capacity(positions.len());
        for (column, mut words) in cells.into_iter().enumerate() {
            words.sort_by(|left, right| {
                box_center_y(&left.bbox)
                    .total_cmp(&box_center_y(&right.bbox))
                    .then_with(|| left.bbox.x0.total_cmp(&right.bbox.x0))
            });
            assigned_words += words.len();
            if !words.is_empty() {
                non_empty += 1;
            }
            let confidence = if words.is_empty() {
                0.0
            } else {
                words
                    .iter()
                    .map(|word| finite_unit(word.confidence))
                    .sum::<f32>()
                    / words.len() as f32
            };
            let text = words
                .iter()
                .map(|word| word.text.trim())
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            output_row.push(TableCell {
                text,
                row: row as u32,
                column: column as u32,
                row_span: 1,
                col_span: 1,
                bbox: Some(BoundingBox::new(
                    x_boundaries[column],
                    y_boundaries[row],
                    x_boundaries[column + 1],
                    y_boundaries[row + 1],
                )),
                confidence,
            });
        }
        rows.push(output_row);
    }
    if assigned_words != source_words {
        return None;
    }

    let total_cells = lines.len() * positions.len();
    let occupancy = non_empty as f32 / total_cells.max(1) as f32;
    let anchor_support = anchors
        .iter()
        .map(|anchor| anchor.rows.len() as f32 / lines.len() as f32)
        .sum::<f32>()
        / anchors.len() as f32;
    let regularity = row_spacing_regularity(lines, median_height);
    let average_word_confidence = lines
        .iter()
        .flat_map(|line| &line.words)
        .map(|word| finite_unit(word.confidence))
        .sum::<f32>()
        / source_words.max(1) as f32;

    let mut matrix = TableMatrix::borderless(rows, table_bbox, 0.0);
    let semantic = classify_table(&matrix, Some((page_width, page_height)));
    let known_roles = semantic
        .column_roles
        .iter()
        .filter(|role| role.role != ColumnRole::Unknown)
        .count();
    let domain_evidence = (known_roles as f32 / positions.len() as f32).min(1.0);
    let strong_geometry = occupancy >= 0.82 && anchor_support >= 0.78 && regularity >= 0.70;
    if occupancy < 0.48 || anchor_support < 0.55 || (known_roles < 2 && !strong_geometry) {
        return None;
    }

    let confidence = (occupancy * 0.29
        + anchor_support * 0.29
        + regularity * 0.18
        + average_word_confidence * 0.12
        + domain_evidence * 0.12)
        .clamp(0.0, 1.0);
    if confidence < MIN_CONFIDENCE {
        return None;
    }
    matrix.confidence = confidence;
    Some(matrix)
}

fn chunks_for_line(line: &PhysicalLine, gap_threshold: f64) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();
    for word in &line.words {
        if let Some(last) = chunks.last_mut() {
            if word.bbox.x0 - last.x1 <= gap_threshold {
                last.x1 = last.x1.max(word.bbox.x1);
                continue;
            }
        }
        chunks.push(Chunk {
            x0: word.bbox.x0,
            x1: word.bbox.x1,
        });
    }
    chunks
}

fn merge_close_anchors(anchors: &mut Vec<Anchor>, tolerance: f64) {
    let mut merged: Vec<Anchor> = Vec::new();
    for anchor in anchors.drain(..) {
        if let Some(last) = merged.last_mut() {
            if anchor.position() - last.position() <= tolerance {
                last.position_sum += anchor.position_sum;
                last.samples += anchor.samples;
                for row in anchor.rows {
                    if !last.rows.contains(&row) {
                        last.rows.push(row);
                    }
                }
                continue;
            }
        }
        merged.push(anchor);
    }
    *anchors = merged;
}

fn column_boundaries(positions: &[f64], bbox: &BoundingBox) -> Vec<f64> {
    let mut boundaries = Vec::with_capacity(positions.len() + 1);
    boundaries.push(bbox.x0);
    for pair in positions.windows(2) {
        boundaries.push((pair[0] + pair[1]) / 2.0);
    }
    boundaries.push(bbox.x1);
    boundaries
}

fn row_boundaries(lines: &[PhysicalLine], bbox: &BoundingBox) -> Vec<f64> {
    let mut boundaries = Vec::with_capacity(lines.len() + 1);
    boundaries.push(bbox.y0);
    for pair in lines.windows(2) {
        boundaries.push((pair[0].center_y + pair[1].center_y) / 2.0);
    }
    boundaries.push(bbox.y1);
    boundaries
}

fn interval_index(value: f64, boundaries: &[f64]) -> Option<usize> {
    let last = *boundaries.last()?;
    boundaries
        .windows(2)
        .position(|pair| value >= pair[0] && (value < pair[1] || value == last))
}

fn row_spacing_regularity(lines: &[PhysicalLine], median_height: f64) -> f32 {
    let gaps = lines
        .windows(2)
        .map(|pair| pair[1].center_y - pair[0].center_y)
        .filter(|gap| *gap > 0.0)
        .collect::<Vec<_>>();
    if gaps.is_empty() {
        return 0.0;
    }
    let median_gap = median(gaps.clone()).max(median_height * 0.5);
    let mean_deviation =
        gaps.iter().map(|gap| (gap - median_gap).abs()).sum::<f64>() / gaps.len() as f64;
    (1.0 - (mean_deviation / median_gap).min(1.0)) as f32
}

fn union_words<'a>(words: impl Iterator<Item = &'a Word>) -> Option<BoundingBox> {
    words.fold(None::<BoundingBox>, |current, word| {
        Some(match current {
            None => word.bbox.clone(),
            Some(bbox) => BoundingBox::new(
                bbox.x0.min(word.bbox.x0),
                bbox.y0.min(word.bbox.y0),
                bbox.x1.max(word.bbox.x1),
                bbox.y1.max(word.bbox.y1),
            ),
        })
    })
}

fn line_bbox(line: &PhysicalLine) -> BoundingBox {
    BoundingBox::new(
        line.words
            .iter()
            .map(|word| word.bbox.x0)
            .fold(f64::INFINITY, f64::min),
        line.y0,
        line.words
            .iter()
            .map(|word| word.bbox.x1)
            .fold(f64::NEG_INFINITY, f64::max),
        line.y1,
    )
}

fn valid_box(bbox: &BoundingBox) -> bool {
    bbox.x0.is_finite()
        && bbox.y0.is_finite()
        && bbox.x1.is_finite()
        && bbox.y1.is_finite()
        && bbox.x1 > bbox.x0
        && bbox.y1 > bbox.y0
}

fn box_center_y(bbox: &BoundingBox) -> f64 {
    (bbox.y0 + bbox.y1) / 2.0
}

fn vertical_overlap_ratio(left: &BoundingBox, right: &BoundingBox) -> f64 {
    let overlap = (left.y1.min(right.y1) - left.y0.max(right.y0)).max(0.0);
    let minimum_height = (left.y1 - left.y0).min(right.y1 - right.y0).max(1.0);
    overlap / minimum_height
}

fn overlap_ratio(left: &BoundingBox, right: &BoundingBox) -> f64 {
    let width = (left.x1.min(right.x1) - left.x0.max(right.x0)).max(0.0);
    let height = (left.y1.min(right.y1) - left.y0.max(right.y0)).max(0.0);
    let intersection = width * height;
    let minimum_area = ((left.x1 - left.x0) * (left.y1 - left.y0))
        .min((right.x1 - right.x0) * (right.y1 - right.y0))
        .max(1.0);
    intersection / minimum_area
}

#[expect(
    clippy::manual_is_multiple_of,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

fn finite_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::detect_borderless_tables;
    use crate::dto::{BoundingBox, TableDetectorType, Word};

    fn word(text: &str, x: f64, y: f64) -> Word {
        Word::new(text, BoundingBox::new(x, y, x + 58.0, y + 16.0), 0.94)
    }

    fn specification(scale: f64) -> Vec<Word> {
        let rows = [
            ["Поз.", "Код", "Наименование", "Ед. изм.", "Кол-во"],
            ["1", "AX-100", "Шкаф", "шт", "2"],
            ["2", "BX-200", "Датчик", "шт", "4"],
            ["3", "CX-300", "Кабель", "м", "20"],
        ];
        rows.iter()
            .enumerate()
            .flat_map(|(row, values)| {
                values.iter().enumerate().map(move |(column, value)| {
                    let mut item = word(
                        value,
                        60.0 + column as f64 * 150.0,
                        80.0 + row as f64 * 34.0,
                    );
                    item.bbox.x0 *= scale;
                    item.bbox.x1 *= scale;
                    item.bbox.y0 *= scale;
                    item.bbox.y1 *= scale;
                    item
                })
            })
            .collect()
    }

    #[test]
    fn reconstructs_borderless_specification_with_real_geometry() {
        let tables = detect_borderless_tables(&specification(1.0), 900, 600);
        assert_eq!(tables.len(), 1);
        let table = &tables[0];
        assert_eq!(table.detector_type, TableDetectorType::Borderless);
        assert_eq!(table.rows.len(), 4);
        assert_eq!(table.rows[0].len(), 5);
        assert!(table.bbox.is_some());
        assert!(table.confidence >= 0.70);
        assert_eq!(table.rows[2][2].text, "Датчик");
        assert!(table
            .rows
            .iter()
            .flatten()
            .all(|cell| cell.bbox.is_some() && cell.row_span == 1 && cell.col_span == 1));
    }

    #[test]
    fn scale_invariance_does_not_depend_on_one_dpi() {
        for scale in [0.5, 1.0, 2.0] {
            let tables = detect_borderless_tables(
                &specification(scale),
                (900.0 * scale) as u32,
                (600.0 * scale) as u32,
            );
            assert_eq!(tables.len(), 1, "scale={scale}");
            assert_eq!(tables[0].rows.len(), 4);
            assert_eq!(tables[0].rows[0].len(), 5);
        }
    }

    #[test]
    fn missing_cell_does_not_shift_following_columns() {
        let mut words = specification(1.0);
        words.retain(|word| !(word.text == "Датчик" && word.bbox.y0 > 100.0));
        let tables = detect_borderless_tables(&words, 900, 600);
        assert_eq!(tables.len(), 1);
        assert!(tables[0].rows[2][2].text.is_empty());
        assert_eq!(tables[0].rows[2][3].text, "шт");
    }

    #[test]
    fn paragraph_and_two_column_prose_are_rejected() {
        let mut words = Vec::new();
        for row in 0..8 {
            words.push(word("Paragraph", 60.0, 60.0 + row as f64 * 24.0));
            words.push(word("continuation", 145.0, 60.0 + row as f64 * 24.0));
        }
        assert!(detect_borderless_tables(&words, 900, 600).is_empty());
    }

    #[test]
    fn duplicate_words_are_assigned_only_once() {
        let mut words = specification(1.0);
        words.push(words[7].clone());
        let tables = detect_borderless_tables(&words, 900, 600);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].rows[1][2].text, "Шкаф");
    }
}
