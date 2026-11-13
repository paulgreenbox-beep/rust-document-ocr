//! Input-to-layout graphics for the opt-in Word export. The OCR result is read-only.

use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

use crate::{
    export_word::{
        export_word, WordExportOptions, WordExportReport, WordGraphic, WordPageGraphics,
        WordRasterFormat, WordRect,
    },
    input_format::{detect_input_format, InputFormat, InputLimits},
    OcrOptions, OcrResult,
};

const IMAGE_BYTES: u64 = 24 * 1024 * 1024;
const IMAGE_PIXELS: u64 = 12_000_000;

pub fn export_pdf_word(
    input: &Path,
    options: &OcrOptions,
    result: &OcrResult,
    output: &Path,
) -> Result<WordExportReport, Box<dyn std::error::Error>> {
    let (graphics, warnings) =
        super::word_pdf_graphics::collect_pdf_graphics(input, options, result)
            .map_err(std::io::Error::other)?;
    let mut report = write(result, options, &graphics, output)?;
    report.warnings.extend(warnings);
    Ok(report)
}

/// The original raster file is a full-page OCR source, not an illustration.
/// Only include it when the caller explicitly asks for the scan reference.
pub fn export_input_word(
    input: &Path,
    options: &OcrOptions,
    result: &OcrResult,
    output: &Path,
    include_scan_reference: bool,
) -> Result<WordExportReport, Box<dyn std::error::Error>> {
    let format = detect_input_format(input, InputLimits::default())?;
    if format == InputFormat::Pdf {
        let mut report = export_pdf_word(input, options, result, output)?;
        if include_scan_reference {
            report.warnings.push("The scan reference option applies only to single raster inputs, not embedded PDF page backgrounds.".into());
        }
        return Ok(report);
    }
    let mut warnings = Vec::new();
    let graphics = if include_scan_reference {
        match source_image(input, format, options.ocr_dpi) {
            Ok(graphic) => vec![graphic],
            Err(message) => {
                warnings.push(message);
                Vec::new()
            }
        }
    } else {
        warnings.push(
            "The input is a whole-page raster source; the scan reference was not requested.".into(),
        );
        Vec::new()
    };
    let mut report = write(result, options, &graphics, output)?;
    report.warnings.extend(warnings);
    Ok(report)
}

fn write(
    result: &OcrResult,
    options: &OcrOptions,
    graphics: &[WordPageGraphics],
    output: &Path,
) -> Result<WordExportReport, Box<dyn std::error::Error>> {
    export_word(
        result,
        graphics,
        output,
        WordExportOptions {
            include_title_blocks: options.export_title_blocks,
        },
    )
}

fn source_image(input: &Path, format: InputFormat, dpi: u32) -> Result<WordPageGraphics, String> {
    let image_format = match format {
        InputFormat::Jpeg => ImageFormat::Jpeg,
        InputFormat::Png => ImageFormat::Png,
        InputFormat::Bmp => ImageFormat::Bmp,
        InputFormat::WebP => ImageFormat::WebP,
        _ => {
            return Err(format!(
            "Original scan reference is not implemented for {}; editable OCR content was retained.",
            format.as_str()
        ))
        }
    };
    let mut bytes = Vec::new();
    File::open(input)
        .map_err(|e| e.to_string())?
        .take(IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > IMAGE_BYTES {
        return Err("Scan reference skipped: encoded image exceeds 24 MiB.".into());
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), image_format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(IMAGE_PIXELS * 8);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| format!("Cannot decode scan reference: {e}"))?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > IMAGE_PIXELS {
        return Err("Scan reference skipped: pixel limit exceeded.".into());
    }
    let orientation = decoder
        .orientation()
        .map_err(|e| format!("Cannot read scan reference orientation: {e}"))?;
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("Cannot decode scan reference: {e}"))?;
    let orientation_applied = orientation != image::metadata::Orientation::NoTransforms;
    image.apply_orientation(orientation);
    let width_pt = image.width() as f32 * 72.0 / dpi.max(1) as f32;
    let height_pt = image.height() as f32 * 72.0 / dpi.max(1) as f32;
    let (raster, raster_format) =
        if !orientation_applied && format == InputFormat::Jpeg && bytes.ends_with(b"\xff\xd9") {
            (bytes, WordRasterFormat::Jpeg)
        } else if !orientation_applied
            && format == InputFormat::Png
            && bytes.ends_with(b"\0\0\0\0IEND\xaeB`\x82")
        {
            (bytes, WordRasterFormat::Png)
        } else {
            let mut cursor = std::io::Cursor::new(Vec::new());
            image
                .write_to(&mut cursor, ImageFormat::Png)
                .map_err(|e| format!("Cannot encode scan reference: {e}"))?;
            (cursor.into_inner(), WordRasterFormat::Png)
        };
    if raster.len() as u64 > IMAGE_BYTES {
        return Err("Scan reference skipped: converted image exceeds 24 MiB.".into());
    }
    // Place the optional reference after the editable content, not over it.
    Ok(WordPageGraphics {
        page_index: 0,
        width_pt,
        height_pt,
        images: vec![WordGraphic {
            rect: WordRect {
                x: 0.,
                y: 0.,
                width: width_pt,
                height: height_pt,
            },
            raster,
            raster_format,
            svg: None,
            scan_reference: true,
        }],
    })
}
