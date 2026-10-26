//! Input format detection and resource limits shared by native page sources.
//!
//! File extensions are intentionally not consulted. A caller may use an
//! arbitrary file name, while a misleading extension cannot force a decoder.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use thiserror::Error;

/// Maximum prefix needed by the supported signature probes.
const SIGNATURE_PREFIX_BYTES: usize = 4 * 1024;

/// Hard limits applied before a decoder allocates a full page bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputLimits {
    pub max_file_bytes: u64,
    pub max_pixels_per_frame: u64,
    pub max_frames: u32,
}

impl Default for InputLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 512 * 1024 * 1024,
            max_pixels_per_frame: 50_000_000,
            max_frames: 300,
        }
    }
}

/// A format selected only from the input byte signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Pdf,
    Jpeg,
    Png,
    Bmp,
    Tiff,
    WebP,
    Heif,
    Djvu,
}

impl InputFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
            Self::WebP => "webp",
            Self::Heif => "heif",
            Self::Djvu => "djvu",
        }
    }
}

/// Source errors retain stable codes even when the human-readable detail
/// changes. The API layer should preserve these errors instead of stringifying
/// them inside the parallel producer.
#[derive(Debug, Error)]
pub enum InputSourceError {
    #[error("unsupported input format: {path}")]
    UnsupportedInputFormat { path: PathBuf },

    #[error("failed to read input {path}: {detail}")]
    InputReadFailed { path: PathBuf, detail: String },

    #[error("image decode failed for {path}: {detail}")]
    ImageDecodeFailed { path: PathBuf, detail: String },

    #[error("HEIF decode failed for {path}: {detail}")]
    HeifDecodeFailed { path: PathBuf, detail: String },

    #[error("DjVu decode failed for {path}: {detail}")]
    DjvuDecodeFailed { path: PathBuf, detail: String },

    #[error(
        "image frame {frame_number} is {width}x{height} ({pixels} pixels), exceeding the limit of {max_pixels}"
    )]
    ImagePixelLimitExceeded {
        frame_number: u32,
        width: u32,
        height: u32,
        pixels: u64,
        max_pixels: u64,
    },

    #[error("input contains at least {frames} frames, exceeding the limit of {max_frames}")]
    ImageFrameLimitExceeded { frames: u32, max_frames: u32 },

    #[error("input file is {bytes} bytes, exceeding the limit of {max_bytes}")]
    InputFileSizeLimitExceeded { bytes: u64, max_bytes: u64 },

    #[error("page {page_number} is out of range for {page_count} pages")]
    PageOutOfRange { page_number: u32, page_count: u32 },
}

impl InputSourceError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedInputFormat { .. } => "unsupported_input_format",
            Self::InputReadFailed { .. } => "input_read_failed",
            Self::ImageDecodeFailed { .. } => "image_decode_failed",
            Self::HeifDecodeFailed { .. } => "heif_decode_failed",
            Self::DjvuDecodeFailed { .. } => "djvu_decode_failed",
            Self::ImagePixelLimitExceeded { .. } => "image_pixel_limit_exceeded",
            Self::ImageFrameLimitExceeded { .. } => "image_frame_limit_exceeded",
            Self::InputFileSizeLimitExceeded { .. } => "input_file_size_limit_exceeded",
            Self::PageOutOfRange { .. } => "invalid_page_index",
        }
    }
}

/// Detects a supported input from a bounded prefix, without using its suffix.
pub fn detect_input_format(
    path: &Path,
    limits: InputLimits,
) -> Result<InputFormat, InputSourceError> {
    let metadata = std::fs::metadata(path).map_err(|error| InputSourceError::InputReadFailed {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    if !metadata.is_file() {
        return Err(InputSourceError::InputReadFailed {
            path: path.to_path_buf(),
            detail: "input is not a regular file".to_string(),
        });
    }
    if metadata.len() > limits.max_file_bytes {
        return Err(InputSourceError::InputFileSizeLimitExceeded {
            bytes: metadata.len(),
            max_bytes: limits.max_file_bytes,
        });
    }

    let mut file = File::open(path).map_err(|error| InputSourceError::InputReadFailed {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let mut prefix = vec![0_u8; SIGNATURE_PREFIX_BYTES];
    let read = file
        .read(&mut prefix)
        .map_err(|error| InputSourceError::InputReadFailed {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
    // Leave a reusable file handle at the beginning if this function is ever
    // adapted to accept one from a host.
    let _ = file.seek(SeekFrom::Start(0));
    prefix.truncate(read);

    detect_input_format_bytes(&prefix).ok_or_else(|| InputSourceError::UnsupportedInputFormat {
        path: path.to_path_buf(),
    })
}

/// Pure signature detector used by the dispatcher and unit tests.
pub fn detect_input_format_bytes(bytes: &[u8]) -> Option<InputFormat> {
    if bytes.starts_with(b"%PDF-") {
        return Some(InputFormat::Pdf);
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(InputFormat::Jpeg);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(InputFormat::Png);
    }
    if bytes.starts_with(b"BM") {
        return Some(InputFormat::Bmp);
    }
    if bytes.starts_with(b"II*\0")
        || bytes.starts_with(b"MM\0*")
        || bytes.starts_with(b"II+\0")
        || bytes.starts_with(b"MM\0+")
    {
        return Some(InputFormat::Tiff);
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(InputFormat::WebP);
    }
    if is_heif_signature(bytes) {
        return Some(InputFormat::Heif);
    }
    if bytes.len() >= 16
        && bytes.starts_with(b"AT&TFORM")
        && matches!(&bytes[12..16], b"DJVU" | b"DJVM" | b"DJVI")
    {
        return Some(InputFormat::Djvu);
    }
    None
}

/// Validates dimensions with checked arithmetic before any full-frame buffer
/// allocation.
pub fn validate_frame_dimensions(
    frame_number: u32,
    width: u32,
    height: u32,
    limits: InputLimits,
) -> Result<(), InputSourceError> {
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or(
        InputSourceError::ImagePixelLimitExceeded {
            frame_number,
            width,
            height,
            pixels: u64::MAX,
            max_pixels: limits.max_pixels_per_frame,
        },
    )?;
    if width == 0 || height == 0 || pixels > limits.max_pixels_per_frame {
        return Err(InputSourceError::ImagePixelLimitExceeded {
            frame_number,
            width,
            height,
            pixels,
            max_pixels: limits.max_pixels_per_frame,
        });
    }
    Ok(())
}

pub fn validate_frame_count(frames: u32, limits: InputLimits) -> Result<(), InputSourceError> {
    if frames > limits.max_frames {
        return Err(InputSourceError::ImageFrameLimitExceeded {
            frames,
            max_frames: limits.max_frames,
        });
    }
    Ok(())
}

fn is_heif_signature(bytes: &[u8]) -> bool {
    // ISO BMFF: size (4), "ftyp" (4), major brand (4), minor version (4),
    // then compatible brands. Only inspect the bounded ftyp box.
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    let box_size = u32::from_be_bytes(bytes[0..4].try_into().expect("four-byte slice")) as usize;
    if box_size < 12 {
        return false;
    }
    let end = box_size.min(bytes.len());
    let mut offsets = vec![8_usize];
    offsets.extend((16..end).step_by(4));
    offsets.into_iter().any(|offset| {
        bytes
            .get(offset..offset + 4)
            .is_some_and(|brand| is_heif_brand(brand.try_into().expect("four-byte brand")))
    })
}

fn is_heif_brand(brand: &[u8; 4]) -> bool {
    matches!(
        brand,
        b"heic"
            | b"heix"
            | b"hevc"
            | b"hevx"
            | b"heim"
            | b"heis"
            | b"hevm"
            | b"hevs"
            | b"mif1"
            | b"msf1"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        detect_input_format_bytes, validate_frame_count, validate_frame_dimensions, InputFormat,
        InputLimits, InputSourceError,
    };

    #[test]
    fn detects_common_raster_signatures_without_extensions() {
        assert_eq!(
            detect_input_format_bytes(b"\xff\xd8\xff\xe0rest"),
            Some(InputFormat::Jpeg)
        );
        assert_eq!(
            detect_input_format_bytes(b"\x89PNG\r\n\x1a\nrest"),
            Some(InputFormat::Png)
        );
        assert_eq!(detect_input_format_bytes(b"BMrest"), Some(InputFormat::Bmp));
        assert_eq!(
            detect_input_format_bytes(b"II*\0rest"),
            Some(InputFormat::Tiff)
        );
        assert_eq!(
            detect_input_format_bytes(b"MM\0+rest"),
            Some(InputFormat::Tiff)
        );

        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&[12, 0, 0, 0]);
        webp.extend_from_slice(b"WEBPVP8 ");
        assert_eq!(detect_input_format_bytes(&webp), Some(InputFormat::WebP));
    }

    #[test]
    fn detects_pdf_heif_and_djvu_signatures() {
        assert_eq!(
            detect_input_format_bytes(b"%PDF-1.7\n"),
            Some(InputFormat::Pdf)
        );
        assert_eq!(
            detect_input_format_bytes(b"\0\0\0\x18ftypheic\0\0\0\0mif1"),
            Some(InputFormat::Heif)
        );
        assert_eq!(
            detect_input_format_bytes(b"AT&TFORM\0\0\0\x10DJVM"),
            Some(InputFormat::Djvu)
        );
    }

    #[test]
    fn does_not_trust_unknown_or_partial_signatures() {
        assert_eq!(detect_input_format_bytes(b"not an image.pdf"), None);
        assert_eq!(detect_input_format_bytes(b"RIFFshort"), None);
        assert_eq!(detect_input_format_bytes(b"\0\0\0\x08ftypheic"), None);
    }

    #[test]
    fn enforces_pixel_and_frame_limits_with_stable_codes() {
        let limits = InputLimits {
            max_file_bytes: 100,
            max_pixels_per_frame: 12,
            max_frames: 2,
        };
        validate_frame_dimensions(1, 3, 4, limits).unwrap();
        let pixels = validate_frame_dimensions(2, 4, 4, limits).unwrap_err();
        assert_eq!(pixels.code(), "image_pixel_limit_exceeded");
        assert!(matches!(
            pixels,
            InputSourceError::ImagePixelLimitExceeded {
                frame_number: 2,
                ..
            }
        ));

        let frames = validate_frame_count(3, limits).unwrap_err();
        assert_eq!(frames.code(), "image_frame_limit_exceeded");
    }
}
