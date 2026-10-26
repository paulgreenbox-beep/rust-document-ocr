//! Minimal in-process integration example for OCR Core v1.
//!
//! Usage:
//!   cargo run --example integrate_ocr --features native -- <input.pdf> <existing-output-dir> [--page-limit <n>] [--pdfium-dll <path>]

use std::{env, path::PathBuf, process::ExitCode};

use ocr_pipeline::{
    export_ocr_result_json, export_ocr_result_xlsx, process_document, GridKind, OcrOptions,
};

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let input = match args.next() {
        Some(path) => PathBuf::from(path),
        None => return usage(),
    };
    let output_dir = match args.next() {
        Some(path) => PathBuf::from(path),
        None => return usage(),
    };
    if !output_dir.is_dir() {
        eprintln!(
            "Output directory must already exist; no directory was created: {}",
            output_dir.display()
        );
        return ExitCode::from(1);
    }

    let mut options = OcrOptions::default();
    while let Some(flag) = args.next() {
        match flag.to_string_lossy().as_ref() {
            "--page-limit" if options.page_limit.is_none() => {
                let limit = match args.next().and_then(|value| value.into_string().ok()) {
                    Some(value) => match value.parse::<u32>() {
                        Ok(value) if value > 0 => value,
                        _ => return usage(),
                    },
                    None => return usage(),
                };
                options.page_limit = Some(limit);
            }
            "--pdfium-dll" if options.pdfium_dll_path.is_none() => {
                let path = match args.next() {
                    Some(path) => PathBuf::from(path),
                    None => return usage(),
                };
                options.pdfium_dll_path = Some(path);
            }
            _ => return usage(),
        }
    }
    let result = match process_document(&input, &options) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("OCR processing failed: {error}");
            return ExitCode::from(2);
        }
    };

    let json = output_dir.join("result.v1.json");
    let xlsx = output_dir.join("result.xlsx");
    if json.exists() || xlsx.exists() {
        eprintln!(
            "Refusing to overwrite existing result.v1.json or result.xlsx in {}",
            output_dir.display()
        );
        return ExitCode::from(1);
    }
    if let Err(error) = export_ocr_result_json(&result, &json) {
        eprintln!("JSON export failed: {error}");
        return ExitCode::from(3);
    }
    if let Err(error) = export_ocr_result_xlsx(&result, &xlsx, options.export_title_blocks) {
        eprintln!("XLSX export failed: {error}");
        return ExitCode::from(3);
    }

    let (data_tables, title_blocks) = result.tables.iter().flat_map(|page| &page.grids).fold(
        (0usize, 0usize),
        |(data, title), grid| match grid.kind {
            GridKind::DataTable => (data + 1, title),
            GridKind::TitleBlock => (data, title + 1),
            GridKind::Unknown => (data, title),
        },
    );
    println!("pages={}", result.document.pages.len());
    println!("data_tables={data_tables}");
    println!("title_blocks={title_blocks}");
    println!("warnings={}", result.warnings.len());
    println!("json={}", json.display());
    println!("xlsx={}", xlsx.display());
    ExitCode::SUCCESS
}

fn usage() -> ExitCode {
    eprintln!(
        "Usage: integrate_ocr <input.pdf> <existing-output-dir> [--page-limit <n>] [--pdfium-dll <path>]"
    );
    ExitCode::from(1)
}
