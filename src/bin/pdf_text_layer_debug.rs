//! Read-only diagnostic for a PDF page's native text layer.
//!
//! This utility intentionally does not render pages and does not initialize
//! Tesseract. It writes only the explicitly requested UTF-8 text report.

#[cfg(feature = "native")]
use std::path::PathBuf;

#[cfg(feature = "native")]
use clap::Parser;
#[cfg(feature = "native")]
use pdfium_render::prelude::*;

#[cfg(feature = "native")]
#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    text_output: PathBuf,
}

#[cfg(feature = "native")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    eprintln!("stage=bind_pdfium");
    let bindings = Pdfium::bind_to_system_library()?;
    let pdfium = Pdfium::new(bindings);

    eprintln!("stage=load_document");
    let document = pdfium.load_pdf_from_file(&args.input, None)?;
    eprintln!("stage=load_page_0 pages={}", document.pages().len());
    let page = document.pages().get(0)?;
    eprintln!(
        "stage=query_page_size width={} height={}",
        page.width().value,
        page.height().value
    );

    eprintln!("stage=load_text_page");
    let page_text = page.text()?;
    let character_count = page_text.len();
    eprintln!("stage=extract_text characters={character_count}");
    let text = page_text.all();
    std::fs::write(&args.text_output, &text)?;

    let chars: Vec<char> = text.chars().collect();
    let non_whitespace = chars.iter().filter(|c| !c.is_whitespace()).count();
    let printable = chars.iter().filter(|c| !c.is_control()).count();
    let cyrillic = chars.iter().any(|c| ('\u{0400}'..='\u{052F}').contains(c));
    let latin = chars.iter().any(|c| c.is_ascii_alphabetic());
    let digits = chars.iter().any(|c| c.is_ascii_digit());
    let printable_ratio = if chars.is_empty() {
        0.0
    } else {
        printable as f64 / chars.len() as f64
    };
    let mode = if non_whitespace >= 20 && printable_ratio >= 0.8 {
        "NativeText"
    } else {
        "Ocr"
    };
    println!(
        "mode={mode} pdfium_characters={character_count} unicode_characters={} non_whitespace={} printable_ratio={printable_ratio:.3} cyrillic={cyrillic} latin={latin} digits={digits}",
        chars.len(),
        non_whitespace
    );
    Ok(())
}

#[cfg(not(feature = "native"))]
fn main() {
    eprintln!("pdf_text_layer_debug requires the 'native' feature");
}
