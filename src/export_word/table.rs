use std::fmt::Write as _;

use super::{paragraph, PageGeometry, WordExportOptions, WordExportReport};
use crate::api::{BoundingBox, DetectedGrid, GridKind, TableCell};
use crate::dto::{PageResult, TableDetectorType, TableMatrix, TableType};
use crate::page_analysis::RenderSize;

const WORD_MAX_COLUMNS: usize = 63;
const MAX_GEOMETRY_SLOTS: usize = 250_000;

pub(super) fn append_tables(
    xml: &mut String,
    page: &PageResult,
    grids: Option<&[DetectedGrid]>,
    geometry: PageGeometry,
    render: Option<RenderSize>,
    options: WordExportOptions,
    report: &mut WordExportReport,
) {
    let mut order: Vec<_> = page
        .tables
        .iter()
        .enumerate()
        .filter(|(_, matrix)| {
            options.include_title_blocks || matrix.table_type != TableType::TitleBlock
        })
        .collect();
    order.sort_by(|(left_index, left), (right_index, right)| {
        let (ly, lx) = position(left);
        let (ry, rx) = position(right);
        (left.table_type == TableType::TitleBlock)
            .cmp(&(right.table_type == TableType::TitleBlock))
            .then(ly.total_cmp(&ry))
            .then(lx.total_cmp(&rx))
            .then(left_index.cmp(right_index))
    });

    let mut previous_table = false;
    for (index, matrix) in order {
        let grid = grids.and_then(|g| g.get(index));
        let columns = matrix.rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            continue;
        }
        let title = matrix.table_type == TableType::TitleBlock;
        if title {
            paragraph(xml, "Title block", true);
        } else if previous_table {
            // Adjacent Word tables with the same properties can be coalesced
            // by readers. A separate paragraph preserves region boundaries.
            xml.push_str("<w:p/>");
        }
        if let Some((grid, layout)) = grid.and_then(|g| checked_layout(matrix, g).map(|l| (g, l))) {
            append_grid(xml, grid, &layout, geometry, render);
        } else {
            if matrix.detector_type != TableDetectorType::WordAlignmentFallback {
                report.warnings.push(format!(
                    "Word page {}, table {}: grid geometry unavailable or inconsistent; used editable matrix",
                    page.page_number,
                    index + 1
                ));
            }
            if columns > WORD_MAX_COLUMNS {
                report.warnings.push(format!(
                    "Word page {}, table {}: split at the Word limit of {WORD_MAX_COLUMNS} columns",
                    page.page_number,
                    index + 1
                ));
            }
            append_matrix(xml, matrix, columns, geometry);
        }
        previous_table = true;
    }
}

fn position(matrix: &TableMatrix) -> (f64, f64) {
    matrix
        .bbox
        .as_ref()
        .filter(|b| {
            [b.x0, b.y0, b.x1, b.y1].into_iter().all(f64::is_finite) && b.x1 > b.x0 && b.y1 > b.y0
        })
        .map_or((f64::INFINITY, f64::INFINITY), |b| (b.y0, b.x0))
}

fn checked_layout(matrix: &TableMatrix, grid: &DetectedGrid) -> Option<Vec<Option<usize>>> {
    let expected_kind = match (&matrix.detector_type, &matrix.table_type) {
        (TableDetectorType::WordAlignmentFallback, _) => return None,
        (_, TableType::DataTable) => GridKind::DataTable,
        (_, TableType::TitleBlock) => GridKind::TitleBlock,
    };
    let slots = grid.rows.checked_mul(grid.columns)?;
    if grid.kind != expected_kind
        || grid.rows != matrix.rows.len()
        || grid.columns != matrix.rows.iter().map(Vec::len).max().unwrap_or(0)
        || !(1..=WORD_MAX_COLUMNS).contains(&grid.columns)
        || !(1..=MAX_GEOMETRY_SLOTS).contains(&slots)
        || grid.cells.len() > slots
        || matrix
            .bbox
            .as_ref()
            .is_some_and(|bbox| box_from_legacy(bbox) != grid.bbox)
    {
        return None;
    }

    let mut seen = vec![false; slots];
    let mut owner = vec![None; slots];
    for (index, cell) in grid.cells.iter().enumerate() {
        let row_end = cell.row.checked_add(cell.row_span)?;
        let col_end = cell.column.checked_add(cell.col_span)?;
        if cell.row_span == 0 || cell.col_span == 0 || row_end > grid.rows || col_end > grid.columns
        {
            return None;
        }
        let offset = cell.row * grid.columns + cell.column;
        if std::mem::replace(&mut seen[offset], true) {
            return None;
        }
        let original = matrix.rows.get(cell.row)?.get(cell.column)?;
        if original.row as usize != cell.row
            || original.column as usize != cell.column
            || cell.text != original.text
            || cell.row_span != original.row_span as usize
            || cell.col_span != original.col_span as usize
            || original
                .bbox
                .as_ref()
                .map_or_else(BoundingBox::default, box_from_legacy)
                != cell.bbox
        {
            return None;
        }
        if cell.bbox == BoundingBox::default()
            && cell.text.is_empty()
            && cell.row_span == 1
            && cell.col_span == 1
        {
            // Legacy physical slot covered by a merge (or no verified cell).
            continue;
        }
        if cell.bbox.width == 0 || cell.bbox.height == 0 || !inside_grid(cell, grid) {
            return None;
        }
        for row in cell.row..row_end {
            for column in cell.column..col_end {
                if owner[row * grid.columns + column].replace(index).is_some() {
                    return None;
                }
            }
        }
    }
    for (row, cells) in matrix.rows.iter().enumerate() {
        for (column, cell) in cells.iter().enumerate() {
            if !seen[row * grid.columns + column]
                && (!cell.text.is_empty()
                    || cell.bbox.is_some()
                    || cell.row_span != 1
                    || cell.col_span != 1)
            {
                return None;
            }
        }
    }
    Some(owner)
}

fn inside_grid(cell: &TableCell, grid: &DetectedGrid) -> bool {
    // Tolerate one pixel of independent rounding around physical borders.
    let margin = 1;
    cell.bbox.x.saturating_add(margin) >= grid.bbox.x
        && cell.bbox.y.saturating_add(margin) >= grid.bbox.y
        && cell
            .bbox
            .x
            .checked_add(cell.bbox.width)
            .is_some_and(|right| {
                right
                    <= grid
                        .bbox
                        .x
                        .saturating_add(grid.bbox.width)
                        .saturating_add(margin)
            })
        && cell
            .bbox
            .y
            .checked_add(cell.bbox.height)
            .is_some_and(|bottom| {
                bottom
                    <= grid
                        .bbox
                        .y
                        .saturating_add(grid.bbox.height)
                        .saturating_add(margin)
            })
}

fn box_from_legacy(b: &crate::dto::BoundingBox) -> BoundingBox {
    fn pixel(n: f64) -> u32 {
        if n.is_finite() {
            n.round() as u32
        } else {
            0
        }
    }
    let x = pixel(b.x0);
    let y = pixel(b.y0);
    BoundingBox {
        x,
        y,
        width: pixel(b.x1).saturating_sub(x),
        height: pixel(b.y1).saturating_sub(y),
    }
}

fn append_grid(
    xml: &mut String,
    grid: &DetectedGrid,
    owner: &[Option<usize>],
    geometry: PageGeometry,
    render: Option<RenderSize>,
) {
    let (width, indent) = table_width(grid, geometry, render);
    let widths = physical_widths(grid, width);
    start_table(xml, &widths, indent);
    for row in 0..grid.rows {
        xml.push_str("<w:tr>");
        let mut column = 0;
        while column < grid.columns {
            if let Some(cell_index) = owner[row * grid.columns + column] {
                let cell = &grid.cells[cell_index];
                debug_assert_eq!(column, cell.column);
                let width = widths[column..column + cell.col_span].iter().sum();
                let head = row == cell.row;
                let merge =
                    (cell.row_span > 1).then_some(if head { "restart" } else { "continue" });
                append_cell(
                    xml,
                    if head { &cell.text } else { "" },
                    width,
                    cell.col_span,
                    merge,
                );
                column += cell.col_span;
            } else {
                append_cell(xml, "", widths[column], 1, None);
                column += 1;
            }
        }
        xml.push_str("</w:tr>");
    }
    xml.push_str("</w:tbl>");
}

fn table_width(
    grid: &DetectedGrid,
    geometry: PageGeometry,
    render: Option<RenderSize>,
) -> (u32, u32) {
    let content_width = geometry.content_width();
    if let Some(render) = render.filter(|r| r.width > 0 && r.height > 0) {
        // OCR bounding boxes are pixels. Only convert them when an unambiguous
        // display-size mapping is available (including near-equal aspect).
        let sx = f64::from(geometry.width) / f64::from(render.width);
        let sy = f64::from(geometry.height) / f64::from(render.height);
        if (sx / sy - 1.0).abs() <= 0.05
            && grid.bbox.x.saturating_add(grid.bbox.width) <= render.width
        {
            let width = (sx * f64::from(grid.bbox.width)).round() as u32;
            let min_width = (grid.columns as u32 * 120).min(content_width);
            let width = width.clamp(min_width, content_width);
            let left = (sx * f64::from(grid.bbox.x)).round() as u32;
            let indent = left
                .saturating_sub(geometry.margin_x)
                .min(content_width - width);
            return (width, indent);
        }
    }
    (content_width, 0)
}

fn physical_widths(grid: &DetectedGrid, width: u32) -> Vec<u32> {
    let mut edges = vec![None; grid.columns + 1];
    for cell in &grid.cells {
        if cell.bbox.width == 0 {
            continue;
        }
        for (edge, coordinate) in [
            (cell.column, cell.bbox.x),
            (cell.column + cell.col_span, cell.bbox.x + cell.bbox.width),
        ] {
            if edges[edge].is_some_and(|prev: u32| prev.abs_diff(coordinate) > 2) {
                return equal_widths(grid.columns, width);
            }
            edges[edge] = Some(coordinate);
        }
    }
    if let Some(edges) = edges.into_iter().collect::<Option<Vec<_>>>() {
        if edges.windows(2).all(|edge| edge[0] < edge[1]) {
            let first = u64::from(edges[0]);
            let span = u64::from(edges[grid.columns]) - first;
            let scaled: Vec<u32> = edges
                .into_iter()
                .map(|edge| {
                    (((u64::from(edge) - first) * u64::from(width) + span / 2) / span) as u32
                })
                .collect();
            let widths: Vec<_> = scaled.windows(2).map(|edge| edge[1] - edge[0]).collect();
            if widths.iter().all(|&width| width > 0) {
                return widths;
            }
        }
    }
    equal_widths(grid.columns, width)
}

fn equal_widths(columns: usize, width: u32) -> Vec<u32> {
    let width = width.max(columns as u32);
    let base = width / columns as u32;
    let extras = width % columns as u32;
    (0..columns)
        .map(|column| base + u32::from((column as u32) < extras))
        .collect()
}

fn append_matrix(xml: &mut String, matrix: &TableMatrix, columns: usize, geometry: PageGeometry) {
    for start in (0..columns).step_by(WORD_MAX_COLUMNS) {
        if start > 0 {
            xml.push_str("<w:p/>");
        }
        let widths = equal_widths(
            (columns - start).min(WORD_MAX_COLUMNS),
            geometry.content_width(),
        );
        start_table(xml, &widths, 0);
        for row in &matrix.rows {
            xml.push_str("<w:tr>");
            for (offset, &width) in widths.iter().enumerate() {
                let text = row
                    .get(start + offset)
                    .map_or("", |cell| cell.text.as_str());
                append_cell(xml, text, width, 1, None);
            }
            xml.push_str("</w:tr>");
        }
        xml.push_str("</w:tbl>");
    }
}

fn start_table(xml: &mut String, widths: &[u32], indent: u32) {
    let width: u32 = widths.iter().sum();
    write!(xml, concat!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="{}" w:type="dxa"/><w:tblInd w:w="{}" w:type="dxa"/>"#,
        r#"<w:tblBorders><w:top w:val="single" w:sz="4"/><w:left w:val="single" w:sz="4"/>"#,
        r#"<w:bottom w:val="single" w:sz="4"/><w:right w:val="single" w:sz="4"/>"#,
        r#"<w:insideH w:val="single" w:sz="4"/><w:insideV w:val="single" w:sz="4"/></w:tblBorders>"#,
        r#"<w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid>"#,
    ), width, indent).unwrap();
    for width in widths {
        write!(xml, r#"<w:gridCol w:w="{width}"/>"#).unwrap();
    }
    xml.push_str("</w:tblGrid>");
}

fn append_cell(xml: &mut String, text: &str, width: u32, span: usize, merge: Option<&str>) {
    write!(xml, r#"<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="dxa"/>"#).unwrap();
    if span > 1 {
        write!(xml, r#"<w:gridSpan w:val="{span}"/>"#).unwrap();
    }
    if let Some(merge) = merge {
        write!(xml, r#"<w:vMerge w:val="{merge}"/>"#).unwrap();
    }
    xml.push_str("</w:tcPr>");
    for line in text.split('\n') {
        paragraph(xml, line.strip_suffix('\r').unwrap_or(line), false);
    }
    xml.push_str("</w:tc>");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::OcrResult;
    use crate::dto::{BoundingBox as B, DocumentResult, PageKind, TableCell as Cell};

    #[test]
    fn rectangular_merge_continues_with_same_grid_span_and_never_drops_cell_text() {
        let mut merged = Cell::grid(0, 0, B::new(0.0, 0.0, 20.0, 40.0));
        merged.text = "MERGED".to_string();
        merged.row_span = 2;
        merged.col_span = 2;
        let mut cells = Vec::new();
        for row in 0..2 {
            let y = f64::from(row) * 20.0;
            let mut side = Cell::grid(row, 2, B::new(20.0, y, 30.0, y + 20.0));
            side.text = format!("SIDE{row}");
            let empty = |column| Cell {
                row,
                column,
                ..Cell::default()
            };
            cells.push(vec![
                if row == 0 { merged.clone() } else { empty(0) },
                empty(1),
                side,
            ]);
        }
        let matrix = TableMatrix::grid(cells, B::new(0.0, 0.0, 30.0, 40.0), 1.0);
        let page = PageResult::new(1, PageKind::Mixed, String::new(), vec![matrix]);
        let mut result = OcrResult::from_document(DocumentResult::new(vec![page]), vec![]);
        let page = &result.document.pages[0];

        let write_tables = |grids: &[DetectedGrid]| {
            let mut xml = String::new();
            let mut report = WordExportReport::default();
            append_tables(
                &mut xml,
                page,
                Some(grids),
                PageGeometry::default(),
                None,
                WordExportOptions::default(),
                &mut report,
            );
            (xml, report)
        };
        let (xml, report) = write_tables(&result.tables[0].grids);
        assert!(report.warnings.is_empty());
        assert!(xml.contains(r#"<w:gridSpan w:val="2"/><w:vMerge w:val="restart"/>"#));
        assert!(xml.contains(r#"<w:gridSpan w:val="2"/><w:vMerge w:val="continue"/>"#));
        assert_eq!(xml.matches("MERGED").count(), 1);
        assert!(xml.contains("SIDE0") && xml.contains("SIDE1"));

        result.tables[0].grids[0].cells[0].row_span = 99;
        let (xml, report) = write_tables(&result.tables[0].grids);
        assert_eq!(report.warnings.len(), 1);
        assert!(!xml.contains("w:vMerge"));
        assert_eq!(xml.matches("<w:tc>").count(), 6);
        assert!(xml.contains("MERGED") && xml.contains("SIDE0") && xml.contains("SIDE1"));
    }
}
