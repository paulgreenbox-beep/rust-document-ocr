//! Native-text selection regression.
//!
//! This test is intentionally a separate integration-test process: PDFium's
//! high-level binding allows only one initialization per process, while the
//! library unit tests also exercise direct rendering.

#![cfg(feature = "native")]

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use ocr_pipeline::{process_document, OcrOptions, ProcessingMode, TableMode};

#[test]
fn full_multipage_pdf_finishes_sequentially_and_matches_single_pages() {
    const PAGE_COUNT: usize = 35;
    let dir = tempfile::tempdir().expect("temporary directory");
    let input = dir.path().join("native-text-35-pages.pdf");
    let labels = (1..=PAGE_COUNT).collect::<Vec<_>>();
    write_text_pdf(&input, &labels);

    let started = Instant::now();
    let result = process_document(&input, &native_text_options())
        .expect("35-page searchable PDF should process without raster OCR");

    assert!(
        started.elapsed() < Duration::from_secs(20),
        "synthetic multi-page PDF must not silently hang"
    );
    assert_eq!(result.document.pages.len(), PAGE_COUNT);
    assert_eq!(result.page_processing.len(), PAGE_COUNT);
    for (index, (page, processing)) in result
        .document
        .pages
        .iter()
        .zip(&result.page_processing)
        .enumerate()
    {
        let page_number = index + 1;
        assert_eq!(page.page_number, page_number as u32);
        assert!(page.text.contains(&format!("Page {page_number:03}")));
        assert_eq!(processing.page_number, page_number as u32);
        assert_eq!(processing.processing_mode, ProcessingMode::NativeText);
        assert!(processing.render_size.is_none());
        assert!(processing.ocr_confidence.is_none());
        assert!(processing
            .native_text_quality
            .as_ref()
            .is_some_and(|quality| quality.is_usable));

        let individual = dir.path().join(format!("native-text-{page_number:03}.pdf"));
        write_text_pdf(&individual, &[page_number]);
        let standalone_text = process_single_page_in_child(&individual);
        assert_eq!(
            page.text, standalone_text,
            "full-document page {page_number} must match its standalone equivalent"
        );
    }
}

#[test]
fn standalone_native_text_page_child() {
    let Ok(input) = std::env::var("OCR_NATIVE_TEXT_CHILD_INPUT") else {
        return;
    };
    let result = process_document(Path::new(&input), &native_text_options())
        .expect("one-page child input should process");
    assert_eq!(result.document.pages.len(), 1);
    println!(
        "OCR_CHILD_TEXT_JSON={}",
        serde_json::to_string(&result.document.pages[0].text).expect("serialize child text")
    );
}

fn process_single_page_in_child(input: &Path) -> String {
    let output = Command::new(std::env::current_exe().expect("current test executable"))
        .arg("--exact")
        .arg("standalone_native_text_page_child")
        .arg("--nocapture")
        .env("OCR_NATIVE_TEXT_CHILD_INPUT", input)
        .output()
        .expect("start isolated one-page test process");
    assert!(
        output.status.success(),
        "standalone child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("child stdout is UTF-8");
    let encoded = stdout
        .lines()
        .find_map(|line| line.strip_prefix("OCR_CHILD_TEXT_JSON="))
        .expect("child must emit the extracted page text");
    serde_json::from_str(encoded).expect("child text JSON")
}
fn native_text_options() -> OcrOptions {
    OcrOptions {
        pdfium_dll_path: Some(versioned_pdfium_dll()),
        tessdata_path: Some(bundled_tessdata()),
        table_mode: TableMode::Off,
        max_page_workers: 1,
        ..OcrOptions::default()
    }
}

fn versioned_pdfium_dll() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("runtime")
        .join("pdfium")
        .join("7881")
        .join("bin")
        .join("pdfium.dll")
}

fn bundled_tessdata() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("runtime")
        .join("tessdata")
}

fn write_text_pdf(path: &Path, page_labels: &[usize]) {
    assert!(!page_labels.is_empty());
    let page_count = page_labels.len();
    let font_object = 3 + page_count * 2;
    let page_object_numbers = (0..page_count)
        .map(|index| 3 + index * 2)
        .collect::<Vec<_>>();
    let kids = page_object_numbers
        .iter()
        .map(|object| format!("{object} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");

    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {page_count} >>"),
    ];
    for (label, page_object) in page_labels.iter().zip(&page_object_numbers) {
        let content_object = page_object + 1;
        let content = format!(
            "BT /F1 18 Tf 20 50 Td (Page {label:03}: Native Text Fixture searchable PDF regression 12345) Tj ET\n"
        );
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 100] /Resources << /Font << /F1 {font_object} 0 R >> >> /Contents {content_object} 0 R >>"
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());

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
    std::fs::write(path, pdf).expect("write synthetic multi-page native-text PDF");
}
