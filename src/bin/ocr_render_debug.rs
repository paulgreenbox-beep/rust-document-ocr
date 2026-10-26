//! Local diagnostic utility for inspecting the image handed to Tesseract.
//!
//! This is deliberately separate from the user-facing CLI: it does not create
//! OCR output directories and only writes the PNG explicitly requested by the
//! operator.

#[cfg(feature = "native")]
use std::path::PathBuf;

#[cfg(feature = "native")]
use clap::Parser;
#[cfg(feature = "native")]
use image::ImageFormat;
#[cfg(feature = "native")]
use ocr_pipeline::{OcrEngine, PageSource, PdfiumPageSource, TesseractOcrEngine};

#[cfg(feature = "native")]
#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output_png: PathBuf,
    #[arg(long, default_value_t = 300)]
    dpi: u32,
    #[arg(long, default_value = "eng")]
    language: String,
}

#[cfg(feature = "native")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    eprintln!("stage=open_pdf");
    let mut source = PdfiumPageSource::open(&args.input, f64::from(args.dpi))?;
    eprintln!("stage=render_page");
    let page = source.render_page(1)?;
    eprintln!("stage=pack_and_save");
    let (min, max) = page
        .image
        .pixels()
        .map(|pixel| pixel[0])
        .fold((u8::MAX, u8::MIN), |(min, max), value| {
            (min.min(value), max.max(value))
        });
    page.image
        .save_with_format(&args.output_png, ImageFormat::Png)?;

    eprintln!("stage=initialize_tesseract");
    let ocr = TesseractOcrEngine::new(&args.language)?;
    eprintln!("stage=recognize");
    let text = ocr.recognize_text(&page.image)?;
    println!(
        "pages={} bitmap={}x{} pixels={}..{} text_chars={} text={:?}",
        source.total_pages()?,
        page.image.width(),
        page.image.height(),
        min,
        max,
        text.chars().count(),
        text.trim()
    );
    Ok(())
}

#[cfg(not(feature = "native"))]
fn main() {
    eprintln!("ocr_render_debug requires the 'native' feature");
}
