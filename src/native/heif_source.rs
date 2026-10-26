//! HEIC/HEIF page source backed by the safe `libheif-rs` wrapper.
//!
//! The source intentionally exposes only the container's primary image as one
//! OCR page. Additional top-level images are reported through
//! [`HeifSourceWarning`] so callers can surface the limitation without silently
//! changing the document contract.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use image::GrayImage;
use libheif_rs::{
    check_file_type, ColorSpace, DecodingOptions, FileTypeResult, HeifContext, LibHeif, RgbChroma,
    StreamReader,
};
use thiserror::Error;

use crate::page_source::{PageSource, RenderedPage};

pub const DEFAULT_HEIF_DPI: f64 = 300.0;
pub const DEFAULT_HEIF_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_HEIF_MAX_PIXELS: u64 = 50_000_000;
pub const DEFAULT_HEIF_MAX_FRAMES: usize = 64;

/// Resource limits enforced before libheif is allowed to decode pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeifSourceLimits {
    pub max_file_bytes: u64,
    pub max_pixels: u64,
    pub max_frames: usize,
    /// Limits tile decoding inside libheif. `0` decodes tiles on the caller's
    /// thread; `1` uses one libheif worker.
    pub max_decoding_threads: u32,
}

impl Default for HeifSourceLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_HEIF_MAX_FILE_BYTES,
            max_pixels: DEFAULT_HEIF_MAX_PIXELS,
            max_frames: DEFAULT_HEIF_MAX_FRAMES,
            max_decoding_threads: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeifSourceWarning {
    /// Only the primary top-level image is exposed as an OCR page.
    AdditionalTopLevelImagesIgnored {
        total_images: usize,
        ignored_images: usize,
    },
}

#[derive(Debug, Error)]
pub enum HeifSourceError {
    #[error("invalid HEIF source configuration: {message}")]
    InvalidConfiguration { message: String },

    #[error("failed to access HEIF input {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("HEIF input {path} is {actual_bytes} bytes, exceeding the {limit_bytes}-byte limit")]
    ImageFileLimitExceeded {
        path: PathBuf,
        actual_bytes: u64,
        limit_bytes: u64,
    },

    #[error(
        "HEIF image is {width}x{height} ({actual_pixels} pixels), exceeding the {limit_pixels}-pixel limit"
    )]
    ImagePixelLimitExceeded {
        width: u32,
        height: u32,
        actual_pixels: u64,
        limit_pixels: u64,
    },

    #[error(
        "HEIF container has {actual_frames} top-level images, exceeding the {limit_frames}-frame limit"
    )]
    ImageFrameLimitExceeded {
        actual_frames: usize,
        limit_frames: usize,
    },

    #[error("HEIF decode failed during {stage}: {message}")]
    HeifDecodeFailed {
        stage: &'static str,
        message: String,
    },

    #[error("HEIF page {page_num} is out of range; the primary image is page 1")]
    InvalidPageIndex { page_num: u32 },
}

impl HeifSourceError {
    /// Stable machine-readable error code used by the public OCR API.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidConfiguration { .. } => "invalid_options",
            Self::Io { .. } | Self::HeifDecodeFailed { .. } => "heif_decode_failed",
            Self::ImageFileLimitExceeded { .. } => "image_file_limit_exceeded",
            Self::ImagePixelLimitExceeded { .. } => "image_pixel_limit_exceeded",
            Self::ImageFrameLimitExceeded { .. } => "image_frame_limit_exceeded",
            Self::InvalidPageIndex { .. } => "invalid_page_index",
        }
    }
}

/// A single-page source containing the transformed primary HEIC/HEIF image.
///
/// Rotation, mirroring, and cropping properties stored in the container are
/// applied by libheif while decoding. The decoded pixels are cached so the
/// pipeline's thumbnail and OCR passes cannot trigger duplicate native decodes.
pub struct HeifPageSource {
    path: PathBuf,
    primary_image: GrayImage,
    dpi: f64,
    top_level_image_count: usize,
    warnings: Vec<HeifSourceWarning>,
    libheif_version: [u8; 3],
}

impl HeifPageSource {
    pub fn open(path: impl AsRef<Path>, dpi: f64) -> Result<Self, HeifSourceError> {
        Self::open_with_limits(path, dpi, HeifSourceLimits::default())
    }

    pub fn open_with_limits(
        path: impl AsRef<Path>,
        dpi: f64,
        limits: HeifSourceLimits,
    ) -> Result<Self, HeifSourceError> {
        validate_configuration(dpi, limits)?;

        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path).map_err(|source| HeifSourceError::Io {
            path: path.clone(),
            source,
        })?;
        let file_size = file
            .metadata()
            .map_err(|source| HeifSourceError::Io {
                path: path.clone(),
                source,
            })?
            .len();
        if file_size > limits.max_file_bytes {
            return Err(HeifSourceError::ImageFileLimitExceeded {
                path,
                actual_bytes: file_size,
                limit_bytes: limits.max_file_bytes,
            });
        }

        validate_signature(&path, &mut file)?;
        file.seek(SeekFrom::Start(0))
            .map_err(|source| HeifSourceError::Io {
                path: path.clone(),
                source,
            })?;

        let libheif = LibHeif::new();
        let libheif_version = libheif.version();
        let mut context =
            HeifContext::new().map_err(|error| decode_error("create_context", error))?;
        context.set_max_decoding_threads(limits.max_decoding_threads);
        context
            .read_reader(Box::new(StreamReader::new(file, file_size)))
            .map_err(|error| decode_error("read_container", error))?;

        let image_ids = context.image_ids();
        let top_level_image_count = image_ids.len();
        if top_level_image_count > limits.max_frames {
            return Err(HeifSourceError::ImageFrameLimitExceeded {
                actual_frames: top_level_image_count,
                limit_frames: limits.max_frames,
            });
        }

        let handle = context
            .primary_image_handle()
            .map_err(|error| decode_error("primary_image", error))?;
        enforce_pixel_limit(handle.width(), handle.height(), limits.max_pixels)?;

        // Keep libheif's upstream decoder-side security limits unchanged.
        // `heif_security_limits` is a versioned C struct; round-tripping a
        // value through bindings built for an older ABI can truncate fields
        // added by a newer bundled DLL and corrupt limits such as total memory.
        // The application still enforces file, frame, and pixel limits before
        // decoding and re-checks the decoded dimensions below.

        let mut decoding_options =
            DecodingOptions::new().ok_or_else(|| HeifSourceError::HeifDecodeFailed {
                stage: "create_decoding_options",
                message: "libheif did not allocate decoding options".to_string(),
            })?;
        decoding_options.set_ignore_transformations(false);
        decoding_options.set_convert_hdr_to_8bit(true);
        decoding_options.set_strict_decoding(true);

        let decoded = libheif
            .decode(
                &handle,
                ColorSpace::Rgb(RgbChroma::Rgba),
                Some(decoding_options),
            )
            .map_err(|error| decode_error("decode_primary_image", error))?;
        enforce_pixel_limit(decoded.width(), decoded.height(), limits.max_pixels)?;
        let primary_image = decoded_rgba_to_gray(&decoded)?;

        let warnings = (top_level_image_count > 1)
            .then(|| HeifSourceWarning::AdditionalTopLevelImagesIgnored {
                total_images: top_level_image_count,
                ignored_images: top_level_image_count - 1,
            })
            .into_iter()
            .collect();

        Ok(Self {
            path,
            primary_image,
            dpi,
            top_level_image_count,
            warnings,
            libheif_version,
        })
    }

    pub fn source_path(&self) -> &Path {
        &self.path
    }

    pub fn top_level_image_count(&self) -> usize {
        self.top_level_image_count
    }

    pub fn warnings(&self) -> &[HeifSourceWarning] {
        &self.warnings
    }

    pub fn libheif_version(&self) -> [u8; 3] {
        self.libheif_version
    }
}

impl PageSource for HeifPageSource {
    fn total_pages(&self) -> Result<u32, Box<dyn std::error::Error>> {
        Ok(1)
    }

    fn render_page(&mut self, page_num: u32) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        if page_num != 1 {
            return Err(Box::new(HeifSourceError::InvalidPageIndex { page_num }));
        }
        Ok(RenderedPage::new(1, self.primary_image.clone(), self.dpi))
    }
}

fn validate_configuration(dpi: f64, limits: HeifSourceLimits) -> Result<(), HeifSourceError> {
    if !dpi.is_finite() || dpi <= 0.0 {
        return Err(HeifSourceError::InvalidConfiguration {
            message: format!("DPI must be finite and greater than zero, got {dpi}"),
        });
    }
    if limits.max_file_bytes == 0 {
        return Err(HeifSourceError::InvalidConfiguration {
            message: "max_file_bytes must be greater than zero".to_string(),
        });
    }
    if limits.max_pixels == 0 {
        return Err(HeifSourceError::InvalidConfiguration {
            message: "max_pixels must be greater than zero".to_string(),
        });
    }
    if limits.max_frames == 0 {
        return Err(HeifSourceError::InvalidConfiguration {
            message: "max_frames must be greater than zero".to_string(),
        });
    }
    Ok(())
}

fn validate_signature(path: &Path, file: &mut File) -> Result<(), HeifSourceError> {
    let mut signature = [0_u8; 12];
    file.read_exact(&mut signature)
        .map_err(|error| HeifSourceError::HeifDecodeFailed {
            stage: "signature",
            message: format!("cannot read the required 12-byte HEIF signature: {error}"),
        })?;

    if check_file_type(&signature) == FileTypeResult::No {
        return Err(HeifSourceError::HeifDecodeFailed {
            stage: "signature",
            message: format!("{} does not have a HEIF-family signature", path.display()),
        });
    }
    Ok(())
}

fn enforce_pixel_limit(width: u32, height: u32, limit_pixels: u64) -> Result<(), HeifSourceError> {
    if width == 0 || height == 0 {
        return Err(HeifSourceError::HeifDecodeFailed {
            stage: "image_dimensions",
            message: format!("decoded image has invalid dimensions {width}x{height}"),
        });
    }
    let actual_pixels = u64::from(width).checked_mul(u64::from(height)).ok_or(
        HeifSourceError::ImagePixelLimitExceeded {
            width,
            height,
            actual_pixels: u64::MAX,
            limit_pixels,
        },
    )?;
    if actual_pixels > limit_pixels {
        return Err(HeifSourceError::ImagePixelLimitExceeded {
            width,
            height,
            actual_pixels,
            limit_pixels,
        });
    }
    Ok(())
}

fn decoded_rgba_to_gray(decoded: &libheif_rs::Image) -> Result<GrayImage, HeifSourceError> {
    let width = decoded.width();
    let height = decoded.height();
    let plane = decoded
        .planes()
        .interleaved
        .ok_or_else(|| HeifSourceError::HeifDecodeFailed {
            stage: "read_rgba_plane",
            message: "decoded image has no interleaved RGBA plane".to_string(),
        })?;

    if plane.width != width || plane.height != height {
        return Err(HeifSourceError::HeifDecodeFailed {
            stage: "read_rgba_plane",
            message: format!(
                "RGBA plane dimensions {}x{} differ from decoded image dimensions {width}x{height}",
                plane.width, plane.height
            ),
        });
    }
    if plane.storage_bits_per_pixel != 32 {
        return Err(HeifSourceError::HeifDecodeFailed {
            stage: "read_rgba_plane",
            message: format!(
                "expected 32 storage bits per interleaved RGBA pixel, got {}",
                plane.storage_bits_per_pixel
            ),
        });
    }

    let packed_row_bytes = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| HeifSourceError::HeifDecodeFailed {
            stage: "read_rgba_plane",
            message: format!("RGBA row byte count overflows for width {width}"),
        })?;
    if plane.stride < packed_row_bytes {
        return Err(HeifSourceError::HeifDecodeFailed {
            stage: "read_rgba_plane",
            message: format!(
                "RGBA stride {} is smaller than packed row size {packed_row_bytes}",
                plane.stride
            ),
        });
    }
    let required_bytes = plane
        .stride
        .checked_mul(usize::try_from(height).map_err(|error| {
            HeifSourceError::HeifDecodeFailed {
                stage: "read_rgba_plane",
                message: format!("height conversion failed: {error}"),
            }
        })?)
        .ok_or_else(|| HeifSourceError::HeifDecodeFailed {
            stage: "read_rgba_plane",
            message: "RGBA plane byte count overflows".to_string(),
        })?;
    if plane.data.len() < required_bytes {
        return Err(HeifSourceError::HeifDecodeFailed {
            stage: "read_rgba_plane",
            message: format!(
                "RGBA buffer has {} bytes, but stride and height require {required_bytes}",
                plane.data.len()
            ),
        });
    }

    rgba_rows_to_gray(
        plane.data,
        width,
        height,
        plane.stride,
        decoded.is_premultiplied_alpha(),
    )
}

fn rgba_rows_to_gray(
    rgba: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    premultiplied_alpha: bool,
) -> Result<GrayImage, HeifSourceError> {
    let packed_row_bytes = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| HeifSourceError::HeifDecodeFailed {
            stage: "convert_rgba",
            message: "packed RGBA row byte count overflows".to_string(),
        })?;
    if stride < packed_row_bytes {
        return Err(HeifSourceError::HeifDecodeFailed {
            stage: "convert_rgba",
            message: format!("RGBA stride {stride} is smaller than row size {packed_row_bytes}"),
        });
    }
    let height_usize =
        usize::try_from(height).map_err(|error| HeifSourceError::HeifDecodeFailed {
            stage: "convert_rgba",
            message: format!("height conversion failed: {error}"),
        })?;
    let required_bytes =
        stride
            .checked_mul(height_usize)
            .ok_or_else(|| HeifSourceError::HeifDecodeFailed {
                stage: "convert_rgba",
                message: "RGBA source byte count overflows".to_string(),
            })?;
    if rgba.len() < required_bytes {
        return Err(HeifSourceError::HeifDecodeFailed {
            stage: "convert_rgba",
            message: format!(
                "RGBA buffer has {} bytes, but {required_bytes} bytes are required",
                rgba.len()
            ),
        });
    }

    let pixel_count = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(height_usize))
        .ok_or_else(|| HeifSourceError::HeifDecodeFailed {
            stage: "convert_rgba",
            message: "grayscale output size overflows".to_string(),
        })?;
    let mut gray = Vec::with_capacity(pixel_count);

    for row in 0..height_usize {
        let offset = row * stride;
        let row_pixels = &rgba[offset..offset + packed_row_bytes];
        for pixel in row_pixels.chunks_exact(4) {
            let alpha = u32::from(pixel[3]);
            let composite = |channel: u8| -> u8 {
                if premultiplied_alpha {
                    u32::from(channel).saturating_add(255 - alpha).min(255) as u8
                } else {
                    ((u32::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
                }
            };
            let red = u32::from(composite(pixel[0]));
            let green = u32::from(composite(pixel[1]));
            let blue = u32::from(composite(pixel[2]));
            gray.push(((77 * red + 150 * green + 29 * blue + 128) >> 8) as u8);
        }
    }

    GrayImage::from_raw(width, height, gray).ok_or_else(|| HeifSourceError::HeifDecodeFailed {
        stage: "convert_rgba",
        message: format!("cannot construct a grayscale image of size {width}x{height}"),
    })
}

fn decode_error(stage: &'static str, error: libheif_rs::HeifError) -> HeifSourceError {
    HeifSourceError::HeifDecodeFailed {
        stage,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_conversion_honors_stride_and_transparency() {
        let rgba = [
            255, 0, 0, 255, // opaque red
            0, 0, 0, 0, // transparent black becomes white
            99, 98, 97, 96, // row padding
            0, 255, 0, 128, // half-transparent green on white
            0, 0, 255, 255, // opaque blue
            11, 12, 13, 14, // row padding
        ];

        let image = rgba_rows_to_gray(&rgba, 2, 2, 12, false).expect("convert RGBA");
        assert_eq!(image.dimensions(), (2, 2));
        assert_eq!(image.as_raw().len(), 4);
        assert_eq!(image.get_pixel(1, 0).0[0], 255);
        assert!(image.get_pixel(0, 1).0[0] > image.get_pixel(1, 1).0[0]);
    }

    #[test]
    fn premultiplied_alpha_is_composited_without_multiplying_twice() {
        // Straight red=200 at alpha=128 is stored as approximately 100 when
        // premultiplied. Compositing on white must produce approximately 227.
        let rgba = [100, 0, 0, 128];
        let image = rgba_rows_to_gray(&rgba, 1, 1, 4, true).expect("convert RGBA");
        let expected_red = 227_u32;
        let expected_green_blue = 127_u32;
        let expected =
            ((77 * expected_red + 150 * expected_green_blue + 29 * expected_green_blue + 128) >> 8)
                as u8;
        assert_eq!(image.get_pixel(0, 0).0[0], expected);
    }

    #[test]
    fn pixel_limit_is_checked_without_overflow() {
        let error = enforce_pixel_limit(10_000, 10_000, 50_000_000)
            .expect_err("image must exceed the limit");
        assert_eq!(error.code(), "image_pixel_limit_exceeded");
    }

    #[test]
    fn invalid_limits_are_rejected() {
        let error = validate_configuration(
            DEFAULT_HEIF_DPI,
            HeifSourceLimits {
                max_frames: 0,
                ..HeifSourceLimits::default()
            },
        )
        .expect_err("zero frame limit must fail");
        assert_eq!(error.code(), "invalid_options");
    }
}
