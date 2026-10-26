//! End-to-end smoke test: build a small page, push it through the
//! pipeline with stub adapters, and verify the JSON and DOCX artifacts.

use std::path::{Path, PathBuf};
use std::process::Command;

use image::{GrayImage, Luma};
use tempfile::tempdir;

use ocr_pipeline::dto::{DocumentResult, PageKind};
use ocr_pipeline::export_docx::export_docx;
use ocr_pipeline::export_json::export_json;
use ocr_pipeline::ocr::StubOcrEngine;
use ocr_pipeline::page_source::StubPageSource;
use ocr_pipeline::pipeline::run as run_pipeline;
use ocr_pipeline::preprocessing::DEFAULT_MAX_PIXELS;

fn make_page() -> GrayImage {
    // 100x100 white image with a small black rectangle. Even if the
    // detector evolves, this fixture is well below any sensible
    // size limit and is cheap to allocate.
    let mut img = GrayImage::from_pixel(100, 100, Luma([255]));
    for y in 20..40 {
        for x in 20..40 {
            img.put_pixel(x, y, Luma([0]));
        }
    }
    img
}

#[test]
#[expect(
    clippy::bool_comparison,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
fn pipeline_runs_one_page_and_writes_artifacts() {
    let dir = tempdir().expect("create tempdir");
    let img = make_page();
    let mut source = StubPageSource::new(vec![img]);
    let ocr = StubOcrEngine::new();

    let result: DocumentResult =
        run_pipeline(&mut source, &ocr, DEFAULT_MAX_PIXELS, None).expect("pipeline run");

    assert_eq!(result.pages.len(), 1, "expected exactly one page");
    let page = &result.pages[0];
    assert_eq!(page.page_number, 1);
    assert_eq!(page.kind, PageKind::Text);
    assert!(
        !page.tables.is_empty() == false,
        "stub detector reports no tables"
    );
    assert!(!page.text.is_empty(), "stub OCR should return some text");

    // Export JSON
    let json_path = dir.path().join("result.json");
    export_json(&result, &json_path).expect("export json");
    assert!(Path::new(&json_path).exists(), "JSON file should exist");
    let json_bytes = std::fs::read(&json_path).expect("read json");
    let json_str = std::str::from_utf8(&json_bytes).expect("utf-8");
    assert!(json_str.contains("\"page_number\": 1"));
    assert!(json_str.contains("\"kind\": \"Text\""));

    // Export DOCX
    let docx_path = dir.path().join("result.docx");
    export_docx(&result.pages, &docx_path).expect("export docx");
    assert!(Path::new(&docx_path).exists(), "DOCX file should exist");
    let meta = std::fs::metadata(&docx_path).expect("stat docx");
    assert!(meta.len() > 0, "DOCX must not be empty");
}

#[test]
fn empty_document_writes_valid_artifacts() {
    let dir = tempdir().expect("create tempdir");
    let mut source = StubPageSource::new(Vec::new());
    let ocr = StubOcrEngine::new();

    let result = run_pipeline(&mut source, &ocr, DEFAULT_MAX_PIXELS, None)
        .expect("pipeline run on empty source");
    assert!(result.pages.is_empty());

    let json_path = dir.path().join("empty.json");
    export_json(&result, &json_path).expect("export empty json");
    let body = std::fs::read_to_string(&json_path).expect("read empty json");
    assert!(body.contains("\"pages\": []"));

    let docx_path = dir.path().join("empty.docx");
    export_docx(&result.pages, &docx_path).expect("export empty docx");
    assert!(docx_path.exists());
}

#[test]
fn pipeline_respects_page_limit() {
    // Create a 5-page stub source
    let pages: Vec<GrayImage> = (0..5)
        .map(|i| GrayImage::from_pixel(10, 10, Luma([i as u8 * 50])))
        .collect();
    let mut source = StubPageSource::new(pages);
    let ocr = StubOcrEngine::new();

    // Limit to 3 pages
    let result = run_pipeline(&mut source, &ocr, DEFAULT_MAX_PIXELS, Some(3))
        .expect("pipeline run with page_limit");
    assert_eq!(result.pages.len(), 3, "should process exactly 3 pages");
    assert_eq!(result.pages[0].page_number, 1);
    assert_eq!(result.pages[2].page_number, 3);
}

#[test]
fn pipeline_page_limit_clamps_to_total() {
    // 2 pages available, limit 10 → should process 2
    let pages: Vec<GrayImage> = (0..2)
        .map(|_| GrayImage::from_pixel(10, 10, Luma([255])))
        .collect();
    let mut source = StubPageSource::new(pages);
    let ocr = StubOcrEngine::new();

    let result =
        run_pipeline(&mut source, &ocr, DEFAULT_MAX_PIXELS, Some(10)).expect("pipeline run");
    assert_eq!(result.pages.len(), 2, "should not exceed available pages");
}

// ---------------------------------------------------------------------------
// CLI smoke tests driven via the binary built by `cargo test`.
// These tests intentionally target the default (non-`native`) build so they
// run in any environment. They use the manifest dir to locate the input file
// and a tempdir for the output directory so they do not depend on cwd.
// ---------------------------------------------------------------------------

/// Returns the path to the binary built for these integration tests.
fn ocr_pipeline_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ocr-pipeline"))
}

#[test]
fn cli_parallelism_zero_rejects_without_creating_output_dir() {
    let bin = ocr_pipeline_bin();
    let workdir = tempdir().expect("create tempdir");
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    assert!(
        input.is_file(),
        "fixture input must exist: {}",
        input.display()
    );
    // Use a path inside the tempdir that is guaranteed not to exist yet.
    let output_dir = workdir.path().join("should-not-be-created");

    let output = Command::new(&bin)
        .current_dir(workdir.path())
        .arg("--input")
        .arg(&input)
        .arg("--output-dir")
        .arg(&output_dir)
        .arg("--parallelism")
        .arg("0")
        .output()
        .expect("failed to execute ocr-pipeline binary");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit code 1, got status={:?}\nstderr=\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Configuration error") || stderr.contains("parallelism"),
        "stderr should explain the configuration error, got:\n{}",
        stderr
    );
    assert!(
        !output_dir.exists(),
        "output-dir must NOT be created when configuration is rejected, but {} exists",
        output_dir.display()
    );
}

#[test]
#[cfg(not(feature = "native"))]
fn cli_valid_input_without_native_feature_reports_native_error() {
    let bin = ocr_pipeline_bin();
    let workdir = tempdir().expect("create tempdir");
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    assert!(
        input.is_file(),
        "fixture input must exist: {}",
        input.display()
    );
    let output_dir = workdir.path().join("also-should-not-exist");

    let output = Command::new(&bin)
        .current_dir(workdir.path())
        .arg("--input")
        .arg(&input)
        .arg("--output-dir")
        .arg(&output_dir)
        .arg("--parallelism")
        .arg("1")
        .output()
        .expect("failed to execute ocr-pipeline binary");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit code 1 in non-native build, got status={:?}\nstderr=\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("native") || stderr.contains("--features native"),
        "stderr should mention the missing 'native' feature, got:\n{}",
        stderr
    );
    assert!(
        !output_dir.exists(),
        "output-dir must NOT be created when the native feature is missing, but {} exists",
        output_dir.display()
    );
}
