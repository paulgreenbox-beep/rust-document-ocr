use image::GrayImage;

#[derive(Debug, Clone, Copy)]
pub struct DetectionResult {
    pub is_table: bool,
    pub confidence: f32,
}

fn has_long_dark_run<I>(pixels: I, min_line_length: usize) -> bool
where
    I: IntoIterator<Item = u8>,
{
    let mut run_length = 0;

    for value in pixels {
        if value < 128 {
            run_length += 1;
            if run_length >= min_line_length {
                return true;
            }
        } else {
            run_length = 0;
        }
    }

    false
}

/// Detect table-like structure using horizontal and vertical projections.
///
/// A row is considered a horizontal line when it contains a consecutive run
/// of dark pixels spanning at least ten percent of the image width. Columns
/// are evaluated analogously using the image height.
pub fn detect_table(image: &GrayImage) -> DetectionResult {
    let width = image.width() as usize;
    let height = image.height() as usize;

    if width == 0 || height == 0 {
        return DetectionResult {
            is_table: false,
            confidence: 0.0,
        };
    }

    let min_horizontal_length = ((width as f32 * 0.25).ceil() as usize).max(1);
    let min_vertical_length = ((height as f32 * 0.25).ceil() as usize).max(1);

    let horizontal_lines = (0..height)
        .filter(|&y| {
            has_long_dark_run(
                (0..width).map(|x| image.get_pixel(x as u32, y as u32)[0]),
                min_horizontal_length,
            )
        })
        .count();

    let vertical_lines = (0..width)
        .filter(|&x| {
            has_long_dark_run(
                (0..height).map(|y| image.get_pixel(x as u32, y as u32)[0]),
                min_vertical_length,
            )
        })
        .count();

    let confidence = ((horizontal_lines + vertical_lines) as f32 / 6.0).min(1.0);

    DetectionResult {
        is_table: horizontal_lines >= 2 && vertical_lines >= 2,
        confidence,
    }
}

#[cfg(test)]
mod tests {
    use super::detect_table;
    use image::{GrayImage, ImageBuffer, Luma};

    #[test]
    fn blank_image_is_not_a_table() {
        let image = GrayImage::from_pixel(100, 100, Luma([255]));
        let result = detect_table(&image);

        assert!(!result.is_table);
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn drawn_grid_is_detected_as_a_table() {
        let mut image: GrayImage = ImageBuffer::from_pixel(100, 100, Luma([255]));

        for &y in &[10, 50, 90] {
            for x in 0..100 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        for &x in &[10, 50, 90] {
            for y in 0..100 {
                image.put_pixel(x, y, Luma([0]));
            }
        }

        let result = detect_table(&image);

        assert!(result.is_table);
        assert_eq!(result.confidence, 1.0);
    }

    #[test]
    fn confidence_scales_with_detected_lines() {
        let mut image: GrayImage = ImageBuffer::from_pixel(100, 100, Luma([255]));

        for &y in &[20, 80] {
            for x in 0..100 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        for &x in &[20, 80] {
            for y in 0..100 {
                image.put_pixel(x, y, Luma([0]));
            }
        }

        let result = detect_table(&image);

        assert!(result.is_table);
        assert!((result.confidence - (4.0 / 6.0)).abs() < f32::EPSILON);
    }
}
