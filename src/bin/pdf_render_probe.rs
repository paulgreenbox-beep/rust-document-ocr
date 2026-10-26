//! Render-only PDFium probe. It intentionally does not write output or invoke OCR.

#[cfg(feature = "native")]
use clap::{Parser, ValueEnum};
#[cfg(feature = "native")]
use pdfium_render::prelude::*;
#[cfg(feature = "native")]
use std::io::Write;
#[cfg(feature = "native")]
use std::path::PathBuf;

#[cfg(feature = "native")]
#[derive(Clone, Debug, ValueEnum)]
enum Variant {
    Default,
    Width800,
    Gray,
}
#[cfg(feature = "native")]
#[derive(Parser)]
struct Args {
    #[arg(long)]
    input: PathBuf,
    #[arg(long, value_enum)]
    variant: Variant,
}
#[cfg(feature = "native")]
fn stage(name: &str) {
    eprintln!("stage={name}");
    let _ = std::io::stderr().flush();
}
#[cfg(feature = "native")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    stage("bind_pdfium");
    let bindings = Pdfium::bind_to_system_library()?;
    let pdfium = Pdfium::new(bindings);
    stage("open_pdf");
    let doc = pdfium.load_pdf_from_file(&args.input, None)?;
    stage("load_page_0");
    let page = doc.pages().get(0)?;
    eprintln!(
        "stage=page_size width={} height={}",
        page.width().value,
        page.height().value
    );
    let _ = std::io::stderr().flush();
    stage("create_render_config");
    let config = match args.variant {
        Variant::Default => PdfRenderConfig::new(),
        Variant::Width800 => PdfRenderConfig::new().set_target_width(800),
        Variant::Gray => PdfRenderConfig::new().set_format(PdfBitmapFormat::Gray),
    };
    stage("render_with_config");
    let bitmap = page.render_with_config(&config)?;
    println!("rendered={}x{}", bitmap.width(), bitmap.height());
    Ok(())
}
#[cfg(not(feature = "native"))]
fn main() {
    eprintln!("pdf_render_probe requires native");
}
