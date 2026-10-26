//! Deterministic test-fixture helper for bundled, multi-page DjVu documents.
//!
//! This is not part of the OCR runtime. It converts one or more source images
//! to bilevel pages and writes a bundled DjVu using the same pinned pure-Rust
//! crate that the decoder uses.

use std::{
    error::Error,
    path::{Path, PathBuf},
};

use djvu_rs::{
    jb2_encode::{encode_djvm_bundle_jb2, BUNDLE_DEFAULT_DPI},
    Bitmap,
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let output = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage: generate_djvu_fixture OUTPUT.djvu PAGE.png [PAGE.png ...]")?;
    let pages = args.map(PathBuf::from).collect::<Vec<_>>();
    if pages.len() < 2 {
        return Err("a multi-page fixture requires at least two source images".into());
    }

    let bitmaps = pages
        .iter()
        .map(|page| image_to_bitmap(page))
        .collect::<Result<Vec<_>, _>>()?;
    let bytes = encode_djvm_bundle_jb2(&bitmaps, 2, BUNDLE_DEFAULT_DPI);
    std::fs::write(&output, bytes)?;
    println!("wrote {} pages to {}", bitmaps.len(), output.display());
    Ok(())
}

fn image_to_bitmap(path: &Path) -> Result<Bitmap, Box<dyn Error>> {
    let image = image::open(path)?.into_luma8();
    let mut bitmap = Bitmap::new(image.width(), image.height());
    for (x, y, pixel) in image.enumerate_pixels() {
        bitmap.set(x, y, pixel.0[0] < 180);
    }
    Ok(bitmap)
}
