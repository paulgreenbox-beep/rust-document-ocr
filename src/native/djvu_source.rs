//! Pure-Rust DjVu page source.
//!
//! The decoder is deliberately isolated behind [`PageSource`]. It parses the
//! document from a bounded byte buffer, validates every page before rendering,
//! and converts the decoder's packed RGBA output into the grayscale raster used
//! by the existing OCR pipeline.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use djvu_rs::djvu_render::{render_pixmap, RenderError, RenderOptions};
use djvu_rs::{DjVuDocument, DocError, Rotation};
use image::GrayImage;
use thiserror::Error;

use crate::page_source::{dimensions_within_pixel_limit, PageSource, RenderedPage};

const DJVU_SIGNATURE: &[u8; 8] = b"AT&TFORM";

/// Resource limits applied before a DjVu document is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DjVuLimits {
    max_input_bytes: u64,
    max_frames: u32,
    max_pixels_per_page: usize,
}

impl DjVuLimits {
    pub fn new(
        max_input_bytes: u64,
        max_frames: u32,
        max_pixels_per_page: usize,
    ) -> Result<Self, DjVuSourceError> {
        let limits = Self {
            max_input_bytes,
            max_frames,
            max_pixels_per_page,
        };
        limits.validate()?;
        Ok(limits)
    }

    pub fn max_input_bytes(self) -> u64 {
        self.max_input_bytes
    }

    pub fn max_frames(self) -> u32 {
        self.max_frames
    }

    pub fn max_pixels_per_page(self) -> usize {
        self.max_pixels_per_page
    }

    fn validate(self) -> Result<(), DjVuSourceError> {
        if self.max_input_bytes == 0 {
            return Err(DjVuSourceError::InvalidLimit {
                name: "max_input_bytes",
            });
        }
        if self.max_frames == 0 {
            return Err(DjVuSourceError::InvalidLimit { name: "max_frames" });
        }
        if self.max_pixels_per_page == 0 {
            return Err(DjVuSourceError::InvalidLimit {
                name: "max_pixels_per_page",
            });
        }
        Ok(())
    }
}

/// Typed failures produced by [`DjVuPageSource`].
#[derive(Debug, Error)]
pub enum DjVuSourceError {
    #[error("DjVu limit {name} must be greater than zero")]
    InvalidLimit { name: &'static str },

    #[error("failed to read DjVu input {path}: {source}")]
    InputRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("DjVu input is {actual_bytes} bytes; limit is {max_bytes} bytes")]
    InputLimitExceeded { actual_bytes: u64, max_bytes: u64 },

    #[error("input does not have the DjVu AT&TFORM signature")]
    InvalidSignature,

    #[error("failed to decode DjVu document: {source}")]
    DocumentDecode {
        #[source]
        source: DocError,
    },

    #[error("DjVu document contains no pages")]
    EmptyDocument,

    #[error("DjVu document has {actual_frames} pages; limit is {max_frames}")]
    FrameLimitExceeded {
        actual_frames: usize,
        max_frames: u32,
    },

    #[error("DjVu page {page_num} is {width}x{height} ({pixels} pixels); limit is {max_pixels}")]
    PixelLimitExceeded {
        page_num: u32,
        width: u32,
        height: u32,
        pixels: usize,
        max_pixels: usize,
    },

    #[error("DjVu page numbers are 1-indexed")]
    InvalidPageNumber,

    #[error("DjVu page {page_num} is out of range (document has {page_count} pages)")]
    PageOutOfRange { page_num: u32, page_count: u32 },

    #[error("DjVu page {page_num} has invalid source DPI {dpi}")]
    InvalidPageDpi { page_num: u32, dpi: u16 },

    #[error("failed to render DjVu page {page_num}: {source}")]
    PageRender {
        page_num: u32,
        #[source]
        source: RenderError,
    },

    #[error(
        "DjVu page {page_num} renderer returned {actual_width}x{actual_height}; expected {expected_width}x{expected_height}"
    )]
    UnexpectedRenderDimensions {
        page_num: u32,
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
    },

    #[error(
        "DjVu page {page_num} RGBA buffer has {actual_bytes} bytes; expected {expected_bytes}"
    )]
    InvalidRenderBuffer {
        page_num: u32,
        expected_bytes: usize,
        actual_bytes: usize,
    },

    #[error("DjVu page {page_num} dimensions overflow the address space")]
    DimensionOverflow { page_num: u32 },

    #[error("failed to construct grayscale image for DjVu page {page_num}")]
    GrayImageConstruction { page_num: u32 },
}

impl DjVuSourceError {
    /// Stable machine-readable category used by the public OCR error mapper.
    pub fn code(&self) -> &'static str {
        match self {
            Self::FrameLimitExceeded { .. } => "image_frame_limit_exceeded",
            Self::PixelLimitExceeded { .. } => "image_pixel_limit_exceeded",
            _ => "djvu_decode_failed",
        }
    }
}

/// Multi-page DjVu source backed by `djvu-rs`.
pub struct DjVuPageSource {
    document: DjVuDocument,
    page_count: u32,
    limits: DjVuLimits,
    source_path: Option<PathBuf>,
}

impl DjVuPageSource {
    /// Opens a DjVu document after bounded, non-streaming input validation.
    pub fn open(path: impl AsRef<Path>, limits: DjVuLimits) -> Result<Self, DjVuSourceError> {
        limits.validate()?;
        let path = path.as_ref();
        let bytes = read_file_bounded(path, limits.max_input_bytes)?;
        let mut source = Self::from_bytes(&bytes, limits)?;
        source.source_path = Some(path.to_path_buf());
        Ok(source)
    }

    /// Parses a DjVu document from already-bounded bytes.
    ///
    /// Bundled multi-page `FORM:DJVM` documents are supported. Indirect
    /// documents are rejected by `DjVuDocument::parse()` because resolving
    /// arbitrary companion paths would bypass the input byte limit.
    pub fn from_bytes(bytes: &[u8], limits: DjVuLimits) -> Result<Self, DjVuSourceError> {
        limits.validate()?;
        let actual_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if actual_bytes > limits.max_input_bytes {
            return Err(DjVuSourceError::InputLimitExceeded {
                actual_bytes,
                max_bytes: limits.max_input_bytes,
            });
        }
        if !bytes.starts_with(DJVU_SIGNATURE) {
            return Err(DjVuSourceError::InvalidSignature);
        }

        let document = DjVuDocument::parse(bytes)
            .map_err(|source| DjVuSourceError::DocumentDecode { source })?;
        let actual_frames = document.page_count();
        if actual_frames == 0 {
            return Err(DjVuSourceError::EmptyDocument);
        }
        if actual_frames > limits.max_frames as usize {
            return Err(DjVuSourceError::FrameLimitExceeded {
                actual_frames,
                max_frames: limits.max_frames,
            });
        }

        let page_count =
            u32::try_from(actual_frames).map_err(|_| DjVuSourceError::FrameLimitExceeded {
                actual_frames,
                max_frames: limits.max_frames,
            })?;
        for index in 0..actual_frames {
            let page = document
                .page(index)
                .map_err(|source| DjVuSourceError::DocumentDecode { source })?;
            let page_num = u32::try_from(index + 1)
                .map_err(|_| DjVuSourceError::DimensionOverflow { page_num: u32::MAX })?;
            validate_page_metadata(page_num, page.width(), page.height(), page.dpi(), limits)?;
        }

        Ok(Self {
            document,
            page_count,
            limits,
            source_path: None,
        })
    }

    pub fn source_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    pub fn limits(&self) -> DjVuLimits {
        self.limits
    }

    fn page(&self, page_num: u32) -> Result<&djvu_rs::DjVuPage, DjVuSourceError> {
        if page_num == 0 {
            return Err(DjVuSourceError::InvalidPageNumber);
        }
        if page_num > self.page_count {
            return Err(DjVuSourceError::PageOutOfRange {
                page_num,
                page_count: self.page_count,
            });
        }
        let index = usize::try_from(page_num - 1).map_err(|_| DjVuSourceError::PageOutOfRange {
            page_num,
            page_count: self.page_count,
        })?;
        self.document
            .page(index)
            .map_err(|_| DjVuSourceError::PageOutOfRange {
                page_num,
                page_count: self.page_count,
            })
    }

    fn render_at_native_dimensions(
        &self,
        page_num: u32,
        native_width: u32,
        native_height: u32,
        dpi: f64,
        pixel_limit: usize,
    ) -> Result<RenderedPage, DjVuSourceError> {
        validate_pixel_count(
            page_num,
            native_width,
            native_height,
            pixel_limit.min(self.limits.max_pixels_per_page),
        )?;
        let page = self.page(page_num)?;
        let options = RenderOptions {
            width: native_width,
            height: native_height,
            ..Default::default()
        };
        let pixmap = render_pixmap(page, &options)
            .map_err(|source| DjVuSourceError::PageRender { page_num, source })?;

        let (expected_width, expected_height) =
            rotated_dimensions(native_width, native_height, page.rotation());
        if (pixmap.width, pixmap.height) != (expected_width, expected_height) {
            return Err(DjVuSourceError::UnexpectedRenderDimensions {
                page_num,
                expected_width,
                expected_height,
                actual_width: pixmap.width,
                actual_height: pixmap.height,
            });
        }

        let image = rgba_to_gray(page_num, pixmap.width, pixmap.height, &pixmap.data)?;
        Ok(RenderedPage::new(page_num, image, dpi))
    }
}

impl PageSource for DjVuPageSource {
    fn total_pages(&self) -> Result<u32, Box<dyn std::error::Error>> {
        Ok(self.page_count)
    }

    fn render_page(&mut self, page_num: u32) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        let page = self.page(page_num)?;
        let dpi = page.dpi();
        validate_page_metadata(page_num, page.width(), page.height(), dpi, self.limits)?;
        Ok(self.render_at_native_dimensions(
            page_num,
            u32::from(page.width()),
            u32::from(page.height()),
            f64::from(dpi),
            self.limits.max_pixels_per_page,
        )?)
    }

    fn render_page_with_max_pixels(
        &mut self,
        page_num: u32,
        max_pixels: usize,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        if max_pixels == 0 {
            return Err(DjVuSourceError::InvalidLimit { name: "max_pixels" }.into());
        }
        let page = self.page(page_num)?;
        let source_dpi = page.dpi();
        validate_page_metadata(
            page_num,
            page.width(),
            page.height(),
            source_dpi,
            self.limits,
        )?;
        let source_width = u32::from(page.width());
        let source_height = u32::from(page.height());
        let effective_limit = max_pixels.min(self.limits.max_pixels_per_page);
        let (target_width, target_height) =
            dimensions_within_pixel_limit(source_width, source_height, effective_limit)?;
        let scale = f64::from(target_width) / f64::from(source_width);
        Ok(self.render_at_native_dimensions(
            page_num,
            target_width,
            target_height,
            f64::from(source_dpi) * scale,
            effective_limit,
        )?)
    }

    fn render_page_at_dpi(
        &mut self,
        page_num: u32,
        target_dpi: u32,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        if target_dpi == 0 {
            return Err(DjVuSourceError::InvalidLimit { name: "target_dpi" }.into());
        }
        let page = self.page(page_num)?;
        let source_dpi = page.dpi();
        validate_page_metadata(
            page_num,
            page.width(),
            page.height(),
            source_dpi,
            self.limits,
        )?;
        let target_width =
            scaled_dimension(u32::from(page.width()), target_dpi, source_dpi, page_num)?;
        let target_height =
            scaled_dimension(u32::from(page.height()), target_dpi, source_dpi, page_num)?;
        Ok(self.render_at_native_dimensions(
            page_num,
            target_width,
            target_height,
            f64::from(target_dpi),
            self.limits.max_pixels_per_page,
        )?)
    }

    fn render_table_thumbnail(
        &mut self,
        page_num: u32,
        max_dimension: u32,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        if max_dimension == 0 {
            return Err(DjVuSourceError::InvalidLimit {
                name: "max_dimension",
            }
            .into());
        }
        let page = self.page(page_num)?;
        let source_dpi = page.dpi();
        validate_page_metadata(
            page_num,
            page.width(),
            page.height(),
            source_dpi,
            self.limits,
        )?;
        let source_width = u32::from(page.width());
        let source_height = u32::from(page.height());
        let longest = source_width.max(source_height);
        let (target_width, target_height, scale) = if longest <= max_dimension {
            (source_width, source_height, 1.0)
        } else {
            let scale = f64::from(max_dimension) / f64::from(longest);
            (
                (f64::from(source_width) * scale).round().max(1.0) as u32,
                (f64::from(source_height) * scale).round().max(1.0) as u32,
                scale,
            )
        };
        Ok(self.render_at_native_dimensions(
            page_num,
            target_width,
            target_height,
            f64::from(source_dpi) * scale,
            self.limits.max_pixels_per_page,
        )?)
    }
}

fn read_file_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, DjVuSourceError> {
    let metadata = std::fs::metadata(path).map_err(|source| DjVuSourceError::InputRead {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > max_bytes {
        return Err(DjVuSourceError::InputLimitExceeded {
            actual_bytes: metadata.len(),
            max_bytes,
        });
    }

    let file = File::open(path).map_err(|source| DjVuSourceError::InputRead {
        path: path.to_path_buf(),
        source,
    })?;
    let capacity = usize::try_from(metadata.len()).unwrap_or(0);
    let mut bytes = Vec::with_capacity(capacity);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| DjVuSourceError::InputRead {
            path: path.to_path_buf(),
            source,
        })?;
    let actual_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual_bytes > max_bytes {
        return Err(DjVuSourceError::InputLimitExceeded {
            actual_bytes,
            max_bytes,
        });
    }
    Ok(bytes)
}

fn validate_page_metadata(
    page_num: u32,
    width: u16,
    height: u16,
    dpi: u16,
    limits: DjVuLimits,
) -> Result<(), DjVuSourceError> {
    if dpi == 0 {
        return Err(DjVuSourceError::InvalidPageDpi { page_num, dpi });
    }
    validate_pixel_count(
        page_num,
        u32::from(width),
        u32::from(height),
        limits.max_pixels_per_page,
    )
    .map(|_| ())
}

fn validate_pixel_count(
    page_num: u32,
    width: u32,
    height: u32,
    max_pixels: usize,
) -> Result<usize, DjVuSourceError> {
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or(DjVuSourceError::DimensionOverflow { page_num })?;
    if width == 0 || height == 0 || pixels > max_pixels {
        return Err(DjVuSourceError::PixelLimitExceeded {
            page_num,
            width,
            height,
            pixels,
            max_pixels,
        });
    }
    Ok(pixels)
}

fn scaled_dimension(
    source: u32,
    target_dpi: u32,
    source_dpi: u16,
    page_num: u32,
) -> Result<u32, DjVuSourceError> {
    let numerator = u64::from(source)
        .checked_mul(u64::from(target_dpi))
        .ok_or(DjVuSourceError::DimensionOverflow { page_num })?;
    let rounded = numerator
        .checked_add(u64::from(source_dpi) / 2)
        .ok_or(DjVuSourceError::DimensionOverflow { page_num })?
        / u64::from(source_dpi);
    u32::try_from(rounded.max(1)).map_err(|_| DjVuSourceError::DimensionOverflow { page_num })
}

fn rotated_dimensions(width: u32, height: u32, rotation: Rotation) -> (u32, u32) {
    match rotation {
        Rotation::Cw90 | Rotation::Ccw90 => (height, width),
        Rotation::None | Rotation::Rot180 => (width, height),
    }
}

fn rgba_to_gray(
    page_num: u32,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<GrayImage, DjVuSourceError> {
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or(DjVuSourceError::DimensionOverflow { page_num })?;
    let expected_bytes = pixels
        .checked_mul(4)
        .ok_or(DjVuSourceError::DimensionOverflow { page_num })?;
    if rgba.len() != expected_bytes {
        return Err(DjVuSourceError::InvalidRenderBuffer {
            page_num,
            expected_bytes,
            actual_bytes: rgba.len(),
        });
    }

    let mut gray = Vec::with_capacity(pixels);
    for pixel in rgba.chunks_exact(4) {
        let alpha = u32::from(pixel[3]);
        let inverse_alpha = 255 - alpha;
        let red = (u32::from(pixel[0]) * alpha + 255 * inverse_alpha + 127) / 255;
        let green = (u32::from(pixel[1]) * alpha + 255 * inverse_alpha + 127) / 255;
        let blue = (u32::from(pixel[2]) * alpha + 255 * inverse_alpha + 127) / 255;
        let luma = (299 * red + 587 * green + 114 * blue + 500) / 1_000;
        gray.push(luma as u8);
    }

    GrayImage::from_raw(width, height, gray)
        .ok_or(DjVuSourceError::GrayImageConstruction { page_num })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> DjVuLimits {
        DjVuLimits::new(1_024, 4, 1_000_000).expect("valid limits")
    }

    #[test]
    fn limits_reject_zero_values() {
        assert!(matches!(
            DjVuLimits::new(0, 1, 1),
            Err(DjVuSourceError::InvalidLimit {
                name: "max_input_bytes"
            })
        ));
        assert!(matches!(
            DjVuLimits::new(1, 0, 1),
            Err(DjVuSourceError::InvalidLimit { name: "max_frames" })
        ));
        assert!(matches!(
            DjVuLimits::new(1, 1, 0),
            Err(DjVuSourceError::InvalidLimit {
                name: "max_pixels_per_page"
            })
        ));
    }

    #[test]
    fn byte_limit_is_checked_before_parsing() {
        let error = match DjVuPageSource::from_bytes(b"AT&TFORMtoo-large", limits()) {
            Ok(_) => panic!("malformed input must fail"),
            Err(error) => error,
        };
        assert!(matches!(error, DjVuSourceError::DocumentDecode { .. }));

        let strict = DjVuLimits::new(4, 1, 1).expect("valid strict limits");
        let error = match DjVuPageSource::from_bytes(b"AT&TFORM", strict) {
            Ok(_) => panic!("byte limit must run before signature/decode"),
            Err(error) => error,
        };
        assert!(matches!(error, DjVuSourceError::InputLimitExceeded { .. }));
    }

    #[test]
    fn wrong_signature_is_typed_decode_failure() {
        let error = match DjVuPageSource::from_bytes(b"not djvu", limits()) {
            Ok(_) => panic!("wrong signature must fail"),
            Err(error) => error,
        };
        assert!(matches!(&error, DjVuSourceError::InvalidSignature));
        assert_eq!(error.code(), "djvu_decode_failed");
    }

    #[test]
    fn pixel_limit_is_enforced_without_allocation() {
        let error =
            validate_pixel_count(3, 1_001, 1_000, 1_000_000).expect_err("oversized page must fail");
        assert!(matches!(
            &error,
            DjVuSourceError::PixelLimitExceeded {
                page_num: 3,
                pixels: 1_001_000,
                ..
            }
        ));
        assert_eq!(error.code(), "image_pixel_limit_exceeded");
    }

    #[test]
    fn rgba_conversion_preserves_black_white_and_mid_gray() {
        let image = rgba_to_gray(
            1,
            3,
            1,
            &[0, 0, 0, 255, 255, 255, 255, 255, 127, 127, 127, 255],
        )
        .expect("valid RGBA buffer");
        assert_eq!(image.as_raw(), &[0, 255, 127]);
    }

    #[test]
    fn transparent_pixels_are_composited_on_white() {
        let image = rgba_to_gray(1, 1, 1, &[0, 0, 0, 0]).expect("valid transparent RGBA buffer");
        assert_eq!(image.as_raw(), &[255]);
    }

    #[test]
    fn rgba_conversion_rejects_truncated_buffer() {
        let error = rgba_to_gray(7, 2, 1, &[0; 7]).expect_err("truncated buffer must fail");
        assert!(matches!(
            error,
            DjVuSourceError::InvalidRenderBuffer {
                page_num: 7,
                expected_bytes: 8,
                actual_bytes: 7
            }
        ));
    }

    #[test]
    fn rotation_swaps_only_right_angle_dimensions() {
        assert_eq!(rotated_dimensions(640, 480, Rotation::None), (640, 480));
        assert_eq!(rotated_dimensions(640, 480, Rotation::Rot180), (640, 480));
        assert_eq!(rotated_dimensions(640, 480, Rotation::Cw90), (480, 640));
        assert_eq!(rotated_dimensions(640, 480, Rotation::Ccw90), (480, 640));
    }
}
