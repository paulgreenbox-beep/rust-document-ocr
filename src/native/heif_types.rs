//! Backend-neutral HEIF limits, warnings, and stable errors.

use std::path::PathBuf;

use thiserror::Error;

pub const DEFAULT_HEIF_DPI: f64 = 300.0;
pub const DEFAULT_HEIF_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_HEIF_MAX_PIXELS: u64 = 50_000_000;
pub const DEFAULT_HEIF_MAX_FRAMES: usize = 64;

/// Resource limits shared by the HEIF decoders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeifSourceLimits {
    pub max_file_bytes: u64,
    pub max_pixels: u64,
    pub max_frames: usize,
    /// Limits tile decoding inside libheif. `0` decodes tiles on the caller's
    /// thread; `1` uses one libheif worker. The heic-rs adapter stays serial.
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
