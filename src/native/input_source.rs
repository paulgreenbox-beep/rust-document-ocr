//! Signature-routed native input sources.
//!
//! This module is the only production dispatcher. File suffixes are never
//! trusted: [`detect_input_format`] inspects a bounded byte prefix before a
//! decoder is selected.

use std::path::{Path, PathBuf};

use crate::{
    dto::Word,
    input_format::{detect_input_format, InputFormat, InputLimits, InputSourceError},
    page_analysis::{NativeTextLayer, NativeTextLayout, PageOrientation},
    page_source::{PageSource, RenderedPage, RenderedPageRegion},
};

use super::{
    djvu_source::{DjVuLimits, DjVuPageSource, DjVuSourceError},
    heif_source::{HeifPageSource, HeifSourceError, HeifSourceLimits, HeifSourceWarning},
    pdfium_source::PdfiumPageSource,
    raster_source::RasterImagePageSource,
    tiff_source::TiffPageSource,
};

/// A non-fatal source-level condition that embedding hosts can serialize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSourceWarning {
    /// HEIF sequences/collections expose only their primary image in v1.
    AdditionalImageFramesIgnored {
        format: &'static str,
        total_frames: usize,
        processed_frames: usize,
    },
}

/// A typed failure produced while selecting or opening a native page source.
#[derive(Debug, thiserror::Error)]
pub enum NativeInputError {
    #[error(transparent)]
    Source(#[from] InputSourceError),

    #[error("failed to open PDF {path}: {detail}")]
    PdfOpenFailed { path: PathBuf, detail: String },
}

impl NativeInputError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Source(error) => error.code(),
            Self::PdfOpenFailed { .. } => "pdf_open_failed",
        }
    }
}

/// The format-neutral source used by the production pipeline.
pub enum NativePageSource {
    Pdf(PdfiumPageSource),
    Raster(RasterImagePageSource),
    Tiff(TiffPageSource),
    Heif(HeifPageSource),
    Djvu(DjVuPageSource),
}

/// Result of opening an input, including additive format warnings.
pub struct OpenedPageSource {
    pub format: InputFormat,
    pub source: NativePageSource,
    pub warnings: Vec<InputSourceWarning>,
}

impl OpenedPageSource {
    /// Opens a supported document by byte signature.
    pub fn open(
        path: &Path,
        nominal_dpi: f64,
        pdfium_dll_path: Option<&Path>,
        limits: InputLimits,
    ) -> Result<Self, NativeInputError> {
        let format = detect_input_format(path, limits)?;
        let mut warnings = Vec::new();
        let source = match format {
            InputFormat::Pdf => NativePageSource::Pdf(
                PdfiumPageSource::open_with_pdfium_dll(path, nominal_dpi, pdfium_dll_path)
                    .map_err(|error| NativeInputError::PdfOpenFailed {
                        path: path.to_path_buf(),
                        detail: error.to_string(),
                    })?,
            ),
            InputFormat::Jpeg | InputFormat::Png | InputFormat::Bmp | InputFormat::WebP => {
                NativePageSource::Raster(RasterImagePageSource::open(path, nominal_dpi, limits)?)
            }
            InputFormat::Tiff => {
                NativePageSource::Tiff(TiffPageSource::open(path, nominal_dpi, limits)?)
            }
            InputFormat::Heif => {
                let source = HeifPageSource::open_with_limits(
                    path,
                    nominal_dpi,
                    HeifSourceLimits {
                        max_file_bytes: limits.max_file_bytes,
                        max_pixels: limits.max_pixels_per_frame,
                        max_frames: usize::try_from(limits.max_frames).unwrap_or(usize::MAX),
                        max_decoding_threads: 1,
                    },
                )
                .map_err(|error| map_heif_error(path, error))?;
                for warning in source.warnings() {
                    match warning {
                        HeifSourceWarning::AdditionalTopLevelImagesIgnored {
                            total_images, ..
                        } => warnings.push(InputSourceWarning::AdditionalImageFramesIgnored {
                            format: "heif",
                            total_frames: *total_images,
                            processed_frames: 1,
                        }),
                    }
                }
                NativePageSource::Heif(source)
            }
            InputFormat::Djvu => {
                let djvu_limits = DjVuLimits::new(
                    limits.max_file_bytes,
                    limits.max_frames,
                    usize::try_from(limits.max_pixels_per_frame).unwrap_or(usize::MAX),
                )
                .map_err(|error| map_djvu_error(path, error))?;
                NativePageSource::Djvu(
                    DjVuPageSource::open(path, djvu_limits)
                        .map_err(|error| map_djvu_error(path, error))?,
                )
            }
        };

        Ok(Self {
            format,
            source,
            warnings,
        })
    }
}

impl PageSource for NativePageSource {
    fn total_pages(&self) -> Result<u32, Box<dyn std::error::Error>> {
        match self {
            Self::Pdf(source) => source.total_pages(),
            Self::Raster(source) => source.total_pages(),
            Self::Tiff(source) => source.total_pages(),
            Self::Heif(source) => source.total_pages(),
            Self::Djvu(source) => source.total_pages(),
        }
    }

    fn render_page(&mut self, page_num: u32) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        match self {
            Self::Pdf(source) => source.render_page(page_num),
            Self::Raster(source) => source.render_page(page_num),
            Self::Tiff(source) => source.render_page(page_num),
            Self::Heif(source) => source.render_page(page_num),
            Self::Djvu(source) => source.render_page(page_num),
        }
    }

    fn render_page_with_max_pixels(
        &mut self,
        page_num: u32,
        max_pixels: usize,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        match self {
            Self::Pdf(source) => source.render_page_with_max_pixels(page_num, max_pixels),
            Self::Raster(source) => source.render_page_with_max_pixels(page_num, max_pixels),
            Self::Tiff(source) => source.render_page_with_max_pixels(page_num, max_pixels),
            Self::Heif(source) => source.render_page_with_max_pixels(page_num, max_pixels),
            Self::Djvu(source) => source.render_page_with_max_pixels(page_num, max_pixels),
        }
    }

    fn render_page_at_dpi(
        &mut self,
        page_num: u32,
        target_dpi: u32,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        match self {
            Self::Pdf(source) => source.render_page_at_dpi(page_num, target_dpi),
            Self::Raster(source) => source.render_page_at_dpi(page_num, target_dpi),
            Self::Tiff(source) => source.render_page_at_dpi(page_num, target_dpi),
            Self::Heif(source) => source.render_page_at_dpi(page_num, target_dpi),
            Self::Djvu(source) => source.render_page_at_dpi(page_num, target_dpi),
        }
    }

    fn render_page_at_dpi_with_max_pixels(
        &mut self,
        page_num: u32,
        target_dpi: u32,
        max_pixels: usize,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        match self {
            Self::Pdf(source) => {
                source.render_page_at_dpi_with_max_pixels(page_num, target_dpi, max_pixels)
            }
            Self::Raster(source) => {
                source.render_page_at_dpi_with_max_pixels(page_num, target_dpi, max_pixels)
            }
            Self::Tiff(source) => {
                source.render_page_at_dpi_with_max_pixels(page_num, target_dpi, max_pixels)
            }
            Self::Heif(source) => {
                source.render_page_at_dpi_with_max_pixels(page_num, target_dpi, max_pixels)
            }
            Self::Djvu(source) => {
                source.render_page_at_dpi_with_max_pixels(page_num, target_dpi, max_pixels)
            }
        }
    }

    fn render_region_at_dpi(
        &mut self,
        page_num: u32,
        target_dpi: u32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        max_pixels: usize,
    ) -> Result<Option<RenderedPageRegion>, Box<dyn std::error::Error>> {
        let Self::Pdf(source) = self else {
            return Ok(None);
        };
        PageSource::render_region_at_dpi(
            source, page_num, target_dpi, x, y, width, height, max_pixels,
        )
    }

    fn native_text_for_page(
        &self,
        page_num: u32,
    ) -> Result<Option<NativeTextLayer>, Box<dyn std::error::Error>> {
        match self {
            Self::Pdf(source) => source.native_text_for_page(page_num),
            _ => Ok(None),
        }
    }

    fn native_text_layout_for_page(
        &self,
        page_num: u32,
    ) -> Result<Option<NativeTextLayout>, Box<dyn std::error::Error>> {
        match self {
            Self::Pdf(source) => source.native_text_layout_for_page(page_num),
            _ => Ok(None),
        }
    }

    fn page_orientation_from_metadata(
        &self,
        page_num: u32,
    ) -> Result<PageOrientation, Box<dyn std::error::Error>> {
        match self {
            Self::Pdf(source) => source.page_orientation_from_metadata(page_num),
            Self::Raster(source) => source.page_orientation_from_metadata(page_num),
            Self::Tiff(source) => source.page_orientation_from_metadata(page_num),
            Self::Heif(source) => source.page_orientation_from_metadata(page_num),
            Self::Djvu(source) => source.page_orientation_from_metadata(page_num),
        }
    }

    fn native_words_for_rendered_page(
        &self,
        page_num: u32,
        width: u32,
        height: u32,
    ) -> Result<Option<Vec<Word>>, Box<dyn std::error::Error>> {
        match self {
            Self::Pdf(source) => source.native_words_for_rendered_page(page_num, width, height),
            _ => Ok(None),
        }
    }

    fn render_table_thumbnail(
        &mut self,
        page_num: u32,
        max_dimension: u32,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        match self {
            Self::Pdf(source) => source.render_table_thumbnail(page_num, max_dimension),
            Self::Raster(source) => source.render_table_thumbnail(page_num, max_dimension),
            Self::Tiff(source) => source.render_table_thumbnail(page_num, max_dimension),
            Self::Heif(source) => source.render_table_thumbnail(page_num, max_dimension),
            Self::Djvu(source) => source.render_table_thumbnail(page_num, max_dimension),
        }
    }
}

fn map_heif_error(path: &Path, error: HeifSourceError) -> InputSourceError {
    let detail = error.to_string();
    match error {
        HeifSourceError::Io { .. } => InputSourceError::InputReadFailed {
            path: path.to_path_buf(),
            detail,
        },
        HeifSourceError::ImageFileLimitExceeded {
            actual_bytes,
            limit_bytes,
            ..
        } => InputSourceError::InputFileSizeLimitExceeded {
            bytes: actual_bytes,
            max_bytes: limit_bytes,
        },
        HeifSourceError::ImagePixelLimitExceeded {
            width,
            height,
            actual_pixels,
            limit_pixels,
        } => InputSourceError::ImagePixelLimitExceeded {
            frame_number: 1,
            width,
            height,
            pixels: actual_pixels,
            max_pixels: limit_pixels,
        },
        HeifSourceError::ImageFrameLimitExceeded {
            actual_frames,
            limit_frames,
        } => InputSourceError::ImageFrameLimitExceeded {
            frames: u32::try_from(actual_frames).unwrap_or(u32::MAX),
            max_frames: u32::try_from(limit_frames).unwrap_or(u32::MAX),
        },
        HeifSourceError::InvalidPageIndex { page_num } => InputSourceError::PageOutOfRange {
            page_number: page_num,
            page_count: 1,
        },
        _ => InputSourceError::HeifDecodeFailed {
            path: path.to_path_buf(),
            detail,
        },
    }
}

pub(crate) fn map_djvu_error(path: &Path, error: DjVuSourceError) -> InputSourceError {
    let detail = error.to_string();
    match error {
        DjVuSourceError::InputRead { .. } => InputSourceError::InputReadFailed {
            path: path.to_path_buf(),
            detail,
        },
        DjVuSourceError::InputLimitExceeded {
            actual_bytes,
            max_bytes,
        } => InputSourceError::InputFileSizeLimitExceeded {
            bytes: actual_bytes,
            max_bytes,
        },
        DjVuSourceError::FrameLimitExceeded {
            actual_frames,
            max_frames,
        } => InputSourceError::ImageFrameLimitExceeded {
            frames: u32::try_from(actual_frames).unwrap_or(u32::MAX),
            max_frames,
        },
        DjVuSourceError::PixelLimitExceeded {
            page_num,
            width,
            height,
            pixels,
            max_pixels,
        } => InputSourceError::ImagePixelLimitExceeded {
            frame_number: page_num,
            width,
            height,
            pixels: u64::try_from(pixels).unwrap_or(u64::MAX),
            max_pixels: u64::try_from(max_pixels).unwrap_or(u64::MAX),
        },
        DjVuSourceError::PageOutOfRange {
            page_num,
            page_count,
        } => InputSourceError::PageOutOfRange {
            page_number: page_num,
            page_count,
        },
        _ => InputSourceError::DjvuDecodeFailed {
            path: path.to_path_buf(),
            detail,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heif_additional_frames_map_to_a_stable_warning() {
        let warning = HeifSourceWarning::AdditionalTopLevelImagesIgnored {
            total_images: 3,
            ignored_images: 2,
        };
        let mapped = match warning {
            HeifSourceWarning::AdditionalTopLevelImagesIgnored { total_images, .. } => {
                InputSourceWarning::AdditionalImageFramesIgnored {
                    format: "heif",
                    total_frames: total_images,
                    processed_frames: 1,
                }
            }
        };
        assert_eq!(
            mapped,
            InputSourceWarning::AdditionalImageFramesIgnored {
                format: "heif",
                total_frames: 3,
                processed_frames: 1,
            }
        );
    }
}
