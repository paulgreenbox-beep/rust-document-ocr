//! The opt-in CLI report preserves the ordinary OCR and export contracts.

#![cfg(all(feature = "native", windows))]

mod common;

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;
use zip::ZipArchive;

const DIAGNOSTICS_FILE: &str = "table-ownership-diagnostics.json";

#[test]
fn shadow_report_preserves_native_text_and_raster_exports() {
    let work = tempfile::tempdir().expect("temporary OCR directory");
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/documents");

    assert_shadow_run(
        &fixtures.join("synthetic-text-table.pdf"),
        &work.path().join("text"),
        1,
        "native_text",
    );
    assert_shadow_run(
        &fixtures.join("synthetic-raster.pdf"),
        &work.path().join("raster"),
        1,
        "ocr",
    );

    let multipage = work.path().join("multipage.pdf");
    common::write_text_pdf(
        &multipage,
        &[
            ("Searchable test page one 12345", 0),
            ("Searchable test page two 67890", 0),
        ],
    );
    assert_shadow_run(&multipage, &work.path().join("multipage"), 2, "native_text");
}

fn assert_shadow_run(input: &Path, work: &Path, expected_pages: usize, expected_route: &str) {
    fs::create_dir(work).expect("create output parent");
    let standard = work.join("standard");
    let shadow = work.join("shadow");
    run_ocr(input, &standard, false);
    run_ocr(input, &shadow, true);

    assert!(!standard.join(DIAGNOSTICS_FILE).exists());
    assert_eq!(
        fs::read(standard.join("result.json")).expect("standard legacy JSON"),
        fs::read(shadow.join("result.json")).expect("shadow legacy JSON"),
        "legacy JSON changed for {input:?}",
    );

    let standard_result = read_stable_result(&standard);
    let shadow_result = read_stable_result(&shadow);
    assert_eq!(standard_result, shadow_result, "stable JSON for {input:?}");

    let pages = shadow_result["document"]["pages"]
        .as_array()
        .expect("document pages");
    let processing = shadow_result["page_processing"]
        .as_array()
        .expect("page processing records");
    assert_eq!(pages.len(), expected_pages);
    assert_eq!(processing.len(), expected_pages);

    let report = read_json(&shadow.join(DIAGNOSTICS_FILE));
    assert_eq!(report["schema_version"], 1);
    let diagnostic_pages = report["pages"].as_array().expect("diagnostic page records");
    assert_eq!(diagnostic_pages.len(), expected_pages);
    for (index, diagnostic) in diagnostic_pages.iter().enumerate() {
        let page_number = (index + 1) as u64;
        assert_eq!(diagnostic["page_number"], page_number);
        assert_eq!(pages[index]["page_number"], page_number);
        assert!(!pages[index]["text"].as_str().expect("page text").is_empty());
        let page_processing = &processing[index];
        assert_eq!(page_processing["page_number"], page_number);
        let route = page_processing["processing_mode"]
            .as_str()
            .expect("processing route");
        assert!(
            route == expected_route || route.strip_suffix("+grid") == Some(expected_route),
            "unexpected route {route} for {input:?}",
        );
        assert!(matches!(
            page_processing["quality"]["status"].as_str(),
            Some("reliable" | "needs_review" | "unreliable"),
        ));
        assert_eq!(diagnostic["status"], "observed_no_conflict");
        assert_eq!(diagnostic["evidence_complete"], true);
        assert_eq!(diagnostic["conflict_pairs"], 0);
        assert_eq!(diagnostic["unique_conflicting_words"], 0);
        assert_eq!(diagnostic["additional_memberships"], 0);
        assert!(diagnostic["pairs"]
            .as_array()
            .expect("conflict pairs")
            .is_empty());
        assert!(!diagnostic["reasons"]
            .as_array()
            .expect("ownership reasons")
            .is_empty());
    }

    for (name, main_part) in [
        ("result.docx", "word/document.xml"),
        ("result.xlsx", "xl/workbook.xml"),
    ] {
        let original = read_office_parts(&standard.join(name));
        let diagnosed = read_office_parts(&shadow.join(name));
        assert!(original.contains_key(main_part));
        assert!(original.keys().eq(diagnosed.keys()), "{name} parts changed");
        for (part, contents) in &original {
            assert!(
                contents == &diagnosed[part],
                "{name} part {part} changed for {input:?}",
            );
        }
    }
}

fn run_ocr(input: &Path, output_dir: &Path, diagnostics: bool) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ocr-pipeline"));
    command
        .arg("--input")
        .arg(input)
        .arg("--output-dir")
        .arg(output_dir)
        .arg("--pdfium-dll")
        .arg(common::versioned_pdfium_dll())
        .arg("--tessdata-path")
        .arg(common::bundled_tessdata())
        .args(["--languages", "eng", "--max-page-workers", "2"])
        .env("OMP_THREAD_LIMIT", "1");
    if diagnostics {
        command.arg("--table-ownership-diagnostics");
    }
    let output = command.output().expect("run native OCR CLI");
    assert!(
        output.status.success(),
        "OCR CLI failed for {input:?}:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read JSON artifact")).expect("parse JSON")
}

fn read_stable_result(dir: &Path) -> Value {
    let mut result = read_json(&dir.join("result.v1.json"));
    assert_eq!(result["schema_version"], 1);
    for page in result["page_processing"]
        .as_array_mut()
        .expect("page processing records")
    {
        // Each invocation measures its own wall-clock stage timings.
        let timings = page.as_object_mut().expect("page record").remove("timings");
        assert!(timings.is_some(), "expected ordinary timing metadata");
    }
    result
}

fn read_office_parts(path: &Path) -> BTreeMap<String, Vec<u8>> {
    // Compare every uncompressed part, including package metadata.
    let mut archive = ZipArchive::new(File::open(path).expect("open Office export"))
        .expect("read Office ZIP archive");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut part = archive.by_index(index).expect("read ZIP member");
        let mut bytes = Vec::new();
        part.read_to_end(&mut bytes).expect("read member bytes");
        assert!(parts.insert(part.name().to_owned(), bytes).is_none());
    }
    parts
}
