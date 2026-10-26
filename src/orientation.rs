//! Conservative raster-orientation helpers.
//!
//! PDFium renders intrinsic `/Rotate` metadata into the page bitmap. This
//! module therefore detects only remaining text orientation in that bitmap and
//! applies the selected clockwise rotation before table detection and OCR.

use image::{imageops, GrayImage};

use crate::page_analysis::PageOrientation;

pub const ORIENTATION_ANGLES: [u16; 4] = [0, 90, 180, 270];
const MAX_PROBE_DIMENSION: u32 = 1_000;
/// Non-zero rotations are destructive and therefore require materially
/// stronger evidence than simply keeping the rendered orientation.
const MIN_NONZERO_ORIENTATION_CONFIDENCE: f32 = 0.50;
/// A destructive rotation must retain most of the confidence of the rendered
/// orientation. This prevents a much larger low-quality word set from winning
/// solely through the word-count component of [`OrientationEvidence::score`].
const MIN_NONZERO_RELATIVE_CONFIDENCE: f32 = 0.80;

/// Evidence produced by an OCR preflight for one possible orientation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrientationEvidence {
    pub angle: u16,
    pub word_count: usize,
    pub mean_confidence: f32,
}

impl OrientationEvidence {
    pub fn score(self) -> f32 {
        self.mean_confidence.clamp(0.0, 1.0) * (self.word_count as f32).sqrt()
    }
}

/// Returns a bounded thumbnail for OCR orientation preflight.
pub fn orientation_thumbnail(image: &GrayImage) -> GrayImage {
    let (width, height) = image.dimensions();
    let max_dimension = width.max(height);
    if max_dimension <= MAX_PROBE_DIMENSION {
        return image.clone();
    }
    let scale = MAX_PROBE_DIMENSION as f64 / f64::from(max_dimension);
    let target_width = (f64::from(width) * scale).round().max(1.0) as u32;
    let target_height = (f64::from(height) * scale).round().max(1.0) as u32;
    imageops::resize(
        image,
        target_width,
        target_height,
        imageops::FilterType::Triangle,
    )
}

/// Rotates an image clockwise by one supported right angle.
pub fn rotate_gray_image(image: GrayImage, angle: u16) -> Result<GrayImage, String> {
    match angle {
        0 => Ok(image),
        90 => Ok(imageops::rotate90(&image)),
        180 => Ok(imageops::rotate180(&image)),
        270 => Ok(imageops::rotate270(&image)),
        _ => Err(format!("unsupported page orientation angle: {angle}")),
    }
}

/// Chooses a conservative orientation from OCR evidence.
///
/// Low-confidence/no-text pages retain their rendered orientation (0°). For
/// close scores, 0° wins to avoid arbitrary rotation of technical drawings.
pub fn choose_orientation(evidence: &[OrientationEvidence]) -> PageOrientation {
    let baseline = evidence
        .iter()
        .copied()
        .find(|candidate| candidate.angle == 0)
        .unwrap_or(OrientationEvidence {
            angle: 0,
            word_count: 0,
            mean_confidence: 0.0,
        });
    let Some(best) = evidence
        .iter()
        .copied()
        .filter(|candidate| ORIENTATION_ANGLES.contains(&candidate.angle))
        .max_by(|left, right| left.score().total_cmp(&right.score()))
    else {
        return PageOrientation::default();
    };

    if best.word_count == 0
        || best.mean_confidence < 0.35
        || (best.angle != 0 && best.mean_confidence < MIN_NONZERO_ORIENTATION_CONFIDENCE)
    {
        return PageOrientation::default();
    }
    let baseline_confidence = baseline.mean_confidence.clamp(0.0, 1.0);
    let best_confidence = best.mean_confidence.clamp(0.0, 1.0);
    if best.angle != 0
        && baseline.word_count > 0
        && best_confidence < baseline_confidence * MIN_NONZERO_RELATIVE_CONFIDENCE
    {
        return PageOrientation {
            angle: 0,
            confidence: baseline_confidence,
        };
    }
    // Do not rotate when a non-zero candidate only marginally beats the
    // rendered orientation. This is particularly important for sparse
    // engineering drawings where the OCR preflight has little evidence.
    if best.angle != 0 && best.score() <= baseline.score() + 0.10 {
        return PageOrientation {
            angle: 0,
            confidence: baseline.mean_confidence.clamp(0.0, 1.0),
        };
    }
    PageOrientation {
        angle: best.angle,
        confidence: best.mean_confidence.clamp(0.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use image::{GrayImage, Luma};

    use super::{choose_orientation, rotate_gray_image, OrientationEvidence};

    #[test]
    fn rotations_preserve_pixels_and_swap_landscape_dimensions() {
        let mut image = GrayImage::from_pixel(4, 2, Luma([255]));
        image.put_pixel(0, 0, Luma([0]));

        let rotated = rotate_gray_image(image.clone(), 90).expect("rotate 90");
        assert_eq!(rotated.dimensions(), (2, 4));
        assert_eq!(rotated.get_pixel(1, 0), &Luma([0]));

        let restored = rotate_gray_image(rotated, 270).expect("rotate 270");
        assert_eq!(restored, image);
        assert_eq!(
            rotate_gray_image(image, 45).unwrap_err(),
            "unsupported page orientation angle: 45"
        );
    }

    #[test]
    fn chooses_clearly_better_rotated_text() {
        let selected = choose_orientation(&[
            OrientationEvidence {
                angle: 0,
                word_count: 1,
                mean_confidence: 0.40,
            },
            OrientationEvidence {
                angle: 90,
                word_count: 8,
                mean_confidence: 0.88,
            },
        ]);
        assert_eq!(selected.angle, 90);
        assert!(selected.confidence > 0.8);
    }

    #[test]
    fn sparse_or_ambiguous_evidence_keeps_zero() {
        let sparse = choose_orientation(&[OrientationEvidence {
            angle: 180,
            word_count: 0,
            mean_confidence: 0.9,
        }]);
        assert_eq!(sparse.angle, 0);

        let ambiguous = choose_orientation(&[
            OrientationEvidence {
                angle: 0,
                word_count: 5,
                mean_confidence: 0.8,
            },
            OrientationEvidence {
                angle: 180,
                word_count: 5,
                mean_confidence: 0.81,
            },
        ]);
        assert_eq!(ambiguous.angle, 0);
    }

    #[test]
    fn low_confidence_word_count_cannot_force_a_destructive_rotation() {
        let selected = choose_orientation(&[
            OrientationEvidence {
                angle: 0,
                word_count: 2,
                mean_confidence: 0.20,
            },
            OrientationEvidence {
                angle: 180,
                word_count: 400,
                mean_confidence: 0.40,
            },
        ]);

        assert_eq!(selected.angle, 0);
        assert_eq!(selected.confidence, 0.0);
    }

    #[test]
    fn many_materially_lower_confidence_words_cannot_force_rotation() {
        let selected = choose_orientation(&[
            OrientationEvidence {
                angle: 0,
                word_count: 100,
                mean_confidence: 0.90,
            },
            OrientationEvidence {
                angle: 180,
                word_count: 400,
                mean_confidence: 0.51,
            },
        ]);

        assert_eq!(selected.angle, 0);
        assert_eq!(selected.confidence, 0.90);
    }

    #[test]
    fn relative_confidence_boundary_allows_supported_rotation() {
        let selected = choose_orientation(&[
            OrientationEvidence {
                angle: 0,
                word_count: 100,
                mean_confidence: 0.90,
            },
            OrientationEvidence {
                angle: 180,
                word_count: 400,
                mean_confidence: 0.72,
            },
        ]);

        assert_eq!(selected.angle, 180);
        assert_eq!(selected.confidence, 0.72);
    }

    #[test]
    fn relative_confidence_just_below_boundary_keeps_zero() {
        let selected = choose_orientation(&[
            OrientationEvidence {
                angle: 0,
                word_count: 100,
                mean_confidence: 0.90,
            },
            OrientationEvidence {
                angle: 180,
                word_count: 400,
                mean_confidence: 0.719,
            },
        ]);

        assert_eq!(selected.angle, 0);
        assert_eq!(selected.confidence, 0.90);
    }
}
