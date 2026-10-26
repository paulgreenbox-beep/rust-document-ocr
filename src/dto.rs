//! Domain DTOs shared by every layer of the pipeline.
//!
//! These types are part of the public API contract — other workers and
//! downstream consumers depend on the exact field names and shapes here.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoundingBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl BoundingBox {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { x0, y0, x1, y1 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub bbox: BoundingBox,
    pub confidence: f32,
}

impl Word {
    pub fn new(text: impl Into<String>, bbox: BoundingBox, confidence: f32) -> Self {
        Self {
            text: text.into(),
            bbox,
            confidence,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PageKind {
    Text,
    Table,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TableDetectorType {
    /// A table reconstructed from verified horizontal and vertical grid lines.
    Grid,
    /// A conservative table reconstructed from repeated word alignment without
    /// requiring drawn cell borders.
    Borderless,
    /// Legacy fallback: OCR word coordinates grouped by alignment heuristics.
    #[default]
    WordAlignmentFallback,
}

/// Structural role of a geometrically reconstructed table-like region.
///
/// A title block is preserved in JSON but deliberately kept separate from
/// data tables in spreadsheet exports, so it cannot be mistaken for a
/// specification or a register. Subject-domain semantics are exposed
/// separately by the stable API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TableType {
    #[default]
    DataTable,
    TitleBlock,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableCell {
    pub text: String,
    #[serde(default)]
    pub row: u32,
    #[serde(default)]
    pub column: u32,
    #[serde(default = "default_span")]
    pub row_span: u32,
    #[serde(default = "default_span")]
    pub col_span: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BoundingBox>,
    #[serde(default)]
    pub confidence: f32,
}

impl TableCell {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            row: 0,
            column: 0,
            row_span: 1,
            col_span: 1,
            bbox: None,
            confidence: 0.0,
        }
    }

    pub fn grid(row: u32, column: u32, bbox: BoundingBox) -> Self {
        Self {
            text: String::new(),
            row,
            column,
            row_span: 1,
            col_span: 1,
            bbox: Some(bbox),
            confidence: 0.0,
        }
    }
}

impl Default for TableCell {
    fn default() -> Self {
        Self::new("")
    }
}

const fn default_span() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::{TableCell, TableMatrix, TableType};

    #[test]
    fn default_table_cell_has_unit_spans() {
        let cell = TableCell::default();
        assert_eq!(cell.row_span, 1);
        assert_eq!(cell.col_span, 1);
    }

    #[test]
    fn new_matrix_defaults_to_data_table() {
        assert_eq!(
            TableMatrix::new(Vec::new()).table_type,
            TableType::DataTable
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TableMatrix {
    pub rows: Vec<Vec<TableCell>>,
    #[serde(default)]
    pub detector_type: TableDetectorType,
    #[serde(default)]
    pub table_type: TableType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BoundingBox>,
    #[serde(default)]
    pub confidence: f32,
}

impl TableMatrix {
    pub fn new(rows: Vec<Vec<TableCell>>) -> Self {
        Self {
            rows,
            detector_type: TableDetectorType::WordAlignmentFallback,
            table_type: TableType::DataTable,
            bbox: None,
            confidence: 0.0,
        }
    }

    pub fn grid(rows: Vec<Vec<TableCell>>, bbox: BoundingBox, confidence: f32) -> Self {
        Self::grid_with_type(rows, bbox, confidence, TableType::DataTable)
    }

    pub fn grid_with_type(
        rows: Vec<Vec<TableCell>>,
        bbox: BoundingBox,
        confidence: f32,
        table_type: TableType,
    ) -> Self {
        Self {
            rows,
            detector_type: TableDetectorType::Grid,
            table_type,
            bbox: Some(bbox),
            confidence,
        }
    }

    /// Construct a confidently inferred borderless data table while retaining
    /// the legacy matrix shape used by existing XLSX/DOCX exporters.
    pub fn borderless(rows: Vec<Vec<TableCell>>, bbox: BoundingBox, confidence: f32) -> Self {
        Self {
            rows,
            detector_type: TableDetectorType::Borderless,
            table_type: TableType::DataTable,
            bbox: Some(bbox),
            confidence: confidence.clamp(0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageResult {
    pub page_number: u32,
    pub kind: PageKind,
    pub text: String,
    pub tables: Vec<TableMatrix>,
}

impl PageResult {
    pub fn new(page_number: u32, kind: PageKind, text: String, tables: Vec<TableMatrix>) -> Self {
        Self {
            page_number,
            kind,
            text,
            tables,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentResult {
    pub pages: Vec<PageResult>,
}

impl DocumentResult {
    pub fn new(pages: Vec<PageResult>) -> Self {
        Self { pages }
    }
}
