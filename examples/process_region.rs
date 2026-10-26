//! Minimal in-process region OCR example.
//!
//! Usage:
//! `process_region <input.pdf> <page-index> <x> <y> <width> <height> <auto|text|table> <output.json>`

use std::{error::Error, path::PathBuf, time::Instant};

use ocr_pipeline::{
    export_region_result_json, process_region, NormalizedRect, OcrOptions, RegionMode,
    RegionRequest,
};

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() != 8 {
        return Err(
            "usage: process_region <input.pdf> <page-index> <x> <y> <width> <height> <auto|text|table> <output.json>"
                .into(),
        );
    }

    let input = PathBuf::from(&arguments[0]);
    let page_index = parse::<usize>(&arguments[1], "page-index")?;
    let x = parse::<f32>(&arguments[2], "x")?;
    let y = parse::<f32>(&arguments[3], "y")?;
    let width = parse::<f32>(&arguments[4], "width")?;
    let height = parse::<f32>(&arguments[5], "height")?;
    let mode = match arguments[6].to_string_lossy().as_ref() {
        "auto" => RegionMode::Auto,
        "text" => RegionMode::Text,
        "table" => RegionMode::Table,
        other => {
            return Err(format!("unsupported mode {other:?}; use auto, text, or table").into())
        }
    };
    let output = PathBuf::from(&arguments[7]);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let options = OcrOptions {
        pdfium_dll_path: Some(
            std::env::var_os("OCR_TEST_PDFIUM_LIBRARY")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("runtime/pdfium/7881/bin/pdfium.dll")),
        ),
        tessdata_path: std::env::var_os("TESSDATA_PREFIX").map(PathBuf::from),
        ..OcrOptions::default()
    };

    let started = Instant::now();
    let result = process_region(
        &input,
        RegionRequest {
            page_index,
            rect: NormalizedRect::new(x, y, width, height),
            mode,
        },
        options,
    )
    .map_err(|error| format!("{}: {error}", error.code()))?;
    export_region_result_json(&result, &output)?;

    println!(
        "status=ok page_index={} mode={:?} words={} tables={} filled_cells={} elapsed_ms={:.3} output={}",
        result.page_index,
        result.processing_mode,
        result.metrics.recognized_words,
        result.tables.len(),
        result.metrics.filled_cells,
        started.elapsed().as_secs_f64() * 1_000.0,
        output.display()
    );
    Ok(())
}

fn parse<T>(value: &std::ffi::OsStr, name: &str) -> Result<T, Box<dyn Error>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .to_string_lossy()
        .parse()
        .map_err(|error| format!("invalid {name}: {error}").into())
}
