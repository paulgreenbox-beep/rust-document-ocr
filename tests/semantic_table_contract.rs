use std::io::Read;

use ocr_pipeline::{
    export_ocr_result_xlsx, BoundingBox, ColumnRole, DetectedGrid, DocumentResult, GridKind,
    OcrResult, PageKind, PageResult, SemanticTableType, TableCell, TableMatrix, TableType,
};
use serde::Deserialize;
use tempfile::tempdir;
use zip::ZipArchive;

fn matrix(rows: &[&[&str]]) -> TableMatrix {
    let row_height = 30.0;
    let column_width = 140.0;
    let cells = rows
        .iter()
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
                            column as f64 * column_width,
                            row as f64 * row_height,
                            (column + 1) as f64 * column_width,
                            (row + 1) as f64 * row_height,
                        ),
                    );
                    cell.text = (*text).to_string();
                    cell.confidence = 0.92;
                    cell
                })
                .collect()
        })
        .collect::<Vec<_>>();
    TableMatrix::borderless(
        cells,
        BoundingBox::new(
            0.0,
            0.0,
            rows.first().map_or(0, |row| row.len()) as f64 * column_width,
            rows.len() as f64 * row_height,
        ),
        0.90,
    )
}

fn material_specification() -> TableMatrix {
    matrix(&[
        &["Position", "Code", "Name", "Unit", "Quantity"],
        &["1", "AX-100", "Control cabinet", "pcs", "2"],
        &["2", "BX-200", "Pressure sensor", "pcs", "4"],
    ])
}

fn cable_journal() -> TableMatrix {
    matrix(&[
        &["Cable mark", "From", "To", "Length", "Cross section"],
        &["VVGng-LS", "DB-1", "M-1", "24 m", "3x2.5"],
        &["KVVG", "DB-2", "K-2", "18 m", "5x1.5"],
    ])
}

fn work_estimate() -> TableMatrix {
    matrix(&[
        &["Name", "Unit", "Quantity", "Labor hours", "Amount"],
        &["Cabinet installation", "pcs", "2", "4.5", "12000.00"],
        &["Cable laying", "m", "50", "8.0", "25000.00"],
    ])
}

fn commercial_offer() -> TableMatrix {
    matrix(&[
        &["Name", "Quantity", "Price", "Amount", "VAT"],
        &["Controller", "2", "15000.00 RUB", "30000.00 RUB", "20% VAT"],
        &["Module", "4", "2500.00 RUB", "10000.00 RUB", "20% VAT"],
    ])
}

fn noisy_material_specification() -> TableMatrix {
    matrix(&[
        &["Material specification", "", "", "", ""],
        &["Posltion", "C0de", "Narne", "Unlt", "Quantlty"],
        &["1", "AX-100", "Control cabinet", "pcs", "2"],
        &["2", "BX-200", "Pressure sensor", "pcs", "4"],
    ])
}

fn title_block() -> TableMatrix {
    let mut table = matrix(&[
        &["Sheet", "Stage", "Object"],
        &["1", "P", "Project"],
        &["Rev", "Date", "Signature"],
    ]);
    table.table_type = TableType::TitleBlock;
    table
}

fn weak_table() -> TableMatrix {
    matrix(&[&["note"]])
}

fn result_from_tables(tables: Vec<TableMatrix>) -> OcrResult {
    let pages = tables
        .into_iter()
        .enumerate()
        .map(|(page, table)| {
            PageResult::new(
                page as u32 + 1,
                PageKind::Mixed,
                "page text remains independent from table semantics".to_string(),
                vec![table],
            )
        })
        .collect();
    OcrResult::from_document(DocumentResult::new(pages), Vec::new())
}

#[test]
fn public_result_keeps_kind_and_exposes_additive_semantics() {
    let result = result_from_tables(vec![
        material_specification(),
        cable_journal(),
        work_estimate(),
        commercial_offer(),
        title_block(),
        noisy_material_specification(),
        weak_table(),
    ]);

    let grids = result
        .tables
        .iter()
        .map(|page| &page.grids[0])
        .collect::<Vec<_>>();
    assert_eq!(grids[0].kind, GridKind::DataTable);
    assert_eq!(
        grids[0].semantic_type,
        SemanticTableType::MaterialSpecification
    );
    assert_eq!(grids[1].semantic_type, SemanticTableType::CableJournal);
    assert_eq!(grids[2].semantic_type, SemanticTableType::WorkEstimate);
    assert_eq!(grids[3].semantic_type, SemanticTableType::CommercialOffer);
    assert_eq!(grids[4].kind, GridKind::TitleBlock);
    assert_eq!(grids[4].semantic_type, SemanticTableType::DrawingTitleBlock);
    assert_eq!(grids[5].kind, GridKind::DataTable);
    assert_eq!(
        grids[5].semantic_type,
        SemanticTableType::MaterialSpecification
    );
    assert_eq!(grids[6].kind, GridKind::DataTable);
    assert_eq!(grids[6].semantic_type, SemanticTableType::Unknown);
    assert!(grids[6].semantic_confidence < 0.5);

    assert!(grids[0].header_rows.contains(&0));
    assert!(grids[0]
        .column_roles
        .iter()
        .any(|assignment| assignment.role == ColumnRole::Name));
    assert!(!grids[0].classification_evidence.is_empty());

    let json = serde_json::to_value(&result).expect("serialize public OCR result");
    let table = &json["tables"][0]["grids"][0];
    assert_eq!(table["kind"], "data_table");
    assert_eq!(table["semantic_type"], "material_specification");
    for field in [
        "semantic_confidence",
        "header_rows",
        "column_roles",
        "classification_evidence",
    ] {
        assert!(
            table.get(field).is_some(),
            "missing additive JSON field {field}"
        );
    }
}

#[test]
fn old_grid_json_without_semantic_fields_still_deserializes() {
    let result = result_from_tables(vec![material_specification()]);
    let mut grid = serde_json::to_value(&result.tables[0].grids[0]).expect("serialize grid");
    let object = grid.as_object_mut().expect("grid must be a JSON object");
    object.remove("semantic_type");
    object.remove("semantic_confidence");
    object.remove("header_rows");
    object.remove("column_roles");
    object.remove("classification_evidence");

    let decoded: DetectedGrid = serde_json::from_value(grid).expect("deserialize legacy grid JSON");
    assert_eq!(decoded.kind, GridKind::DataTable);
    assert_eq!(decoded.semantic_type, SemanticTableType::Unknown);
    assert_eq!(decoded.semantic_confidence, 0.0);
    assert!(decoded.header_rows.is_empty());
    assert!(decoded.column_roles.is_empty());
    assert!(decoded.classification_evidence.is_empty());
}

#[derive(Debug, Deserialize)]
struct LegacyEnvelope {
    schema_version: u32,
    tables: Vec<LegacyPageTables>,
}

#[derive(Debug, Deserialize)]
struct LegacyPageTables {
    grids: Vec<LegacyGrid>,
}

#[derive(Debug, Deserialize)]
struct LegacyGrid {
    kind: GridKind,
    rows: usize,
    columns: usize,
}

#[test]
fn legacy_consumer_can_ignore_additive_semantic_fields() {
    let result = result_from_tables(vec![material_specification()]);
    let json = serde_json::to_string(&result).expect("serialize result");
    let legacy: LegacyEnvelope =
        serde_json::from_str(&json).expect("legacy projection ignores new fields");

    assert_eq!(legacy.schema_version, 1);
    assert_eq!(legacy.tables[0].grids[0].kind, GridKind::DataTable);
    assert_eq!(legacy.tables[0].grids[0].rows, 3);
    assert_eq!(legacy.tables[0].grids[0].columns, 5);
}

#[test]
fn legacy_xlsx_path_still_exports_borderless_data_table() {
    let result = result_from_tables(vec![material_specification(), title_block()]);
    let directory = tempdir().expect("create temporary export directory");
    let path = directory.path().join("semantic-contract.xlsx");

    export_ocr_result_xlsx(&result, &path, false).expect("export data table through legacy XLSX");
    assert!(path.is_file());
    assert!(path.metadata().expect("read XLSX metadata").len() > 0);

    let file = std::fs::File::open(&path).expect("open XLSX");
    let mut archive = ZipArchive::new(file).expect("XLSX is a ZIP archive");
    let mut workbook = String::new();
    archive
        .by_name("xl/workbook.xml")
        .expect("XLSX workbook metadata")
        .read_to_string(&mut workbook)
        .expect("read workbook metadata");
    assert!(workbook.contains("Page_1"));
    assert!(!workbook.contains("TitleBlock"));
}
