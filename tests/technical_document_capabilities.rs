#![cfg(feature = "native-common")]

mod common;

use std::{fmt::Write as _, io::Read};

use ocr_pipeline::{
    export_ocr_result_docx, export_ocr_result_json, export_ocr_result_xlsx, process_document,
    GridKind, OcrOptions, PageKind, ProcessingMode, TableMode, TableType,
};
use tempfile::tempdir;
use zip::ZipArchive;

const PAGE_SIZE: f64 = 1_000.0;

/// Exercises real PDFium text extraction, grid reconstruction, cell ownership,
/// and public projections on a synthetic searchable PDF. This is a structural
/// control, not a stamp-accuracy or raster-OCR benchmark.
#[test]
fn searchable_technical_page_preserves_native_text_and_projects_title_block() {
    let directory = tempdir().expect("temporary fixture directory");
    let input = directory.path().join("synthetic-technical-page.pdf");
    write_capability_pdf(&input);

    let mut options: OcrOptions = common::native_options();
    options.table_mode = TableMode::Grid;
    options.enable_cell_ocr_fallback = false;
    options.max_page_workers = 1;

    let result = process_document(&input, &options).expect("process synthetic searchable PDF");
    assert_eq!(result.document.pages.len(), 1);
    assert_eq!(result.page_processing.len(), 1);
    assert_eq!(
        result.page_processing[0].processing_mode,
        ProcessingMode::NativeTextWithGrid
    );

    let page = &result.document.pages[0];
    assert_eq!(page.kind, PageKind::Mixed);
    let full_page_text = &page.text;
    for expected in PAGE_TEXT_TERMS {
        assert!(
            full_page_text.contains(expected),
            "native page text lost {expected:?}: {full_page_text:?}"
        );
    }

    let body = page
        .tables
        .iter()
        .find(|table| {
            table.table_type == TableType::DataTable
                && table.bbox.as_ref().is_some_and(|bbox| bbox.x0 < 800.0)
        })
        .expect("body data grid");
    let title = page
        .tables
        .iter()
        .find(|table| table.table_type == TableType::TitleBlock)
        .expect("lower-right title-block grid");
    assert!(body.confidence > 0.0 && body.confidence <= 1.0);
    assert!(title.confidence > 0.0 && title.confidence <= 1.0);
    assert!(body
        .rows
        .iter()
        .flatten()
        .any(|cell| cell.text == "CABINET_101"));
    assert!(title.rows.iter().flatten().any(|cell| cell.text == "SHEET"));
    assert!(title.rows.iter().flatten().any(|cell| cell.text == "SCALE"));

    let stable_page = result
        .tables
        .iter()
        .find(|tables| tables.page_number == 1)
        .expect("stable page table projection");
    let stable_body = stable_page
        .grids
        .iter()
        .find(|grid| grid.kind == GridKind::DataTable)
        .expect("stable body grid");
    let stable_title = stable_page
        .grids
        .iter()
        .find(|grid| grid.kind == GridKind::TitleBlock)
        .expect("stable title-block grid");
    assert_eq!((stable_body.rows, stable_body.columns), (4, 4));
    assert_eq!((stable_title.rows, stable_title.columns), (4, 4));
    assert!(stable_title.bbox.width > 0 && stable_title.bbox.height > 0);
    assert!(stable_title.confidence > 0.0 && stable_title.confidence <= 1.0);
    assert!(stable_title.cells.iter().any(|cell| {
        cell.text == "SHEET" && cell.bbox.width > 0 && cell.bbox.height > 0 && cell.confidence > 0.0
    }));
    let render_size = result.page_processing[0]
        .render_size
        .expect("grid render dimensions");
    assert!(stable_title.bbox.x > render_size.width / 2);
    assert!(stable_title.bbox.y > render_size.height * 7 / 10);
    assert!(stable_body.bbox.x < stable_title.bbox.x);

    let json_path = directory.path().join("result.v1.json");
    export_ocr_result_json(&result, &json_path).expect("JSON export");
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&json_path).expect("read JSON export"))
            .expect("parse JSON export");
    let json_grids = json["tables"][0]["grids"]
        .as_array()
        .expect("JSON grid list");
    let json_title = json_grids
        .iter()
        .find(|grid| grid["kind"] == "title_block")
        .expect("JSON retains title block");
    assert!(json_title["bbox"]["width"].as_u64().unwrap_or_default() > 0);
    assert!(json_title["confidence"].as_f64().unwrap_or_default() > 0.0);

    let docx_with_title = directory.path().join("with-title-block.docx");
    export_ocr_result_docx(&result, &docx_with_title, true).expect("DOCX with title block");
    let docx_with_title_xml = zip_entry(&docx_with_title, "word/document.xml");
    assert!(docx_with_title_xml.contains("Page 1 - Title block"));
    assert!(docx_with_title_xml.contains("OUTSIDE_NOTE_TOKEN"));

    let docx_without_title = directory.path().join("without-title-block.docx");
    export_ocr_result_docx(&result, &docx_without_title, false)
        .expect("DOCX without title-block table projection");
    let docx_without_title_xml = zip_entry(&docx_without_title, "word/document.xml");
    assert!(docx_without_title_xml.contains("OUTSIDE_NOTE_TOKEN"));
    assert!(!docx_without_title_xml.contains("Page 1 - Title block"));
    // The table option omits the separate title-block table projection, while
    // the complete native page text remains in the mixed-page text section.
    assert!(docx_without_title_xml.contains("SHEET"));

    let xlsx_with_title = directory.path().join("with-title-block.xlsx");
    export_ocr_result_xlsx(&result, &xlsx_with_title, true).expect("XLSX with title block");
    let xlsx_with_title_workbook = zip_entry(&xlsx_with_title, "xl/workbook.xml");
    assert!(xlsx_with_title_workbook.contains("Page_1"));
    assert!(xlsx_with_title_workbook.contains("TitleBlock_1"));
    let xlsx_with_title_book =
        umya_spreadsheet::reader::xlsx::read(&xlsx_with_title).expect("read XLSX with title");
    assert_eq!(
        xlsx_with_title_book
            .get_sheet_by_name("Page_1")
            .expect("data worksheet")
            .get_value("C2"),
        "CABINET_101"
    );
    assert_eq!(
        xlsx_with_title_book
            .get_sheet_by_name("TitleBlock_1")
            .expect("title-block worksheet")
            .get_value("A1"),
        "SHEET"
    );

    let xlsx_without_title = directory.path().join("without-title-block.xlsx");
    export_ocr_result_xlsx(&result, &xlsx_without_title, false)
        .expect("XLSX without title-block table projection");
    let xlsx_without_title_workbook = zip_entry(&xlsx_without_title, "xl/workbook.xml");
    assert!(xlsx_without_title_workbook.contains("Page_1"));
    assert!(!xlsx_without_title_workbook.contains("TitleBlock"));
    let xlsx_without_title_book =
        umya_spreadsheet::reader::xlsx::read(&xlsx_without_title).expect("read data-only XLSX");
    assert_eq!(
        xlsx_without_title_book
            .get_sheet_by_name("Page_1")
            .expect("data worksheet")
            .get_value("C2"),
        "CABINET_101"
    );
    assert!(xlsx_without_title_book
        .get_sheet_by_name("TitleBlock_1")
        .is_none());
}

const PAGE_TEXT_TERMS: &[&str] = &[
    "OUTSIDE_NOTE_TOKEN",
    "POSITION",
    "CODE",
    "NAME",
    "QUANTITY",
    "CABINET_101",
    "SENSOR_202",
    "CABLE_303",
    "SHEET",
    "SCALE",
    "DRAWN",
    "CHECKED",
    "DATE",
    "PROJECT_42",
    "DRAWING_17",
    "REVISION_A",
];

fn write_capability_pdf(path: &std::path::Path) {
    let mut content = String::from("0 G 0.8 w\n");
    append_grid(
        &mut content,
        &[50.0, 175.0, 300.0, 425.0, 550.0],
        &[250.0, 330.0, 410.0, 490.0, 570.0],
    );
    append_grid(
        &mut content,
        &[650.0, 700.0, 750.0, 800.0, 900.0],
        &[760.0, 800.0, 840.0, 880.0, 920.0],
    );
    append_text(&mut content, 50.0, 150.0, 12.0, "OUTSIDE_NOTE_TOKEN");

    let body = [
        ["POSITION", "CODE", "NAME", "QUANTITY"],
        ["1", "CAB-101", "CABINET_101", "2"],
        ["2", "SNS-202", "SENSOR_202", "4"],
        ["3", "CBL-303", "CABLE_303", "10"],
    ];
    append_grid_text(
        &mut content,
        &[50.0, 175.0, 300.0, 425.0, 550.0],
        &[250.0, 330.0, 410.0, 490.0, 570.0],
        &body,
        10.0,
    );

    let title = [
        ["SHEET", "4", "SCALE", "1_50"],
        ["DRAWN", "ALPHA", "DATE", "2026"],
        ["CHECKED", "BETA", "PROJECT_42", "DRAWING_17"],
        ["REVISION_A", "R1", "DOC", "17"],
    ];
    append_grid_text(
        &mut content,
        &[650.0, 700.0, 750.0, 800.0, 900.0],
        &[760.0, 800.0, 840.0, 880.0, 920.0],
        &title,
        7.0,
    );

    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1000 1000] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_string(),
        format!(
            "<< /Length {} >>\nstream\n{}endstream",
            content.len(),
            content
        ),
    ];
    write_pdf_objects(path, &objects);
}

fn append_grid(content: &mut String, xs: &[f64], top_ys: &[f64]) {
    let x0 = xs[0];
    let x1 = *xs.last().expect("grid has a right edge");
    let top = top_ys[0];
    let bottom = *top_ys.last().expect("grid has a bottom edge");
    for top_y in top_ys {
        let pdf_y = PAGE_SIZE - top_y;
        writeln!(content, "{x0} {pdf_y} m {x1} {pdf_y} l S").expect("write horizontal rule");
    }
    for x in xs {
        let pdf_top = PAGE_SIZE - top;
        let pdf_bottom = PAGE_SIZE - bottom;
        writeln!(content, "{x} {pdf_bottom} m {x} {pdf_top} l S").expect("write vertical rule");
    }
}

fn append_grid_text<const ROWS: usize, const COLUMNS: usize>(
    content: &mut String,
    xs: &[f64],
    top_ys: &[f64],
    cells: &[[&str; COLUMNS]; ROWS],
    font_size: f64,
) {
    for (row, values) in cells.iter().enumerate() {
        let top = top_ys[row];
        let bottom = top_ys[row + 1];
        let baseline_from_top = top + (bottom - top) * 0.62;
        for (column, text) in values.iter().enumerate() {
            append_text(
                content,
                xs[column] + 4.0,
                baseline_from_top,
                font_size,
                text,
            );
        }
    }
}

fn append_text(content: &mut String, x: f64, top_baseline: f64, size: f64, text: &str) {
    let pdf_y = PAGE_SIZE - top_baseline;
    writeln!(
        content,
        "BT /F1 {size} Tf 1 0 0 1 {x} {pdf_y} Tm ({text}) Tj ET"
    )
    .expect("write text object");
}

fn write_pdf_objects(path: &std::path::Path, objects: &[String]) {
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }
    let xref_offset = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    std::fs::write(path, pdf).expect("write synthetic PDF fixture");
}

fn zip_entry(path: &std::path::Path, name: &str) -> String {
    let file = std::fs::File::open(path).expect("open exported archive");
    let mut archive = ZipArchive::new(file).expect("read exported archive");
    let mut contents = String::new();
    archive
        .by_name(name)
        .expect("expected archive entry")
        .read_to_string(&mut contents)
        .expect("read archive entry");
    contents
}
