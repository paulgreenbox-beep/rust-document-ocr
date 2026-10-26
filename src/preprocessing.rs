//! Image preprocessing utilities.
//!
//! The default path performs conservative percentile contrast normalization.
//! Tesseract retains the grayscale information it needs for its own adaptive
//! binarization; table-grid detection continues to consume the original image.

use image::GrayImage;

/// Maximum number of pixels allowed in a single page bitmap.
///
/// 50 megapixels is roughly 7000x7000 — enough for ~300 DPI on a tabloid
/// page while still bounding memory to a few hundred MB even with
/// multi-byte per-pixel intermediate buffers.
pub const DEFAULT_MAX_PIXELS: usize = 50_000_000;

/// Normalizes grayscale contrast and rejects pages that exceed `max_pixels`
/// before allocating any additional page-sized image.
///
/// A fixed threshold destroys anti-aliased strokes, faint Cyrillic glyphs, and
/// technical drawings with colored backgrounds. Instead, the darkest and
/// lightest one percent are clipped and the remaining grayscale range is
/// stretched. Uniform and already binary images remain unchanged.
pub fn preprocess(
    mut image: GrayImage,
    max_pixels: usize,
) -> Result<GrayImage, Box<dyn std::error::Error>> {
    let (width, height) = (image.width() as usize, image.height() as usize);
    let pixel_count = width.checked_mul(height).ok_or("pixel count overflow")?;

    if pixel_count > max_pixels {
        return Err(
            format!("page too large: {pixel_count} pixels exceeds limit of {max_pixels}").into(),
        );
    }
    if pixel_count == 0 {
        return Ok(image);
    }

    let mut histogram = [0usize; 256];
    for pixel in image.pixels() {
        histogram[pixel[0] as usize] += 1;
    }
    let tail = pixel_count / 100;
    let mut cumulative = 0usize;
    let mut low = 0u8;
    for (value, count) in histogram.iter().enumerate() {
        cumulative += count;
        if cumulative > tail {
            low = value as u8;
            break;
        }
    }
    cumulative = 0;
    let mut high = u8::MAX;
    for (value, count) in histogram.iter().enumerate().rev() {
        cumulative += count;
        if cumulative > tail {
            high = value as u8;
            break;
        }
    }
    if high <= low {
        return Ok(image);
    }

    let range = u16::from(high - low);
    for pixel in image.pixels_mut() {
        let value = pixel[0].clamp(low, high);
        pixel[0] = (((u16::from(value - low) * 255) + range / 2) / range) as u8;
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    #[test]
    fn uniform_image_is_preserved() {
        let img = GrayImage::from_pixel(2, 2, Luma([200]));
        let out = preprocess(img, DEFAULT_MAX_PIXELS).unwrap();
        assert_eq!(out.get_pixel(0, 0)[0], 200);
    }

    #[test]
    fn rejects_oversize_image() {
        let img = GrayImage::from_pixel(100, 100, Luma([0]));
        let err = preprocess(img, 50).unwrap_err();
        assert!(err.to_string().contains("exceeds"));
    }

    #[test]
    fn stretches_low_contrast_grayscale_without_fixed_binarization() {
        let img = GrayImage::from_raw(3, 1, vec![180, 190, 200]).unwrap();
        let out = preprocess(img, DEFAULT_MAX_PIXELS).unwrap();
        assert_eq!(out.as_raw(), &[0, 128, 255]);
    }

    #[test]
    fn already_binary_strokes_remain_binary() {
        let img = GrayImage::from_raw(4, 1, vec![0, 255, 0, 255]).unwrap();
        let out = preprocess(img, DEFAULT_MAX_PIXELS).unwrap();
        assert_eq!(out.as_raw(), &[0, 255, 0, 255]);
    }
}
