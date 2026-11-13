#![cfg(all(feature = "native-common", feature = "editable-word"))]

mod common;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use image::GenericImageView;
use ocr_pipeline::{export_pdf_word, process_document, GridKind, TableMode};
use tempfile::tempdir;
use zip::ZipArchive;

/// A deterministic, searchable source document exercises native Word tables
/// alongside PDFium vector and image objects. It is deliberately synthetic:
/// no customer document or system font is required.
#[test]
fn editable_word_export_preserves_content_tables_vectors_and_image_rotation() {
    let directory = fixture_directory();
    let input = directory.path.join("synthetic-word-export.pdf");
    let output = directory.path.join("editable.docx");
    assert_output_absent(&input);
    assert_output_absent(&output);
    write_word_export_fixture(&input);

    let mut options = common::native_options();
    options.table_mode = TableMode::Grid;
    options.enable_cell_ocr_fallback = false;
    options.max_page_workers = 1;

    let result = process_document(&input, &options).expect("process synthetic searchable PDF");
    assert_eq!(result.document.pages.len(), 1);
    let page = &result.document.pages[0];
    for expected in [
        "WORD_EXPORT_OUTSIDE_NOTE",
        "CABINET_101",
        "SENSOR_202",
        "CABLE_303",
        "PUMP_404",
        "VALVE_505",
        "PROJECT_42",
        "DRAWING_17",
    ] {
        assert!(
            page.text.contains(expected),
            "fixture text {expected:?} was not extracted: {:?}",
            page.text
        );
    }

    let grids = result
        .tables
        .iter()
        .find(|tables| tables.page_number == 1)
        .expect("stable page table projection");
    assert!(
        grids
            .grids
            .iter()
            .filter(|grid| grid.kind == GridKind::DataTable)
            .count()
            >= 2,
        "the source fixture should yield two editable body tables: {:?}",
        grids
            .grids
            .iter()
            .map(|grid| grid.kind.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        grids
            .grids
            .iter()
            .any(|grid| grid.kind == GridKind::TitleBlock),
        "the lower-right source table should be classified as a title block"
    );

    let report = export_pdf_word(&input, &options, &result, &output)
        .expect("export editable Word document from PDF");
    assert!(
        report
            .warnings
            .iter()
            .all(|warning| !warning.contains("graphics omitted")),
        "expected source graphics to be retained: {:?}",
        report.warnings
    );

    let mut archive = ZipArchive::new(File::open(&output).expect("open DOCX package"))
        .expect("read DOCX ZIP package");
    let part_names = archive
        .file_names()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let document_xml = read_zip_text(&mut archive, "word/document.xml");
    let relationships_xml = read_zip_text(&mut archive, "word/_rels/document.xml.rels");
    let content_types_xml = read_zip_text(&mut archive, "[Content_Types].xml");

    assert!(document_xml.contains("<w:document"));
    assert!(document_xml.contains("<w:tbl>"));
    assert!(
        document_xml.matches("<w:tbl>").count() >= 3,
        "expected two body tables plus the title block"
    );
    for expected in [
        "WORD_EXPORT_OUTSIDE_NOTE",
        "CABINET_101",
        "SENSOR_202",
        "CABLE_303",
        "PUMP_404",
        "VALVE_505",
        "PROJECT_42",
        "DRAWING_17",
    ] {
        assert!(
            document_xml.contains(expected),
            "DOCX main document lost {expected:?}"
        );
    }
    assert!(document_xml.contains("<w:sectPr>"));
    let extents = word_extents(&document_xml);
    assert!(
        extents.len() == 5,
        "expected two solid paths, one dashed path and two positioned images"
    );
    assert!(extents.iter().all(|(cx, cy)| *cx > 0 && *cy > 0));

    let relationships = relationship_entries(&relationships_xml);
    let relationships_by_id = relationships
        .iter()
        .map(|(id, kind, target)| (id.clone(), (kind.clone(), target.clone())))
        .collect::<BTreeMap<_, _>>();
    let embedded_ids = document_xml
        .split("r:embed=\"")
        .skip(1)
        .filter_map(|tail| tail.split_once('"').map(|(id, _)| id.to_owned()))
        .collect::<BTreeSet<_>>();
    assert!(!embedded_ids.is_empty(), "no embedded OOXML drawings found");
    for id in &embedded_ids {
        assert!(
            relationships_by_id.contains_key(id),
            "drawing relationship {id} is absent from document.xml.rels"
        );
    }

    let image_targets = relationships
        .iter()
        .filter(|(_, kind, _)| kind.ends_with("/image"))
        .map(|(_, _, target)| target.clone())
        .collect::<Vec<_>>();
    assert!(
        image_targets.len() >= 4,
        "expected two PDF images and a vector SVG/fallback pair; found {image_targets:?}"
    );
    for target in &image_targets {
        let part = package_part(target);
        assert!(
            part_names.contains(&part),
            "image relationship target {target:?} does not resolve to {part:?}"
        );
    }

    let svg_parts = image_targets
        .iter()
        .filter(|target| target.to_ascii_lowercase().ends_with(".svg"))
        .map(|target| package_part(target))
        .collect::<Vec<_>>();
    let png_parts = image_targets
        .iter()
        .filter(|target| target.to_ascii_lowercase().ends_with(".png"))
        .map(|target| package_part(target))
        .collect::<Vec<_>>();
    assert!(
        svg_parts.len() == 2,
        "the two solid paths should have SVG parts; the dashed path is raster-only"
    );
    assert!(
        png_parts.len() == 5,
        "expected two raster XObjects, two SVG fallbacks and the dashed path raster"
    );
    assert!(content_types_xml.contains("Extension=\"svg\""));
    assert!(document_xml.contains("<asvg:svgBlip"));
    assert!(
        document_xml.contains("<a:blip"),
        "SVG must retain a raster fallback"
    );

    let mut raster_dimensions = Vec::new();
    for part in &png_parts {
        let mut media = archive.by_name(part).expect("open related PNG media");
        let mut bytes = Vec::new();
        media.read_to_end(&mut bytes).expect("read PNG media");
        let decoded = image::load_from_memory(&bytes).expect("decode PNG media");
        raster_dimensions.push(decoded.dimensions());
    }
    assert!(
        raster_dimensions
            .iter()
            .any(|(width, height)| width > height),
        "expected the unrotated non-square source image: {raster_dimensions:?}"
    );
    assert!(
        raster_dimensions
            .iter()
            .any(|(width, height)| height > width),
        "the 90-degree source CTM should produce an oriented portrait raster: {raster_dimensions:?}"
    );

    let svg = read_zip_text(&mut archive, &svg_parts[0]);
    assert!(svg.contains("<svg"));
    assert!(
        svg.contains("<path"),
        "vector rectangle/line paths were not retained in SVG"
    );
    assert!(
        svg.contains("stroke"),
        "expected visible stroked vector geometry"
    );
}

fn write_word_export_fixture(path: &Path) {
    let mut content = String::from("0 G 1 w\n");
    append_grid(
        &mut content,
        &[50.0, 150.0, 250.0, 350.0, 450.0],
        &[250.0, 330.0, 410.0, 490.0, 570.0],
    );
    append_grid(
        &mut content,
        &[550.0, 650.0, 750.0, 850.0, 950.0],
        &[250.0, 330.0, 410.0, 490.0, 570.0],
    );
    append_grid(
        &mut content,
        &[650.0, 700.0, 750.0, 800.0, 900.0],
        &[760.0, 800.0, 840.0, 880.0, 920.0],
    );
    append_text(&mut content, 50.0, 125.0, 12.0, "WORD_EXPORT_OUTSIDE_NOTE");

    let body_a = [
        ["POSITION", "CODE", "NAME", "QUANTITY"],
        ["1", "CAB-101", "CABINET_101", "2"],
        ["2", "SNS-202", "SENSOR_202", "4"],
        ["3", "CBL-303", "CABLE_303", "10"],
    ];
    append_grid_text(
        &mut content,
        &[50.0, 150.0, 250.0, 350.0, 450.0],
        &[250.0, 330.0, 410.0, 490.0, 570.0],
        &body_a,
        10.0,
    );

    let body_b = [
        ["ITEM", "CODE", "DESCRIPTION", "QTY"],
        ["PUMP_404", "P-404", "PUMP_UNIT", "1"],
        ["VALVE_505", "V-505", "VALVE_UNIT", "2"],
        ["FAN_606", "F-606", "FAN_UNIT", "3"],
    ];
    append_grid_text(
        &mut content,
        &[550.0, 650.0, 750.0, 850.0, 950.0],
        &[250.0, 330.0, 410.0, 490.0, 570.0],
        &body_b,
        10.0,
    );

    let title_block = [
        ["SHEET", "4", "SCALE", "1_50"],
        ["DRAWN", "ALPHA", "DATE", "2026"],
        ["CHECKED", "BETA", "PROJECT_42", "DRAWING_17"],
        ["REVISION_A", "R1", "DOC", "17"],
    ];
    append_grid_text(
        &mut content,
        &[650.0, 700.0, 750.0, 800.0, 900.0],
        &[760.0, 800.0, 840.0, 880.0, 920.0],
        &title_block,
        7.0,
    );

    // Standalone non-table paths ensure that the vector and raster fallback
    // branch is exercised independently of the editable table structures.
    content.push_str("2 w\n400 360 140 60 re S\n400 280 m 650 280 l S\n");
    content.push_str("q [5 3] 2 d 400 220 m 570 220 l S Q\n");
    // The first 4x2 image is horizontal. The second is placed through a 90°
    // CTM; its visible extent is vertical and its pixels must follow suit.
    content.push_str("q 120 0 0 60 50 300 cm /ImA Do Q\n");
    content.push_str("q 0 60 -30 0 250 300 cm /ImB Do Q\n");

    let content_bytes = content.into_bytes();
    let image_a = ascii_hex_image_object(&[
        255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 255, 0, 255, 0, 255, 255, 255, 255, 255, 0,
        0, 0,
    ]);
    let image_b = ascii_hex_image_object(&[
        16, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 17, 34, 51, 68, 85,
        102, 119, 136, 153,
    ]);
    let content_object = stream_object(
        format!("<< /Length {} >>", content_bytes.len()),
        &content_bytes,
    );

    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [6 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
        image_a,
        image_b,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1000 1000] /Resources << /Font << /F1 3 0 R >> /XObject << /ImA 4 0 R /ImB 5 0 R >> >> /Contents 7 0 R >>".to_vec(),
        content_object,
    ];
    write_pdf_objects(path, &objects);
}

struct FixtureDirectory {
    path: PathBuf,
    _temporary: Option<tempfile::TempDir>,
}

fn fixture_directory() -> FixtureDirectory {
    if let Some(path) = std::env::var_os("OCR_WORD_FIXTURE_DIR") {
        let path = PathBuf::from(path);
        assert!(
            path.is_dir(),
            "OCR_WORD_FIXTURE_DIR must name an existing evidence directory"
        );
        FixtureDirectory {
            path,
            _temporary: None,
        }
    } else {
        let temporary = tempdir().expect("temporary fixture directory");
        FixtureDirectory {
            path: temporary.path().to_path_buf(),
            _temporary: Some(temporary),
        }
    }
}

fn assert_output_absent(path: &Path) {
    match std::fs::symlink_metadata(path) {
        Ok(_) => panic!(
            "refusing to overwrite existing fixture output {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("inspect fixture output {}: {error}", path.display()),
    }
}

fn ascii_hex_image_object(rgb: &[u8]) -> Vec<u8> {
    assert_eq!(rgb.len(), 4 * 2 * 3);
    let mut encoded = String::with_capacity(rgb.len() * 2 + 1);
    for byte in rgb {
        write!(&mut encoded, "{byte:02X}").expect("encode image fixture");
    }
    encoded.push('>');
    stream_object(
        format!(
            "<< /Type /XObject /Subtype /Image /Width 4 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /ASCIIHexDecode /Length {} >>",
            encoded.len()
        ),
        encoded.as_bytes(),
    )
}

fn stream_object(dictionary: String, bytes: &[u8]) -> Vec<u8> {
    let mut object = format!("{dictionary}\nstream\n").into_bytes();
    object.extend_from_slice(bytes);
    object.extend_from_slice(b"\nendstream");
    object
}

fn append_grid(content: &mut String, xs: &[f64], top_ys: &[f64]) {
    let x0 = xs[0];
    let x1 = *xs.last().expect("grid has a right edge");
    let top = top_ys[0];
    let bottom = *top_ys.last().expect("grid has a bottom edge");
    for top_y in top_ys {
        let pdf_y = 1_000.0 - top_y;
        writeln!(content, "{x0} {pdf_y} m {x1} {pdf_y} l S").expect("write horizontal rule");
    }
    for x in xs {
        let pdf_top = 1_000.0 - top;
        let pdf_bottom = 1_000.0 - bottom;
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
    let pdf_y = 1_000.0 - top_baseline;
    let escaped = text
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    writeln!(
        content,
        "BT /F1 {size} Tf 1 0 0 1 {x} {pdf_y} Tm ({escaped}) Tj ET"
    )
    .expect("write text object");
}

fn write_pdf_objects(path: &Path, objects: &[Vec<u8>]) {
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
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
    std::fs::write(path, pdf).expect("write synthetic Word-export PDF fixture");
}

fn read_zip_text(archive: &mut ZipArchive<File>, part: &str) -> String {
    let mut file = archive
        .by_name(part)
        .unwrap_or_else(|error| panic!("open DOCX part {part}: {error}"));
    let mut text = String::new();
    file.read_to_string(&mut text)
        .unwrap_or_else(|error| panic!("read DOCX part {part}: {error}"));
    text
}

fn xml_attribute(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let value = tag.split_once(&needle)?.1;
    Some(value.split_once('"')?.0.to_owned())
}

fn relationship_entries(xml: &str) -> Vec<(String, String, String)> {
    xml.split("<Relationship")
        .skip(1)
        .filter_map(|fragment| {
            let tag = fragment.split_once('>')?.0;
            Some((
                xml_attribute(tag, "Id")?,
                xml_attribute(tag, "Type")?,
                xml_attribute(tag, "Target")?,
            ))
        })
        .collect()
}

fn package_part(target: &str) -> String {
    let target = target.trim_start_matches('/');
    if target.starts_with("word/") {
        target.to_owned()
    } else {
        format!("word/{target}")
    }
}

fn word_extents(xml: &str) -> Vec<(u64, u64)> {
    xml.split("<wp:extent")
        .skip(1)
        .filter_map(|fragment| {
            let tag = fragment.split_once('>')?.0;
            Some((
                xml_attribute(tag, "cx")?.parse().ok()?,
                xml_attribute(tag, "cy")?.parse().ok()?,
            ))
        })
        .collect()
}
