//! Produces a PDF containing only rasterized page images for diagnostics.
//!
//! The output deliberately has no inherited PDF objects: no text layer,
//! annotations, forms, attachments, or source-page resources are copied.

#[cfg(feature = "native")]
use std::fs::{self, File};
#[cfg(feature = "native")]
use std::io::{BufWriter, Cursor, Write};
#[cfg(feature = "native")]
use std::path::PathBuf;

#[cfg(feature = "native")]
use clap::Parser;
#[cfg(feature = "native")]
use image::{DynamicImage, ImageFormat};
#[cfg(feature = "native")]
use ocr_pipeline::{PageSource, PdfiumPageSource};

#[cfg(feature = "native")]
#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    input: PathBuf,
    #[arg(long, default_value_t = 300)]
    dpi: u32,
}

#[cfg(feature = "native")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let parent = args
        .input
        .parent()
        .ok_or("input PDF has no parent directory")?;
    let stem = args
        .input
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or("input PDF filename is not valid UTF-8")?;
    let output_pdf = parent.join(format!("{stem}.image-only.pdf"));
    let png_dir = parent.join(format!("{stem}.image-only-pages"));
    let staging_pdf = parent.join(format!(".{stem}.image-only.pdf.staging"));

    eprintln!("stage=open_pdf");
    let mut source = PdfiumPageSource::open(&args.input, f64::from(args.dpi))?;
    let total = source.total_pages()?;
    if output_pdf.exists() || png_dir.exists() || staging_pdf.exists() {
        return Err(format!(
            "refusing to overwrite an existing diagnostic path: {}, {}, or {}",
            output_pdf.display(),
            png_dir.display(),
            staging_pdf.display()
        )
        .into());
    }

    fs::create_dir(&png_dir)?;
    let file = File::create(&staging_pdf)?;
    let mut writer = PdfWriter::new(BufWriter::new(file));
    writer.header()?;
    let mut page_refs = Vec::with_capacity(total as usize);

    for page_number in 1..=total {
        eprintln!("stage=render_page page={page_number}/{total}");
        let page = source.render_page(page_number)?;
        let png_path = png_dir.join(format!("page-{page_number:04}.png"));
        page.image.save_with_format(&png_path, ImageFormat::Png)?;

        let width = page.image.width();
        let height = page.image.height();
        let mut jpeg = Cursor::new(Vec::new());
        DynamicImage::ImageLuma8(page.image).write_to(&mut jpeg, ImageFormat::Jpeg)?;
        let image_ref = 4 + (page_number - 1) * 3;
        let content_ref = image_ref + 1;
        let page_ref = image_ref + 2;
        let points_per_pixel = 72.0 / f64::from(args.dpi);
        let page_width = f64::from(width) * points_per_pixel;
        let page_height = f64::from(height) * points_per_pixel;

        writer.object(
            image_ref,
            format!(
                "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>",
                jpeg.get_ref().len()
            )
            .as_bytes(),
            Some(jpeg.get_ref()),
        )?;
        let content = format!("q\n{page_width:.4} 0 0 {page_height:.4} 0 0 cm\n/Im0 Do\nQ\n");
        writer.object(
            content_ref,
            format!("<< /Length {} >>", content.len()).as_bytes(),
            Some(content.as_bytes()),
        )?;
        writer.object(
            page_ref,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_width:.4} {page_height:.4}] /Resources << /XObject << /Im0 {image_ref} 0 R >> >> /Contents {content_ref} 0 R >>"
            )
            .as_bytes(),
            None,
        )?;
        page_refs.push(page_ref);
    }

    let kids = page_refs
        .iter()
        .map(|reference| format!("{reference} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    writer.object(
        2,
        format!("<< /Type /Pages /Count {total} /Kids [{kids}] >>").as_bytes(),
        None,
    )?;
    writer.object(1, b"<< /Type /Catalog /Pages 2 0 R >>", None)?;
    writer.finish(1)?;
    fs::rename(&staging_pdf, &output_pdf)?;
    println!("image_only_pdf={}", output_pdf.display());
    println!("png_directory={}", png_dir.display());
    Ok(())
}

#[cfg(feature = "native")]
struct PdfWriter<W: Write> {
    output: W,
    offsets: Vec<u64>,
    position: u64,
}

#[cfg(feature = "native")]
impl<W: Write> PdfWriter<W> {
    fn new(output: W) -> Self {
        Self {
            output,
            offsets: vec![0],
            position: 0,
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.output.write_all(bytes)?;
        self.position += bytes.len() as u64;
        Ok(())
    }

    fn header(&mut self) -> std::io::Result<()> {
        self.write_all(b"%PDF-1.7\n%\xFF\xFF\xFF\xFF\n")
    }

    fn object(
        &mut self,
        number: u32,
        dictionary: &[u8],
        stream: Option<&[u8]>,
    ) -> std::io::Result<()> {
        while self.offsets.len() <= number as usize {
            self.offsets.push(0);
        }
        self.offsets[number as usize] = self.position;
        self.write_all(format!("{number} 0 obj\n").as_bytes())?;
        self.write_all(dictionary)?;
        if let Some(stream) = stream {
            self.write_all(b"\nstream\n")?;
            self.write_all(stream)?;
            self.write_all(b"\nendstream")?;
        }
        self.write_all(b"\nendobj\n")
    }

    fn finish(mut self, root: u32) -> std::io::Result<()> {
        let xref = self.position;
        self.write_all(format!("xref\n0 {}\n", self.offsets.len()).as_bytes())?;
        self.write_all(b"0000000000 65535 f \n")?;
        let offsets = self.offsets.clone();
        for offset in offsets.iter().skip(1) {
            self.write_all(format!("{offset:010} 00000 n \n").as_bytes())?;
        }
        self.write_all(
            format!(
                "trailer\n<< /Size {} /Root {root} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                self.offsets.len()
            )
            .as_bytes(),
        )?;
        self.output.flush()
    }
}

#[cfg(not(feature = "native"))]
fn main() {
    eprintln!("pdf_image_only_debug requires the 'native' feature");
}
