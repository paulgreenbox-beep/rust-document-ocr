//! DOCX export via `docx-rs`.

use std::path::Path;

use docx_rs::{AlignmentType, BreakType, Docx, Paragraph, Run};

use crate::dto::{PageKind, PageResult, TableMatrix, TableType};

/// Projection policy for DOCX exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DocxExportOptions {
    /// When false, title blocks remain in JSON/API results but are omitted
    /// from DOCX. When true, they are appended after data tables with an
    /// explicit `Title block` heading.
    pub include_title_blocks: bool,
}

/// Write the OCR results to a `.docx` file at `output`.
///
/// For each [`PageResult`]:
/// - `Text` / `Mixed`: emit a paragraph containing the page text.
/// - `Table`: emit a heading-style paragraph describing each detected
///   table (e.g. `Table 1 (3 rows x 4 columns)`). The structured
///   representation is also emitted as a tab-separated paragraph so the
///   data is not lost; richer table layout is the job of the XLSX
///   exporter.
pub fn export_docx(pages: &[PageResult], output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    export_docx_with_options(pages, output, DocxExportOptions::default())
}

/// Write OCR results to DOCX with an explicit title-block projection policy.
/// Every data table on a page is emitted in reading order (top-to-bottom,
/// then left-to-right), avoiding the previous `first()`-table loss.
#[allow(dead_code)]
#[expect(
    clippy::io_other_error,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
fn export_docx_with_options_legacy(
    pages: &[PageResult],
    output: &Path,
    _options: DocxExportOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut doc = Docx::new();

    for (idx, page) in pages.iter().enumerate() {
        // Page separator between pages.
        if idx > 0 {
            doc = doc.add_paragraph(
                Paragraph::new()
                    .add_run(Run::new().add_break(BreakType::Page))
                    .align(AlignmentType::Left),
            );
        }

        match page.kind {
            PageKind::Text | PageKind::Mixed => {
                let para = if page.text.is_empty() {
                    Paragraph::new()
                } else {
                    Paragraph::new().add_run(Run::new().add_text(page.text.clone()))
                };
                doc = doc.add_paragraph(para);
            }
            PageKind::Table => {
                if let Some(table) = page.tables.first() {
                    let rows = table.rows.len();
                    let cols = table.rows.iter().map(|r| r.len()).max().unwrap_or(0);
                    let header = format!(
                        "Page {} — Table ({} rows x {} columns)",
                        page.page_number, rows, cols
                    );
                    doc = doc.add_paragraph(
                        Paragraph::new()
                            .align(AlignmentType::Left)
                            .add_run(Run::new().add_text(header.clone()).bold()),
                    );
                    for row in &table.rows {
                        let line: Vec<String> = row.iter().map(|c| c.text.clone()).collect();
                        doc = doc.add_paragraph(
                            Paragraph::new().add_run(Run::new().add_text(line.join("\t"))),
                        );
                    }
                } else {
                    doc = doc.add_paragraph(Paragraph::new().add_run(Run::new().add_text(
                        format!("Page {} — table detected but empty", page.page_number),
                    )));
                }
            }
        }
    }

    // If there were no pages at all, emit a single placeholder paragraph
    // so the resulting file is still a valid DOCX.
    if pages.is_empty() {
        doc = doc.add_paragraph(Paragraph::new().add_run(Run::new().add_text("(empty)")));
    }

    let file = std::fs::File::create(output)?;
    doc.build()
        .pack(file)
        .map_err(|e| -> Box<dyn std::error::Error> {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("docx build: {e}"),
            ))
        })?;
    Ok(())
}

/// Write OCR results to DOCX with an explicit title-block projection policy.
/// Every data table on a page is emitted in reading order (top-to-bottom,
/// then left-to-right), avoiding the previous `first()`-table loss.
#[expect(
    clippy::io_other_error,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
pub fn export_docx_with_options(
    pages: &[PageResult],
    output: &Path,
    options: DocxExportOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut doc = Docx::new();

    for (idx, page) in pages.iter().enumerate() {
        if idx > 0 {
            doc = doc.add_paragraph(
                Paragraph::new()
                    .add_run(Run::new().add_break(BreakType::Page))
                    .align(AlignmentType::Left),
            );
        }

        if matches!(page.kind, PageKind::Text | PageKind::Mixed) {
            doc = append_text_lines(doc, &page.text);
        }

        let tables = sorted_tables(page, options);
        if tables.is_empty() && page.kind == PageKind::Table && page.tables.is_empty() {
            doc = doc.add_paragraph(Paragraph::new().add_run(Run::new().add_text(format!(
                "Page {} - table detected but empty",
                page.page_number
            ))));
        }
        for table in tables {
            let label = match table.table_type {
                TableType::DataTable => "Table",
                TableType::TitleBlock => "Title block",
            };
            doc = append_matrix(doc, page.page_number, table, label);
        }
    }

    if pages.is_empty() {
        doc = doc.add_paragraph(Paragraph::new().add_run(Run::new().add_text("(empty)")));
    }

    let file = std::fs::File::create(output)?;
    doc.build()
        .pack(file)
        .map_err(|error| -> Box<dyn std::error::Error> {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("docx build: {error}"),
            ))
        })?;
    Ok(())
}

fn append_text_lines(mut doc: Docx, text: &str) -> Docx {
    if text.is_empty() {
        return doc.add_paragraph(Paragraph::new());
    }
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let paragraph = if line.is_empty() {
            Paragraph::new()
        } else {
            Paragraph::new().add_run(Run::new().add_text(line.to_string()))
        };
        doc = doc.add_paragraph(paragraph);
    }
    doc
}

fn append_matrix(mut doc: Docx, page_number: u32, table: &TableMatrix, label: &str) -> Docx {
    let rows = table.rows.len();
    let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
    let header = format!("Page {page_number} - {label} ({rows} rows x {columns} columns)");
    doc = doc.add_paragraph(
        Paragraph::new()
            .align(AlignmentType::Left)
            .add_run(Run::new().add_text(header).bold()),
    );
    for row in &table.rows {
        let line: Vec<String> = row.iter().map(|cell| cell.text.clone()).collect();
        doc = doc.add_paragraph(Paragraph::new().add_run(Run::new().add_text(line.join("\t"))));
    }
    doc
}

fn sorted_tables(page: &PageResult, options: DocxExportOptions) -> Vec<&TableMatrix> {
    let mut indexed: Vec<(usize, &TableMatrix)> = page
        .tables
        .iter()
        .enumerate()
        .filter(|(_, table)| {
            options.include_title_blocks || table.table_type != TableType::TitleBlock
        })
        .collect();
    indexed.sort_by(|(left_index, left), (right_index, right)| {
        // Title blocks are a separate document projection. Keep all data
        // tables together in reading order, then append title blocks in their
        // own reading order when explicitly requested.
        let left_is_title = left.table_type == TableType::TitleBlock;
        let right_is_title = right.table_type == TableType::TitleBlock;
        let left_y = left
            .bbox
            .as_ref()
            .map(|bbox| bbox.y0)
            .unwrap_or(f64::INFINITY);
        let right_y = right
            .bbox
            .as_ref()
            .map(|bbox| bbox.y0)
            .unwrap_or(f64::INFINITY);
        let left_x = left
            .bbox
            .as_ref()
            .map(|bbox| bbox.x0)
            .unwrap_or(f64::INFINITY);
        let right_x = right
            .bbox
            .as_ref()
            .map(|bbox| bbox.x0)
            .unwrap_or(f64::INFINITY);
        left_is_title
            .cmp(&right_is_title)
            .then(left_y.total_cmp(&right_y))
            .then(left_x.total_cmp(&right_x))
            .then(left_index.cmp(right_index))
    });
    indexed.into_iter().map(|(_, table)| table).collect()
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Read};

    use tempfile::tempdir;
    use zip::ZipArchive;

    use super::{export_docx_with_options, sorted_tables, DocxExportOptions};
    use crate::dto::{BoundingBox, PageKind, PageResult, TableCell, TableMatrix, TableType};

    fn matrix(text: &str, y: f64, table_type: TableType) -> TableMatrix {
        TableMatrix::grid_with_type(
            vec![vec![TableCell::new(text)]],
            BoundingBox::new(10.0, y, 100.0, y + 20.0),
            1.0,
            table_type,
        )
    }

    fn first_cell_texts(tables: &[&TableMatrix]) -> Vec<String> {
        tables
            .iter()
            .map(|table| table.rows[0][0].text.clone())
            .collect()
    }

    fn document_xml(path: &std::path::Path) -> String {
        let file = File::open(path).expect("open DOCX");
        let mut archive = ZipArchive::new(file).expect("open DOCX ZIP");
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .expect("document XML")
            .read_to_string(&mut xml)
            .expect("read document XML");
        xml
    }

    #[test]
    #[expect(
        clippy::cloned_ref_to_slice_refs,
        reason = "Preserve the accepted OCR baseline during source-only publication."
    )]
    fn exports_all_data_tables_in_reading_order_and_omits_title_blocks_by_default() {
        let page = PageResult::new(
            1,
            PageKind::Table,
            String::new(),
            vec![
                matrix("LOWER_DATA", 300.0, TableType::DataTable),
                matrix("TITLE_BLOCK", 25.0, TableType::TitleBlock),
                matrix("UPPER_DATA", 50.0, TableType::DataTable),
            ],
        );
        let dir = tempdir().expect("temporary directory");
        let path = dir.path().join("multiple.docx");
        export_docx_with_options(&[page.clone()], &path, DocxExportOptions::default())
            .expect("export DOCX");
        assert!(std::fs::metadata(&path).expect("DOCX metadata").len() > 0);
        let xml = document_xml(&path);
        let upper = xml.find("UPPER_DATA").expect("upper data in DOCX");
        let lower = xml.find("LOWER_DATA").expect("lower data in DOCX");
        assert!(upper < lower, "data tables must be ordered by bbox");
        assert!(!xml.contains("TITLE_BLOCK"));
        let tables = sorted_tables(&page, DocxExportOptions::default());
        assert_eq!(
            first_cell_texts(&tables),
            vec!["UPPER_DATA".to_string(), "LOWER_DATA".to_string()]
        );
    }

    #[test]
    #[expect(
        clippy::cloned_ref_to_slice_refs,
        reason = "Preserve the accepted OCR baseline during source-only publication."
    )]
    fn title_block_can_be_included_after_data_tables() {
        let page = PageResult::new(
            1,
            PageKind::Table,
            String::new(),
            vec![
                matrix("LOWER_DATA", 300.0, TableType::DataTable),
                matrix("TITLE_BLOCK", 25.0, TableType::TitleBlock),
                matrix("UPPER_DATA", 50.0, TableType::DataTable),
            ],
        );
        let dir = tempdir().expect("temporary directory");
        let path = dir.path().join("with-title-block.docx");
        export_docx_with_options(
            &[page.clone()],
            &path,
            DocxExportOptions {
                include_title_blocks: true,
            },
        )
        .expect("export DOCX");
        assert!(std::fs::metadata(&path).expect("DOCX metadata").len() > 0);
        let xml = document_xml(&path);
        assert!(xml.contains("Title block"));
        assert!(xml.contains("TITLE_BLOCK"));
        assert!(
            xml.find("LOWER_DATA").expect("lower data")
                < xml.find("TITLE_BLOCK").expect("title block"),
            "title block should follow data tables"
        );
        let tables = sorted_tables(
            &page,
            DocxExportOptions {
                include_title_blocks: true,
            },
        );
        assert_eq!(
            first_cell_texts(&tables),
            vec![
                "UPPER_DATA".to_string(),
                "LOWER_DATA".to_string(),
                "TITLE_BLOCK".to_string(),
            ]
        );
    }

    #[test]
    fn mixed_page_keeps_text_and_appends_data_tables() {
        let page = PageResult::new(
            1,
            PageKind::Mixed,
            "NORMAL_TEXT".to_string(),
            vec![matrix("TABLE_TEXT", 10.0, TableType::DataTable)],
        );
        let dir = tempdir().expect("temporary directory");
        let path = dir.path().join("mixed.docx");
        export_docx_with_options(&[page], &path, DocxExportOptions::default())
            .expect("export DOCX");
        assert!(std::fs::metadata(&path).expect("DOCX metadata").len() > 0);
        let xml = document_xml(&path);
        assert!(xml.contains("NORMAL_TEXT"));
        assert!(xml.contains("TABLE_TEXT"));
    }

    #[test]
    fn multiline_page_text_is_exported_as_distinct_docx_paragraphs() {
        let page = PageResult::new(
            1,
            PageKind::Text,
            "FIRST_LINE\nSECOND_LINE".to_string(),
            Vec::new(),
        );
        let dir = tempdir().expect("temporary directory");
        let path = dir.path().join("multiline.docx");
        export_docx_with_options(&[page], &path, DocxExportOptions::default())
            .expect("export DOCX");
        let xml = document_xml(&path);
        let first = xml.find("FIRST_LINE").expect("first line");
        let second = xml.find("SECOND_LINE").expect("second line");
        assert!(first < second);
        assert!(xml[first..second].contains("</w:p>"));
    }
}
