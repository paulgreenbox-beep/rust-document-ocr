//! XLSX export via `umya-spreadsheet`.
//!
//! Data tables become separate `Page_<page_number>` sheets. Title blocks are
//! retained but routed to clearly separate `TitleBlock_<page_number>` sheets
//! so they cannot be confused with specifications or registers.

use std::path::Path;

use umya_spreadsheet::Cell;

use crate::dto::{TableMatrix, TableType};

/// Projection policy for spreadsheet exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XlsxExportOptions {
    /// When false, title blocks stay in JSON/API results but are omitted from
    /// the workbook. When true, each one gets a separate `TitleBlock_<page>`
    /// worksheet and is never mixed with data-table sheets.
    pub include_title_blocks: bool,
}

impl Default for XlsxExportOptions {
    fn default() -> Self {
        Self {
            include_title_blocks: true,
        }
    }
}

/// Write the supplied tables to `output`. The `umya-spreadsheet` API is
/// entirely in-memory; the file is only flushed once at the end.
pub fn export_xlsx(
    tables: &[(u32, &TableMatrix)],
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    export_xlsx_with_options(tables, output, XlsxExportOptions::default())
}

/// Write tables to XLSX with an explicit title-block projection policy.
#[expect(
    clippy::io_other_error,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
pub fn export_xlsx_with_options(
    tables: &[(u32, &TableMatrix)],
    output: &Path,
    options: XlsxExportOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    // Start without Excel's default `Sheet1`: every worksheet in a non-empty
    // export corresponds to one reconstructed table.
    let mut book = umya_spreadsheet::new_file_empty_worksheet();

    // Track each semantic role independently. A title block is deliberately
    // not mixed into the data-table sheet namespace.
    let mut data_per_page_count: std::collections::HashMap<u32, u32> =
        std::collections::HashMap::new();
    let mut title_block_per_page_count: std::collections::HashMap<u32, u32> =
        std::collections::HashMap::new();

    let tables: Vec<_> = tables
        .iter()
        .filter(|(_, matrix)| {
            options.include_title_blocks || matrix.table_type != TableType::TitleBlock
        })
        .collect();

    for (page_number, matrix) in &tables {
        let (prefix, counts) = match matrix.table_type {
            TableType::DataTable => ("Page", &mut data_per_page_count),
            TableType::TitleBlock => ("TitleBlock", &mut title_block_per_page_count),
        };
        let count = counts.entry(*page_number).or_insert(0);
        *count += 1;
        let suffix = if *count == 1 {
            String::new()
        } else {
            format!("_{count}")
        };
        let sheet_name = sanitize_sheet_name(&format!("{prefix}_{page_number}{suffix}"));

        let sheet = book.new_sheet(&sheet_name)?;
        populate_sheet(sheet, matrix)?;
    }

    // `umya-spreadsheet` requires at least one worksheet; if the input
    // was empty, add a placeholder sheet so the file is still valid.
    if tables.is_empty() {
        let sheet = book.new_sheet("Empty")?;
        sheet.get_cell_mut("A1").set_value("No tables detected");
    }

    let path_str = output.to_str().ok_or("output path is not valid UTF-8")?;
    umya_spreadsheet::writer::xlsx::write(&book, path_str).map_err(
        |e| -> Box<dyn std::error::Error> {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("xlsx write: {e}"),
            ))
        },
    )?;
    Ok(())
}

fn populate_sheet(
    sheet: &mut umya_spreadsheet::Worksheet,
    matrix: &TableMatrix,
) -> Result<(), Box<dyn std::error::Error>> {
    for (r, row) in matrix.rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let coord = cell_coord(r, c);
            let target: &mut Cell = sheet.get_cell_mut(&*coord);
            // OCR output is text, even when it happens to look like a number.
            // set_value guesses a cell type and treats strings such as NaN as
            // floating-point values. OOXML does not allow non-finite numeric
            // values, so that produced a workbook which Excel/openpyxl could
            // not read. Keep the recognized value explicitly typed as text.
            target.set_value_string(cell.text.clone());
        }
    }
    Ok(())
}

fn cell_coord(row: usize, col: usize) -> String {
    // Column letters: A, B, ..., Z, AA, AB, ...
    let mut name = String::new();
    let mut n = col;
    loop {
        let rem = n % 26;
        name.insert(0, (b'A' + rem as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    // umya-spreadsheet rows are 1-indexed.
    format!("{name}{}", row + 1)
}

/// Excel sheet names must be ≤ 31 chars and may not contain
/// `: \ / ? * [ ]` (the trailing colon we tolerate is the one in
/// `Page_<n>` itself, which is allowed). We defensively trim and strip
/// forbidden characters.
fn sanitize_sheet_name(raw: &str) -> String {
    let forbidden = [':', '\\', '/', '?', '*', '[', ']'];
    let cleaned: String = raw.chars().filter(|c| !forbidden.contains(c)).collect();
    if cleaned.is_empty() {
        "Sheet".to_string()
    } else {
        cleaned.chars().take(31).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{TableCell, TableMatrix, TableType};
    use std::io::Read;
    use tempfile::tempdir;
    use zip::ZipArchive;

    #[test]
    fn cell_coord_basic() {
        assert_eq!(cell_coord(0, 0), "A1");
        assert_eq!(cell_coord(0, 1), "B1");
        assert_eq!(cell_coord(0, 25), "Z1");
    }

    #[test]
    fn cell_coord_beyond_z() {
        assert_eq!(cell_coord(0, 26), "AA1");
        assert_eq!(cell_coord(0, 27), "AB1");
        assert_eq!(cell_coord(0, 51), "AZ1");
        assert_eq!(cell_coord(0, 52), "BA1");
        assert_eq!(cell_coord(1, 0), "A2");
    }

    #[test]
    fn sanitize_sheet_name_strips_forbidden_chars() {
        assert_eq!(sanitize_sheet_name("Page:1/2?*"), "Page12");
        assert_eq!(sanitize_sheet_name("Sheet [v1]"), "Sheet v1");
    }

    #[test]
    fn sanitize_sheet_name_truncates_to_31() {
        let long = "A".repeat(40);
        let result = sanitize_sheet_name(&long);
        assert!(result.len() <= 31);
    }

    #[test]
    fn sanitize_sheet_name_empty_fallback() {
        assert_eq!(sanitize_sheet_name(":::"), "Sheet");
    }

    #[test]
    fn export_xlsx_writes_valid_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.xlsx");
        let matrix = TableMatrix::new(vec![
            vec![TableCell::new("A"), TableCell::new("B")],
            vec![TableCell::new("C"), TableCell::new("D")],
        ]);
        let tables = vec![(1u32, &matrix)];
        export_xlsx(&tables, &path).unwrap();
        assert!(path.exists());
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
    }

    #[test]
    fn export_xlsx_empty_creates_placeholder() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.xlsx");
        export_xlsx(&[], &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn ocr_text_that_looks_non_finite_stays_a_string() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("non-finite-text.xlsx");
        let matrix = TableMatrix::new(vec![vec![
            TableCell::new("NaN"),
            TableCell::new("INF"),
            TableCell::new("-INF"),
            TableCell::new("00123"),
            TableCell::new("1.20"),
            TableCell::new("1E3"),
        ]]);
        let tables = vec![(24u32, &matrix)];

        export_xlsx(&tables, &path).unwrap();

        let book = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
        let sheet = book.get_sheet_by_name("Page_24").unwrap();
        assert_eq!(sheet.get_value("A1"), "NaN");
        assert_eq!(sheet.get_value("B1"), "INF");
        assert_eq!(sheet.get_value("C1"), "-INF");
        assert_eq!(sheet.get_value("D1"), "00123");
        assert_eq!(sheet.get_value("E1"), "1.20");
        assert_eq!(sheet.get_value("F1"), "1E3");

        let file = std::fs::File::open(&path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut worksheet_xml = String::new();
        archive
            .by_name("xl/worksheets/sheet1.xml")
            .unwrap()
            .read_to_string(&mut worksheet_xml)
            .unwrap();
        assert!(!worksheet_xml.contains("<v>NaN</v>"));
        assert!(!worksheet_xml.contains("<v>INF</v>"));
        assert!(!worksheet_xml.contains("<v>-INF</v>"));
    }

    #[test]
    fn title_blocks_are_separated_from_data_table_sheets() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("typed-tables.xlsx");
        let data_table = TableMatrix::new(vec![vec![TableCell::new("data")]]);
        let title_block = TableMatrix::grid_with_type(
            vec![vec![TableCell::new("title")]],
            crate::dto::BoundingBox::new(0.0, 0.0, 10.0, 10.0),
            1.0,
            TableType::TitleBlock,
        );
        let tables = vec![(1u32, &data_table), (1u32, &title_block)];

        export_xlsx(&tables, &path).unwrap();
        let book = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
        assert!(book.get_sheet_by_name("Page_1").is_some());
        assert!(book.get_sheet_by_name("TitleBlock_1").is_some());
    }

    #[test]
    fn title_blocks_can_be_omitted_from_xlsx_without_losing_data_tables() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("data-only.xlsx");
        let data_table = TableMatrix::new(vec![vec![TableCell::new("data")]]);
        let title_block = TableMatrix::grid_with_type(
            vec![vec![TableCell::new("title")]],
            crate::dto::BoundingBox::new(0.0, 0.0, 10.0, 10.0),
            1.0,
            TableType::TitleBlock,
        );
        let tables = vec![(1u32, &data_table), (1u32, &title_block)];

        export_xlsx_with_options(
            &tables,
            &path,
            XlsxExportOptions {
                include_title_blocks: false,
            },
        )
        .unwrap();
        let book = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
        assert!(book.get_sheet_by_name("Page_1").is_some());
        assert!(book.get_sheet_by_name("TitleBlock_1").is_none());
    }
}
