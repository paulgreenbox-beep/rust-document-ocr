//! Read-only diagnostic for comparing PDFium page-text and text-object extraction.
//!
//! This binary never renders pages and never invokes OCR.

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
    #[arg(long, default_value_t = 1)]
    page: u16,
    #[arg(long)]
    object_text_output: Option<PathBuf>,
}

#[cfg(feature = "native")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let dll = std::env::var_os("PDFIUM_DYNAMIC_LIB_PATH")
        .map(PathBuf::from)
        .ok_or("PDFIUM_DYNAMIC_LIB_PATH must point to the versioned runtime directory")?
        .join("pdfium.dll");
    let pdfium = Pdfium::new(Pdfium::bind_to_library(&dll)?);
    let document = pdfium.load_pdf_from_file(&args.input, None)?;
    let page_index = args.page.checked_sub(1).ok_or("page is 1-indexed")?;
    let page = document.pages().get(page_index as i32)?;
    let page_text = page.text()?;
    let all = page_text.all();

    let mut object_text = String::new();
    let mut text_objects = 0usize;
    for object in page.objects().iter() {
        if let Some(text_object) = object.as_text_object() {
            text_objects += 1;
            let text = page_text.for_object(text_object);
            if !text.is_empty() {
                if !object_text.is_empty() {
                    object_text.push('\n');
                }
                object_text.push_str(&text);
            }
        }
    }

    if let Some(path) = args.object_text_output {
        std::fs::write(path, &object_text)?;
    }

    println!(
        "pdfium_count={} page_text_chars={} text_objects={} object_text_chars={}",
        page_text.len(),
        all.chars().count(),
        text_objects,
        object_text.chars().count()
    );
    Ok(())
}

#[cfg(not(feature = "native"))]
fn main() {
    eprintln!("pdf_text_recovery_probe requires the 'native' feature");
}
