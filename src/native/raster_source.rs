//! Single-frame JPEG, PNG, BMP, and WebP page source.
//!
//! The decoder is selected by a previously verified byte signature. JPEG EXIF
//! orientation is applied once during decoding, and alpha is flattened against
//! white before conversion to the pipeline's grayscale page contract.

use std::{
    fs::File,
    io::{BufRead, BufReader, Seek},
    path::{Path, PathBuf},
};

use image::{
    imageops, DynamicImage, GrayImage, ImageDecoder, ImageFormat, ImageReader,
    Limits as ImageLimits, Luma,
};

use crate::{
    input_format::{
        detect_input_format, validate_frame_dimensions, InputFormat, InputLimits, InputSourceError,
    },
    page_analysis::PageOrientation,
    page_source::{dimensions_within_pixel_limit, PageSource, RenderedPage},
};

/// A decoded, physically oriented single raster page.
pub struct RasterImagePageSource {
    path: PathBuf,
    image: GrayImage,
    nominal_dpi: f64,
}

impl RasterImagePageSource {
    pub fn open(
        path: &Path,
        nominal_dpi: f64,
        limits: InputLimits,
    ) -> Result<Self, InputSourceError> {
        if !nominal_dpi.is_finite() || nominal_dpi <= 0.0 {
            return Err(InputSourceError::ImageDecodeFailed {
                path: path.to_path_buf(),
                detail: format!("nominal DPI must be positive, got {nominal_dpi}"),
            });
        }
        let input_format = detect_input_format(path, limits)?;
        let image_format =
            image_format(input_format).ok_or_else(|| InputSourceError::UnsupportedInputFormat {
                path: path.to_path_buf(),
            })?;
        let file = File::open(path).map_err(|error| InputSourceError::InputReadFailed {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
        let image = decode_reader(
            BufReader::new(file),
            image_format,
            path.to_path_buf(),
            limits,
        )?;
        Ok(Self {
            path: path.to_path_buf(),
            image,
            nominal_dpi,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn checked_page(&self, page_num: u32) -> Result<(), InputSourceError> {
        if page_num != 1 {
            return Err(InputSourceError::PageOutOfRange {
                page_number: page_num,
                page_count: 1,
            });
        }
        Ok(())
    }
}

impl PageSource for RasterImagePageSource {
    fn total_pages(&self) -> Result<u32, Box<dyn std::error::Error>> {
        Ok(1)
    }

    fn render_page(&mut self, page_num: u32) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        self.checked_page(page_num)?;
        Ok(RenderedPage::new(
            page_num,
            self.image.clone(),
            self.nominal_dpi,
        ))
    }

    fn render_page_with_max_pixels(
        &mut self,
        page_num: u32,
        max_pixels: usize,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        self.checked_page(page_num)?;
        let (width, height) =
            dimensions_within_pixel_limit(self.image.width(), self.image.height(), max_pixels)?;
        let image = if (width, height) == self.image.dimensions() {
            self.image.clone()
        } else {
            imageops::resize(&self.image, width, height, imageops::FilterType::Triangle)
        };
        let scale = f64::from(width) / f64::from(self.image.width());
        Ok(RenderedPage::new(page_num, image, self.nominal_dpi * scale))
    }

    fn render_page_at_dpi(
        &mut self,
        page_num: u32,
        _target_dpi: u32,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        // Raster pixels are intrinsic. Inventing additional pixels from a DPI
        // label does not add source information.
        self.render_page(page_num)
    }

    fn render_table_thumbnail(
        &mut self,
        page_num: u32,
        max_dimension: u32,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        self.checked_page(page_num)?;
        if max_dimension == 0 {
            return Err("table-thumbnail max dimension must be greater than zero".into());
        }
        let longest = self.image.width().max(self.image.height());
        if longest <= max_dimension {
            return self.render_page(page_num);
        }
        let scale = f64::from(max_dimension) / f64::from(longest);
        let width = (f64::from(self.image.width()) * scale).round().max(1.0) as u32;
        let height = (f64::from(self.image.height()) * scale).round().max(1.0) as u32;
        Ok(RenderedPage::new(
            page_num,
            imageops::resize(&self.image, width, height, imageops::FilterType::Triangle),
            self.nominal_dpi * scale,
        ))
    }

    fn page_orientation_from_metadata(
        &self,
        page_num: u32,
    ) -> Result<PageOrientation, Box<dyn std::error::Error>> {
        self.checked_page(page_num)?;
        // Metadata orientation has already been physically applied.
        Ok(PageOrientation::default())
    }
}

fn image_format(input_format: InputFormat) -> Option<ImageFormat> {
    match input_format {
        InputFormat::Jpeg => Some(ImageFormat::Jpeg),
        InputFormat::Png => Some(ImageFormat::Png),
        InputFormat::Bmp => Some(ImageFormat::Bmp),
        InputFormat::WebP => Some(ImageFormat::WebP),
        _ => None,
    }
}

fn decode_reader<R: BufRead + Seek>(
    reader: R,
    format: ImageFormat,
    path: PathBuf,
    limits: InputLimits,
) -> Result<GrayImage, InputSourceError> {
    let mut image_reader = ImageReader::with_format(reader, format);
    let mut decoder_limits = ImageLimits::default();
    // RGBA decode plus a grayscale output is the largest normal conversion
    // performed below. Keep a small metadata/header allowance so an unusually
    // low caller limit still reaches the explicit dimension check and returns
    // `image_pixel_limit_exceeded` before pixel decoding.
    decoder_limits.max_alloc = Some(
        limits
            .max_pixels_per_frame
            .saturating_mul(5)
            .max(1024 * 1024),
    );
    image_reader.limits(decoder_limits);
    let mut decoder =
        image_reader
            .into_decoder()
            .map_err(|error| InputSourceError::ImageDecodeFailed {
                path: path.clone(),
                detail: error.to_string(),
            })?;
    let (width, height) = decoder.dimensions();
    validate_frame_dimensions(1, width, height, limits)?;
    let orientation =
        decoder
            .orientation()
            .map_err(|error| InputSourceError::ImageDecodeFailed {
                path: path.clone(),
                detail: error.to_string(),
            })?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(|error| {
        InputSourceError::ImageDecodeFailed {
            path,
            detail: error.to_string(),
        }
    })?;
    image.apply_orientation(orientation);
    Ok(flatten_to_gray(image))
}

fn flatten_to_gray(image: DynamicImage) -> GrayImage {
    if !image.color().has_alpha() {
        return image.into_luma8();
    }

    let rgba = image.into_rgba8();
    GrayImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let pixel = rgba.get_pixel(x, y).0;
        let alpha = u32::from(pixel[3]);
        let inverse = 255 - alpha;
        let red = (u32::from(pixel[0]) * alpha + 255 * inverse + 127) / 255;
        let green = (u32::from(pixel[1]) * alpha + 255 * inverse + 127) / 255;
        let blue = (u32::from(pixel[2]) * alpha + 255 * inverse + 127) / 255;
        // Integer approximation of sRGB luma, matching the pipeline's 8-bit
        // grayscale contract.
        let luma = (77 * red + 150 * green + 29 * blue + 128) >> 8;
        Luma([luma.min(255) as u8])
    })
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, path::PathBuf};

    use image::{
        codecs::jpeg::JpegEncoder, DynamicImage, ExtendedColorType, ImageBuffer, Rgb, Rgba,
    };

    use super::{decode_reader, flatten_to_gray};
    use crate::input_format::{InputLimits, InputSourceError};

    #[test]
    fn transparent_pixels_are_composited_on_white() {
        let image = DynamicImage::ImageRgba8(
            ImageBuffer::from_vec(2, 1, vec![0, 0, 0, 0, 0, 0, 0, 255]).unwrap(),
        );
        let gray = flatten_to_gray(image);
        assert_eq!(gray.get_pixel(0, 0).0, [255]);
        assert_eq!(gray.get_pixel(1, 0).0, [0]);
    }

    #[test]
    fn jpeg_exif_orientation_is_applied_before_grayscale() {
        let image = ImageBuffer::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgb([0_u8, 0, 0])
            } else {
                Rgb([255_u8, 255, 255])
            }
        });
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 100)
            .encode(image.as_raw(), 2, 1, ExtendedColorType::Rgb8)
            .unwrap();
        let jpeg = insert_exif_orientation(jpeg, 6);

        let gray = decode_reader(
            Cursor::new(jpeg),
            image::ImageFormat::Jpeg,
            PathBuf::from("rotated.jpg"),
            InputLimits::default(),
        )
        .unwrap();
        assert_eq!(gray.dimensions(), (1, 2));
    }

    #[test]
    fn pixel_limit_is_checked_before_decode_output_allocation() {
        let image = ImageBuffer::<Rgba<u8>, _>::from_pixel(4, 4, Rgba([1, 2, 3, 255]));
        let mut png = Vec::new();
        DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let limits = InputLimits {
            max_pixels_per_frame: 15,
            ..InputLimits::default()
        };
        let error = decode_reader(
            Cursor::new(png),
            image::ImageFormat::Png,
            PathBuf::from("large.png"),
            limits,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            InputSourceError::ImagePixelLimitExceeded { .. }
        ));
        assert_eq!(error.code(), "image_pixel_limit_exceeded");
    }

    fn insert_exif_orientation(jpeg: Vec<u8>, value: u16) -> Vec<u8> {
        assert!(jpeg.starts_with(&[0xff, 0xd8]));
        let mut exif = b"Exif\0\0II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0".to_vec();
        exif.extend_from_slice(&value.to_le_bytes());
        exif.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let length = u16::try_from(exif.len() + 2).unwrap();

        let mut output = Vec::with_capacity(jpeg.len() + exif.len() + 4);
        output.extend_from_slice(&jpeg[..2]);
        output.extend_from_slice(&[0xff, 0xe1]);
        output.extend_from_slice(&length.to_be_bytes());
        output.extend_from_slice(&exif);
        output.extend_from_slice(&jpeg[2..]);
        output
    }
}
