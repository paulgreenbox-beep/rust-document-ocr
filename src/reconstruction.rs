use crate::dto::{TableCell, TableMatrix, Word};

/// Reconstruct a table by clustering word bounding-box coordinates.
///
/// Rows use the first word's `y0` as their anchor. Within each row, words
/// are ordered by `x0` and assigned to columns using ordered column anchors
/// shared across the entire table.
pub fn reconstruct_table(mut words: Vec<Word>, y_tolerance: f64, x_tolerance: f64) -> TableMatrix {
    if words.is_empty() {
        return TableMatrix::default();
    }

    words.sort_by(|left, right| left.bbox.y0.total_cmp(&right.bbox.y0));

    let mut row_groups: Vec<(f64, Vec<Word>)> = Vec::new();
    for word in words {
        let belongs_to_row = row_groups
            .last()
            .is_some_and(|(anchor, _)| (word.bbox.y0 - *anchor).abs() < y_tolerance);

        if belongs_to_row {
            row_groups
                .last_mut()
                .expect("row group exists")
                .1
                .push(word);
        } else {
            row_groups.push((word.bbox.y0, vec![word]));
        }
    }

    for (_, row) in &mut row_groups {
        row.sort_by(|left, right| left.bbox.x0.total_cmp(&right.bbox.x0));
    }

    let mut column_anchors = Vec::new();
    for (_, row) in &row_groups {
        for word in row {
            if !column_anchors
                .iter()
                .any(|anchor: &f64| (word.bbox.x0 - *anchor).abs() < x_tolerance)
            {
                let insertion_index = column_anchors
                    .iter()
                    .position(|anchor| *anchor > word.bbox.x0)
                    .unwrap_or(column_anchors.len());
                column_anchors.insert(insertion_index, word.bbox.x0);
            }
        }
    }

    let rows = row_groups
        .into_iter()
        .map(|(_, words)| {
            let mut cells: Vec<Option<TableCell>> =
                (0..column_anchors.len()).map(|_| None).collect();

            for word in words {
                if let Some(column) = column_anchors
                    .iter()
                    .position(|anchor| (word.bbox.x0 - *anchor).abs() < x_tolerance)
                {
                    cells[column] = Some(TableCell::new(word.text));
                }
            }

            cells
                .into_iter()
                .map(|cell| cell.unwrap_or_default())
                .collect()
        })
        .collect();

    TableMatrix::new(rows)
}

#[cfg(test)]
mod tests {
    use super::reconstruct_table;
    use crate::dto::{BoundingBox, Word};

    fn word(text: &str, x0: f64, y0: f64) -> Word {
        Word::new(text, BoundingBox::new(x0, y0, x0 + 10.0, y0 + 10.0), 1.0)
    }

    #[test]
    fn reconstructs_a_two_by_two_grid() {
        let matrix = reconstruct_table(
            vec![
                word("A", 10.0, 10.0),
                word("B", 100.0, 10.0),
                word("C", 10.0, 50.0),
                word("D", 100.0, 50.0),
            ],
            5.0,
            10.0,
        );

        assert_eq!(matrix.rows.len(), 2);
        assert_eq!(matrix.rows[0].len(), 2);
        assert_eq!(matrix.rows[0][0].text, "A");
        assert_eq!(matrix.rows[0][1].text, "B");
        assert_eq!(matrix.rows[1][0].text, "C");
        assert_eq!(matrix.rows[1][1].text, "D");
    }

    #[test]
    fn empty_input_produces_empty_matrix() {
        let matrix = reconstruct_table(Vec::new(), 5.0, 10.0);

        assert!(matrix.rows.is_empty());
    }

    #[test]
    fn reconstructs_a_single_row() {
        let matrix = reconstruct_table(
            vec![word("one", 10.0, 20.0), word("two", 50.0, 20.0)],
            5.0,
            10.0,
        );

        assert_eq!(matrix.rows.len(), 1);
        assert_eq!(
            matrix.rows[0]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<Vec<_>>(),
            ["one", "two"]
        );
    }

    #[test]
    fn tolerance_groups_slightly_offset_words() {
        let matrix = reconstruct_table(
            vec![
                word("A", 10.0, 10.0),
                word("B", 105.0, 13.0),
                word("C", 12.0, 50.0),
                word("D", 103.0, 53.0),
            ],
            5.0,
            10.0,
        );

        assert_eq!(matrix.rows.len(), 2);
        assert_eq!(
            matrix.rows[0]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
        assert_eq!(
            matrix.rows[1]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<Vec<_>>(),
            ["C", "D"]
        );
    }
}
