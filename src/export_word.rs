//! Optional editable Word projection. This is not a facsimile: source graphics
//! are positioned, while OCR text and native Word tables can reflow.

mod graphics;
mod table;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

use zip::write::FileOptions;

use crate::api::OcrResult;
use crate::page_analysis::RenderSize;

/// A rectangle in the displayed page's top-left coordinate system, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WordRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordRasterFormat {
    Png,
    Jpeg,
}

/// A raster picture, optionally with a self-contained vector alternative.
/// Vector pictures must carry a PNG fallback.
#[derive(Debug, Clone, PartialEq)]
pub struct WordGraphic {
    pub rect: WordRect,
    pub raster: Vec<u8>,
    pub raster_format: WordRasterFormat,
    pub svg: Option<Vec<u8>>,
    /// Explicit scan reference. Place inline after the editable page content.
    pub scan_reference: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WordPageGraphics {
    /// Zero-based source page index, not the position in the result vector.
    pub page_index: usize,
    pub width_pt: f32,
    pub height_pt: f32,
    /// Paint order, back to front.
    pub images: Vec<WordGraphic>,
}

pub type WordGraphics = Vec<WordPageGraphics>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WordExportOptions {
    pub include_title_blocks: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct WordExportReport {
    pub warnings: Vec<String>,
}

const XML_DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const OFFICE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const DOCUMENT_START: &str = concat!(
    r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#,
    r#" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#,
    r#" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing""#,
    r#" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#,
    r#" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#,
    r#" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#,
    r#" xmlns:asvg="http://schemas.microsoft.com/office/drawing/2016/SVG/main" mc:Ignorable="asvg"><w:body>"#,
);
const CONTENT_TYPES: &str = concat!(
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
    r#"<Default Extension="xml" ContentType="application/xml"/>"#,
    r#"<Default Extension="png" ContentType="image/png"/>"#,
    r#"<Default Extension="jpg" ContentType="image/jpeg"/>"#,
    r#"<Default Extension="svg" ContentType="image/svg+xml"/>"#,
    r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
    "</Types>",
);

/// Writes an ordinary DOCX with editable text and tables. Page text is never
/// discarded based on processing mode or inferred overlap with a table. The
/// destination must not exist; a failed export does not publish a partial file.
pub fn export_word(
    result: &OcrResult,
    graphics: &[WordPageGraphics],
    output: &Path,
    options: WordExportOptions,
) -> Result<WordExportReport, Box<dyn std::error::Error>> {
    let mut report = WordExportReport::default();
    let mut by_index = BTreeMap::new();
    for page in graphics {
        if let std::collections::btree_map::Entry::Vacant(entry) = by_index.entry(page.page_index) {
            entry.insert(page);
        } else {
            report.warnings.push(format!(
                "Word source page index {}: duplicate graphics ignored",
                page.page_index
            ));
        }
    }
    let mut tables_by_page = BTreeMap::new();
    for page in &result.tables {
        tables_by_page
            .entry(page.page_number)
            .or_insert(&page.grids);
    }
    let mut render_by_page = BTreeMap::new();
    for page in &result.page_processing {
        if let Some(render) = page.render_size {
            render_by_page.entry(page.page_number).or_insert(render);
        }
    }

    let mut package = graphics::Package::new();
    let mut xml = String::from(XML_DECL);
    xml.push_str(DOCUMENT_START);
    for (index, page) in result.document.pages.iter().enumerate() {
        let source = page
            .page_number
            .checked_sub(1)
            .and_then(|source_index| by_index.remove(&(source_index as usize)));
        let render = render_by_page.get(&page.page_number).copied();
        let geometry = PageGeometry::new(page.page_number, source, render, &mut report);

        if let Some(source) = source {
            package.add_graphics(&mut xml, source, geometry, false, &mut report);
        }
        if !page.text.is_empty() {
            for line in page.text.split('\n') {
                paragraph(&mut xml, line.strip_suffix('\r').unwrap_or(line), false);
            }
        }
        table::append_tables(
            &mut xml,
            page,
            tables_by_page
                .get(&page.page_number)
                .map(|grids| grids.as_slice()),
            geometry,
            render,
            options,
            &mut report,
        );
        if let Some(source) = source {
            package.add_graphics(&mut xml, source, geometry, true, &mut report);
        }

        let last = index + 1 == result.document.pages.len();
        if last {
            // Keep an explicit editable paragraph even for a graphics-only or blank page.
            if page.text.is_empty() {
                xml.push_str("<w:p/>");
            }
            geometry.section(&mut xml);
        } else {
            // A section boundary belongs to the page preceding it. Its marker
            // is tiny so it does not unnecessarily advance a filled page.
            xml.push_str(concat!(
                r#"<w:p><w:pPr><w:spacing w:before="0" w:after="0" w:line="20" w:lineRule="exact"/>"#,
                r#"<w:rPr><w:sz w:val="2"/><w:szCs w:val="2"/></w:rPr>"#,
            ));
            geometry.section(&mut xml);
            xml.push_str("</w:pPr></w:p>");
        }
    }
    for page in by_index.into_values() {
        report.warnings.push(format!(
            "Word source page index {}: no matching OCR page; graphics ignored",
            page.page_index
        ));
    }
    if result.document.pages.is_empty() {
        xml.push_str("<w:p/>");
        PageGeometry::default().section(&mut xml);
    }
    xml.push_str("</w:body></w:document>");

    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut zip = zip::ZipWriter::new(temporary.as_file_mut());
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("[Content_Types].xml", options)?;
    zip.write_all(XML_DECL.as_bytes())?;
    zip.write_all(CONTENT_TYPES.as_bytes())?;
    zip.start_file("_rels/.rels", options)?;
    write!(
        zip,
        r#"{XML_DECL}<Relationships xmlns="{PACKAGE_REL}"><Relationship Id="rId1" Type="{OFFICE_REL}/officeDocument" Target="word/document.xml"/></Relationships>"#
    )?;
    zip.start_file("word/document.xml", options)?;
    zip.write_all(xml.as_bytes())?;
    package.write_to_zip(&mut zip)?;
    zip.finish()?;
    drop(zip);
    temporary.persist_noclobber(output)?;
    Ok(report)
}

#[derive(Debug, Clone, Copy)]
struct PageGeometry {
    width: u32,
    height: u32,
    margin_x: u32,
    margin_y: u32,
    scale: f64,
}

impl Default for PageGeometry {
    fn default() -> Self {
        Self::from_points(595.3, 841.9)
    }
}

impl PageGeometry {
    fn new(
        page_number: u32,
        source: Option<&WordPageGraphics>,
        render: Option<RenderSize>,
        report: &mut WordExportReport,
    ) -> Self {
        let dimensions = source
            .map(|p| (f64::from(p.width_pt), f64::from(p.height_pt)))
            .filter(|&(width, height)| {
                width.is_finite() && height.is_finite() && width >= 1.0 && height >= 1.0
            });
        if source.is_some() && dimensions.is_none() {
            report.warnings.push(format!(
                "Word page {page_number}: invalid source size; graphics omitted"
            ));
        }
        let dimensions = dimensions.or_else(|| {
            render
                .filter(|r| r.width > 0 && r.height > 0 && r.dpi > 0)
                .map(|r| {
                    let points_per_pixel = 72.0 / f64::from(r.dpi);
                    (
                        f64::from(r.width) * points_per_pixel,
                        f64::from(r.height) * points_per_pixel,
                    )
                })
        });
        let geometry = dimensions.map_or_else(Self::default, |(w, h)| Self::from_points(w, h));
        if geometry.scale < 1.0 {
            report.warnings.push(format!(
                "Word page {page_number}: source size proportionally reduced to the Word page limit"
            ));
        }
        geometry
    }

    fn from_points(width: f64, height: f64) -> Self {
        // Word page dimensions are limited to 22 inches (1584 pt).
        let scale = (1584.0 / width.max(height)).min(1.0);
        let width = (width * scale * 20.0).round().max(20.0) as u32;
        let height = (height * scale * 20.0).round().max(20.0) as u32;
        Self {
            width,
            height,
            margin_x: (width / 20).min(720),
            margin_y: (height / 20).min(720),
            scale,
        }
    }

    fn content_width(self) -> u32 {
        self.width - 2 * self.margin_x
    }

    fn section(self, xml: &mut String) {
        let Self {
            width,
            height,
            margin_x,
            margin_y,
            ..
        } = self;
        let orient = if width > height {
            "landscape"
        } else {
            "portrait"
        };
        write!(xml, concat!(
            r#"<w:sectPr><w:type w:val="nextPage"/><w:pgSz w:w="{}" w:h="{}" w:orient="{}"/>"#,
            r#"<w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="0" w:footer="0" w:gutter="0"/></w:sectPr>"#,
        ), width, height, orient, margin_y, margin_x, margin_y, margin_x).unwrap();
    }
}

fn paragraph(xml: &mut String, text: &str, bold: bool) {
    xml.push_str(r#"<w:p><w:pPr><w:spacing w:before="0" w:after="0"/></w:pPr>"#);
    if !text.is_empty() {
        xml.push_str("<w:r>");
        if bold {
            xml.push_str("<w:rPr><w:b/></w:rPr>");
        }
        for (index, part) in text.split('\t').enumerate() {
            if index > 0 {
                xml.push_str("<w:tab/>");
            }
            if !part.is_empty() {
                xml.push_str(r#"<w:t xml:space="preserve">"#);
                for ch in part.chars() {
                    match ch {
                        '&' => xml.push_str("&amp;"),
                        '<' => xml.push_str("&lt;"),
                        '>' => xml.push_str("&gt;"),
                        '\r' => xml.push_str("&#13;"),
                        '\u{20}'..='\u{d7ff}'
                        | '\u{e000}'..='\u{fffd}'
                        | '\u{10000}'..='\u{10ffff}' => xml.push(ch),
                        _ => xml.push('\u{fffd}'),
                    }
                }
                xml.push_str("</w:t>");
            }
        }
        xml.push_str("</w:r>");
    }
    xml.push_str("</w:p>");
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Read as _;

    use super::*;
    use crate::dto::{DocumentResult, PageKind, PageResult};

    #[test]
    fn output_uses_source_page_index_and_sizes_and_preserves_existing_destination() {
        let result = OcrResult::from_document(
            DocumentResult::new(vec![
                PageResult::new(1, PageKind::Table, "TABLE & TEXT\tA".to_string(), vec![]),
                PageResult::new(
                    3,
                    PageKind::Text,
                    "THIRD PAGE\nЧертёж №17 — кабель ВВГнг 3×2,5".to_string(),
                    vec![],
                ),
            ]),
            vec![],
        );
        let source = |page_index, width_pt, height_pt| WordPageGraphics {
            page_index,
            width_pt,
            height_pt,
            images: vec![],
        };
        let graphics = vec![source(2, 500.0, 300.0), source(0, 300.0, 700.0)];
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("editable.docx");
        let write = || export_word(&result, &graphics, &output, WordExportOptions::default());
        assert!(write().unwrap().warnings.is_empty());
        let existing = std::fs::read(&output).unwrap();
        assert!(write().is_err());
        assert_eq!(existing, std::fs::read(&output).unwrap());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);

        let mut zip = zip::ZipArchive::new(File::open(output).unwrap()).unwrap();
        let mut xml = String::new();
        zip.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert_eq!(xml.matches("<w:sectPr>").count(), 2);
        let first = xml.find("TABLE &amp; TEXT").unwrap();
        let first_size = xml
            .find(r#"<w:pgSz w:w="6000" w:h="14000" w:orient="portrait"/>"#)
            .unwrap();
        let third = xml.find("THIRD PAGE").unwrap();
        let third_size = xml
            .find(r#"<w:pgSz w:w="10000" w:h="6000" w:orient="landscape"/>"#)
            .unwrap();
        assert!(first < first_size && first_size < third && third < third_size);
        assert!(xml[first..first_size].contains("<w:tab/>"));
        assert!(xml[third..third_size].contains("Чертёж №17 — кабель ВВГнг 3×2,5"));
        assert!(zip.by_name("word/_rels/document.xml.rels").is_ok());
    }
}
