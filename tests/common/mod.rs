#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn versioned_pdfium_dll() -> PathBuf {
    if let Some(path) = std::env::var_os("OCR_TEST_PDFIUM_LIBRARY") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/pdfium/7881/bin/pdfium.dll")
}

pub fn bundled_tessdata() -> PathBuf {
    if let Some(path) = std::env::var_os("OCR_TEST_TESSDATA") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/tessdata")
}

pub fn native_options() -> ocr_pipeline::OcrOptions {
    ocr_pipeline::OcrOptions {
        pdfium_dll_path: Some(versioned_pdfium_dll()),
        tessdata_path: Some(bundled_tessdata()),
        ..ocr_pipeline::OcrOptions::default()
    }
}

pub fn write_text_pdf(path: &Path, pages: &[(&str, u16)]) {
    let mut objects = Vec::new();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());

    let page_ids = (0..pages.len())
        .map(|index| 4 + index * 2)
        .collect::<Vec<_>>();
    let kids = page_ids
        .iter()
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!(
        "<< /Type /Pages /Kids [{kids}] /Count {} >>",
        pages.len()
    ));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());

    for ((text, rotation), page_id) in pages.iter().zip(page_ids) {
        let content_id = page_id + 1;
        let rotation_entry = if *rotation == 0 {
            String::new()
        } else {
            format!(" /Rotate {rotation}")
        };
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300]{rotation_entry} /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>"
        ));
        let content = format!("BT /F1 18 Tf 120 145 Td ({text}) Tj ET\n");
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ));
    }
    write_objects(path, &objects);
}

pub fn write_grid_text_pdf(path: &Path) {
    let content = concat!(
        "0 G 1 w\n",
        "50 50 m 350 50 l S\n",
        "50 150 m 350 150 l S\n",
        "50 250 m 350 250 l S\n",
        "50 50 m 50 250 l S\n",
        "200 50 m 200 250 l S\n",
        "350 50 m 350 250 l S\n",
        "BT /F1 14 Tf 75 205 Td (Alpha 101) Tj ET\n",
        "BT /F1 14 Tf 225 205 Td (Beta 202) Tj ET\n",
        "BT /F1 14 Tf 75 105 Td (Gamma 303) Tj ET\n",
        "BT /F1 14 Tf 225 105 Td (Delta 404) Tj ET\n",
        "BT /F1 10 Tf 20 280 Td (Outside table note) Tj ET\n",
    );
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
    ];
    write_objects(path, &objects);
}

pub fn write_cropped_text_pdf(path: &Path) {
    let content = "BT /F1 18 Tf 240 195 Td (Crop Box Fixture) Tj ET\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 400] /CropBox [100 50 500 350] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
    ];
    write_objects(path, &objects);
}

fn write_objects(path: &Path, objects: &[String]) {
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
