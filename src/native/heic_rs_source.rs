//! Opt-in, pure-Rust primary-image adapter for the bounded HEIC research path.
//!
//! This is not a fallback: a failed `heic-rs` decode never invokes libheif.
//! The shared limit also bounds aggregate untrimmed HEVC luma sites, which can
//! exceed the displayed area (for example, with cropping, grids or alpha).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use heic_rs::context::Context;
use heic_rs::meta::iref::RefKind;
use heic_rs::{DecodeOptions, PixelLayout};
use image::GrayImage;

use super::heif_source::{HeifSourceError, HeifSourceLimits, HeifSourceWarning};
use crate::page_source::{PageSource, RenderedPage};

/// One cached, transformed primary image; nominal DPI does not resample it.
pub struct HeicRsPageSource {
    path: PathBuf,
    primary_image: GrayImage,
    dpi: f64,
    top_level_image_count: usize,
    warnings: Vec<HeifSourceWarning>,
}

impl HeicRsPageSource {
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
        // Do not trust metadata alone: a concurrently growing input must not
        // make an unbounded read. One sentinel byte detects that race.
        let bytes = read_bounded(&mut file, &path, limits.max_file_bytes)?;
        // The container brand parser is the signature probe, including an
        // explicit rejection of both major and compatible AVIF/AVIS brands.
        heic_rs::ftyp::parse(&bytes).map_err(|error| decode_error("signature", error))?;
        let context =
            Context::open(&bytes).map_err(|error| decode_error("read_container", error))?;
        let top_level_image_count = top_level_image_count(&context);
        if top_level_image_count > limits.max_frames {
            return Err(HeifSourceError::ImageFrameLimitExceeded {
                actual_frames: top_level_image_count,
                limit_frames: limits.max_frames,
            });
        }
        context
            .meta
            .primary_item()
            .map_err(|error| decode_error("primary_image", error))?;
        validate_alpha_semantics(&context)?;
        let info =
            heic_rs::probe(&bytes).map_err(|error| decode_error("probe_primary_image", error))?;
        enforce_pixel_limit(info.coded_width, info.coded_height, limits.max_pixels)?;
        enforce_pixel_limit(info.width, info.height, limits.max_pixels)?;

        // Always bounded, strict and serial, regardless of the shared native
        // thread option. Cargo enables only heic-rs's `std`, not `parallel`.
        let options = DecodeOptions::default()
            .with_layout(PixelLayout::Rgba8)
            .with_max_pixels(Some(limits.max_pixels))
            .with_transforms(true)
            .with_alpha(true)
            .with_strict(true)
            .with_threads(Some(1));
        let decoded = heic_rs::decode(&bytes, &options)
            .map_err(|error| decode_error("decode_primary_image", error))?;
        enforce_pixel_limit(decoded.width, decoded.height, limits.max_pixels)?;
        if decoded.layout != PixelLayout::Rgba8 {
            return Err(decode_failure(
                "read_rgba_plane",
                "decoder did not return RGBA8",
            ));
        }
        let stride = usize::try_from(decoded.width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .ok_or_else(|| decode_failure("read_rgba_plane", "RGBA stride overflows"))?;
        // heic-rs promises tightly packed output; check that promise before
        // indexing. Premultiplied input was rejected above, since the pinned
        // decoder neither interprets `prem` nor reports an alpha association.
        let expected_len = usize::try_from(decoded.height)
            .ok()
            .and_then(|height| stride.checked_mul(height))
            .ok_or_else(|| decode_failure("read_rgba_plane", "RGBA byte count overflows"))?;
        if decoded.data.len() != expected_len {
            return Err(decode_failure(
                "read_rgba_plane",
                format!(
                    "RGBA buffer has {} bytes; packed stride and height require {expected_len}",
                    decoded.data.len()
                ),
            ));
        }
        let primary_image =
            rgba_rows_to_gray(&decoded.data, decoded.width, decoded.height, stride)?;
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
}

impl PageSource for HeicRsPageSource {
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
    let message = if !dpi.is_finite() || dpi <= 0.0 {
        Some(format!(
            "DPI must be finite and greater than zero, got {dpi}"
        ))
    } else if limits.max_file_bytes == 0 {
        Some("max_file_bytes must be greater than zero".to_string())
    } else if limits.max_pixels == 0 {
        Some("max_pixels must be greater than zero".to_string())
    } else if limits.max_frames == 0 {
        Some("max_frames must be greater than zero".to_string())
    } else {
        None
    };
    match message {
        Some(message) => Err(HeifSourceError::InvalidConfiguration { message }),
        None => Ok(()),
    }
}

fn read_bounded(
    reader: &mut impl Read,
    path: &Path,
    limit: u64,
) -> Result<Vec<u8>, HeifSourceError> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| HeifSourceError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let actual_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual_bytes > limit {
        return Err(HeifSourceError::ImageFileLimitExceeded {
            path: path.to_path_buf(),
            actual_bytes,
            limit_bytes: limit,
        });
    }
    Ok(bytes)
}

fn top_level_image_count(context: &Context<'_>) -> usize {
    context
        .meta
        .items
        .iter()
        .filter(|item| {
            let image = item.is_hevc()
                || matches!(
                    &item.item_type,
                    b"grid" | b"iovl" | b"iden" | b"av01" | b"jpeg" | b"j2k1" | b"unci"
                );
            image
                && !item.hidden
                && !context.meta.refs.iter().any(|reference| {
                    (matches!(reference.kind, RefKind::Thmb | RefKind::Auxl)
                        && reference.from == item.id)
                        || (reference.kind == RefKind::Dimg && reference.to.contains(&item.id))
                })
        })
        .count()
}

fn validate_alpha_semantics(context: &Context<'_>) -> Result<(), HeifSourceError> {
    // Be conservative about either direction of a primary-linked `prem`
    // relation: this research adapter must not silently multiply alpha twice.
    if context.meta.refs.iter().any(|reference| {
        reference.kind == RefKind::Other(*b"prem")
            && (reference.from == context.meta.primary
                || reference.to.contains(&context.meta.primary))
    }) {
        return Err(decode_failure(
            "unsupported_premultiplied_alpha",
            "the primary image has a premultiplied-alpha (`prem`) reference; the pinned heic-rs decoder does not implement its semantics",
        ));
    }
    Ok(())
}

fn enforce_pixel_limit(width: u32, height: u32, limit_pixels: u64) -> Result<(), HeifSourceError> {
    if width == 0 || height == 0 {
        return Err(decode_failure(
            "image_dimensions",
            format!("invalid dimensions {width}x{height}"),
        ));
    }
    let actual_pixels = u64::from(width) * u64::from(height);
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

fn rgba_rows_to_gray(
    rgba: &[u8],
    width: u32,
    height: u32,
    stride: usize,
) -> Result<GrayImage, HeifSourceError> {
    enforce_pixel_limit(width, height, u64::MAX)?;
    let width = usize::try_from(width)
        .map_err(|error| decode_failure("convert_rgba", error.to_string()))?;
    let height_usize = usize::try_from(height)
        .map_err(|error| decode_failure("convert_rgba", error.to_string()))?;
    let row_bytes = width
        .checked_mul(4)
        .ok_or_else(|| decode_failure("convert_rgba", "RGBA row size overflows"))?;
    let required = stride
        .checked_mul(height_usize)
        .ok_or_else(|| decode_failure("convert_rgba", "RGBA buffer size overflows"))?;
    if stride < row_bytes || rgba.len() < required {
        return Err(decode_failure(
            "convert_rgba",
            "RGBA stride or buffer is shorter than the image",
        ));
    }
    let count = width
        .checked_mul(height_usize)
        .ok_or_else(|| decode_failure("convert_rgba", "grayscale size overflows"))?;
    let mut gray = Vec::with_capacity(count);
    for row in 0..height_usize {
        for pixel in rgba[row * stride..row * stride + row_bytes].chunks_exact(4) {
            let alpha = u32::from(pixel[3]);
            let composite =
                |channel: u8| (u32::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255;
            let red = composite(pixel[0]);
            let green = composite(pixel[1]);
            let blue = composite(pixel[2]);
            // Identical integer white-compositing and luma weights to the
            // existing libheif source; no library-specific grayscale shortcut.
            gray.push(((77 * red + 150 * green + 29 * blue + 128) >> 8) as u8);
        }
    }
    GrayImage::from_raw(width as u32, height, gray)
        .ok_or_else(|| decode_failure("convert_rgba", "cannot construct grayscale image"))
}

fn decode_failure(stage: &'static str, message: impl Into<String>) -> HeifSourceError {
    HeifSourceError::HeifDecodeFailed {
        stage,
        message: message.into(),
    }
}

fn decode_error(stage: &'static str, error: heic_rs::Error) -> HeifSourceError {
    let stage = match error {
        // Do not mislabel aggregate raw sample sites as displayed width*height.
        heic_rs::Error::ResourceLimit { .. } => "hevc_resource_limit",
        heic_rs::Error::PixelLimit { .. } => "decoder_canvas_pixel_limit",
        _ => stage,
    };
    decode_failure(stage, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use heic_rs::boxes::BoxIter;
    use heic_rs::meta::iref::ItemReference;
    use std::io::{Cursor, Write};

    const FLAT: &[u8] = include_bytes!("../../tools/heic-rs-evaluation/fixtures/flat-64.heic");
    const ALPHA: &[u8] =
        include_bytes!("../../tools/heic-rs-evaluation/fixtures/extended/flat-alpha128-64.heic");
    const TEN: &[u8] =
        include_bytes!("../../tools/heic-rs-evaluation/fixtures/extended/gray-main10-64.heic");
    const STRIPS: &[u8] =
        include_bytes!("../../tools/heic-rs-evaluation/fixtures/rgb-strips-96.heic");
    const ROTATED: &[u8] =
        include_bytes!("../../tools/heic-rs-evaluation/fixtures/extended/strips-irot90.heic");
    const GRID: &[u8] = include_bytes!("../../tools/heic-rs-evaluation/fixtures/checker-1024.heic");

    fn fixture(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("temporary synthetic input");
        file.write_all(bytes).unwrap();
        file.flush().unwrap();
        file
    }

    fn open(bytes: &[u8]) -> HeicRsPageSource {
        let file = fixture(bytes);
        HeicRsPageSource::open(file.path(), 300.0).unwrap()
    }

    fn failure(bytes: &[u8], limits: HeifSourceLimits) -> HeifSourceError {
        let file = fixture(bytes);
        HeicRsPageSource::open_with_limits(file.path(), 300.0, limits)
            .err()
            .expect("reject input")
    }

    #[test]
    fn primary_page_is_cached_and_dpi_is_nominal() {
        let file = fixture(FLAT);
        let mut source = HeicRsPageSource::open(file.path(), 123.0).unwrap();
        assert_eq!(source.source_path(), file.path());
        assert_eq!(source.total_pages().unwrap(), 1);
        assert_eq!(source.top_level_image_count(), 1);
        assert!(source.warnings().is_empty());
        file.as_file().set_len(0).unwrap();
        let mut first = source.render_page(1).unwrap();
        assert_eq!(first.image.dimensions(), (64, 64));
        assert_eq!((first.page_num, first.dpi), (1, 123.0));
        let original = first.image[(0, 0)];
        first.image[(0, 0)] = image::Luma([0]);
        assert_eq!(source.render_page(1).unwrap().image[(0, 0)], original);
        for page_num in [0, 2, u32::MAX] {
            let error = source.render_page(page_num).err().unwrap();
            assert!(
                matches!(error.downcast_ref::<HeifSourceError>(), Some(HeifSourceError::InvalidPageIndex { page_num: actual }) if *actual == page_num)
            );
        }
    }

    #[test]
    fn real_main10_decodes_to_eight_bit_ocr_gray() {
        assert_eq!(heic_rs::probe(TEN).unwrap().bit_depth, 10);
        let page = open(TEN).render_page(1).unwrap();
        assert_eq!(page.image.dimensions(), (64, 64));
        assert!(page.image.as_raw().iter().all(|value| *value == 130));
    }

    #[test]
    fn real_auxiliary_alpha_is_composited_on_white_not_counted_as_a_page() {
        assert!(heic_rs::probe(ALPHA).unwrap().has_alpha);
        let mut source = open(ALPHA);
        assert_eq!(source.top_level_image_count(), 1);
        assert!(source.warnings().is_empty());
        let page = source.render_page(1).unwrap();
        let opaque = open(FLAT).render_page(1).unwrap();
        assert_eq!(page.image.dimensions(), (64, 64));
        assert!(page.image[(32, 32)][0] > opaque.image[(32, 32)][0]);
        let decoded = heic_rs::decode(
            ALPHA,
            &DecodeOptions::default()
                .with_layout(PixelLayout::Rgba8)
                .with_threads(Some(1)),
        )
        .unwrap();
        for (pixel, gray) in decoded.data.chunks_exact(4).zip(page.image.as_raw()) {
            assert_eq!(pixel[3], 128, "real nonopaque alpha payload");
            let composite = |channel: u8| (u32::from(channel) * 128 + 255 * 127 + 127) / 255;
            assert_eq!(
                *gray,
                ((77 * composite(pixel[0])
                    + 150 * composite(pixel[1])
                    + 29 * composite(pixel[2])
                    + 128)
                    >> 8) as u8
            );
        }
    }

    #[test]
    fn premultiplied_primary_alpha_is_explicitly_rejected() {
        let mut bytes = ALPHA.to_vec();
        let link = bytes.windows(4).position(|kind| kind == b"auxl").unwrap();
        bytes[link..link + 4].copy_from_slice(b"prem");
        let error = failure(&bytes, Default::default());
        assert!(matches!(error, HeifSourceError::HeifDecodeFailed {
            stage: "unsupported_premultiplied_alpha", message,
        } if message.contains("premultiplied-alpha")));
        let mut context = Context::open(ALPHA).unwrap();
        let alpha = context.alpha_item(context.meta.primary).unwrap().unwrap();
        context.meta.refs.push(ItemReference {
            kind: RefKind::Other(*b"prem"),
            from: context.meta.primary,
            to: vec![alpha],
        });
        assert!(validate_alpha_semantics(&context).is_err());
    }

    #[test]
    fn nonsquare_primary_rotation_is_applied_exactly_once() {
        let base = open(STRIPS).render_page(1).unwrap().image;
        let rotated = open(ROTATED).render_page(1).unwrap().image;
        assert_eq!(base.dimensions(), (96, 32));
        assert_eq!(rotated.dimensions(), (32, 96));
        for y in 0..96 {
            for x in 0..32 {
                assert_eq!(rotated[(x, y)], base[(95 - y, x)]);
            }
        }
    }

    #[test]
    fn white_compositing_luma_and_stride_match_existing_integer_contract() {
        let rgba = [
            255, 0, 0, 255, 0, 0, 0, 0, 99, 98, 97, 96, 0, 255, 0, 128, 0, 0, 255, 255, 11, 12, 13,
            14,
        ];
        let image = rgba_rows_to_gray(&rgba, 2, 2, 12).unwrap();
        assert_eq!(image.as_raw(), &[77, 255, 202, 29]);
        assert!(rgba_rows_to_gray(&rgba[..8], 2, 2, 12).is_err());
        assert!(rgba_rows_to_gray(&rgba, 2, 2, 4).is_err());
        assert!(rgba_rows_to_gray(&[], 0, 1, 0).is_err());
    }

    #[test]
    fn file_and_growing_reader_limits_are_bounded() {
        assert!(matches!(
            failure(
                FLAT,
                HeifSourceLimits {
                    max_file_bytes: FLAT.len() as u64 - 1,
                    ..Default::default()
                }
            ),
            HeifSourceError::ImageFileLimitExceeded { .. }
        ));
        let mut reader = Cursor::new(vec![0; 100]);
        let error = read_bounded(&mut reader, Path::new("synthetic-growing-input"), 8).unwrap_err();
        assert_eq!(
            reader.position(),
            9,
            "read no more than limit plus sentinel"
        );
        assert!(matches!(
            error,
            HeifSourceError::ImageFileLimitExceeded {
                actual_bytes: 9,
                limit_bytes: 8,
                ..
            }
        ));
        let mut reader = Cursor::new(vec![0; 8]);
        assert_eq!(
            read_bounded(&mut reader, Path::new("exact-limit"), 8)
                .unwrap()
                .len(),
            8
        );
    }

    #[test]
    fn displayed_pixel_limit_and_raw_aggregate_are_not_conflated() {
        assert!(matches!(
            failure(
                FLAT,
                HeifSourceLimits {
                    max_pixels: 4095,
                    ..Default::default()
                }
            ),
            HeifSourceError::ImagePixelLimitExceeded {
                width: 64,
                height: 64,
                actual_pixels: 4096,
                limit_pixels: 4095
            }
        ));
        // Both displayed images fit, and each coded picture fits individually;
        // their combined raw areas (10240 + 4096) do not fit this budget.
        let error = failure(
            ALPHA,
            HeifSourceLimits {
                max_pixels: 12_000,
                ..Default::default()
            },
        );
        assert!(
            matches!(&error, HeifSourceError::HeifDecodeFailed { stage: "hevc_resource_limit", message } if message.contains("AggregateCodedPictures") && message.contains("14336") && message.contains("12000") && message.contains("untrimmed luma sites")),
            "{error:?}"
        );
        assert_eq!(error.code(), "heif_decode_failed");
    }

    #[test]
    fn forged_small_ispe_cannot_bypass_the_raw_hevc_limit() {
        let meta = BoxIter::new(FLAT).require(b"meta", "meta").unwrap();
        let (_, _, body) = meta.full_box("meta").unwrap();
        let iprp = BoxIter::new(body.rest()).require(b"iprp", "iprp").unwrap();
        let ipco = BoxIter::new(iprp.payload).require(b"ipco", "ipco").unwrap();
        let ispe = BoxIter::new(ipco.payload).require(b"ispe", "ispe").unwrap();
        assert_eq!(ispe.payload.len(), 12);
        let offset = ispe.payload.as_ptr() as usize - FLAT.as_ptr() as usize;
        let mut forged = FLAT.to_vec();
        forged[offset + 4..offset + 8].copy_from_slice(&16_u32.to_be_bytes());
        forged[offset + 8..offset + 12].copy_from_slice(&16_u32.to_be_bytes());
        assert_eq!(heic_rs::probe(&forged).unwrap().width, 16);
        let error = failure(
            &forged,
            HeifSourceLimits {
                max_pixels: 256,
                ..Default::default()
            },
        );
        assert!(
            matches!(error, HeifSourceError::HeifDecodeFailed { stage: "hevc_resource_limit", message } if message.contains("CodedPicture") && message.contains("untrimmed luma sites"))
        );
    }

    fn two_top_level_images() -> Vec<u8> {
        let mut bytes = ALPHA.to_vec();
        let entries: Vec<_> = ALPHA
            .windows(4)
            .enumerate()
            .filter_map(|(offset, kind)| (kind == b"infe").then_some(offset))
            .collect();
        assert_eq!(entries.len(), 2);
        for offset in entries {
            bytes[offset + 7] = 0;
        } // Clear hidden flag.
        let link = ALPHA.windows(4).position(|kind| kind == b"auxl").unwrap();
        bytes[link..link + 4].copy_from_slice(b"cdsc");
        bytes
    }

    #[test]
    fn additional_top_level_images_warn_and_frame_limit_precedes_decode() {
        let bytes = two_top_level_images();
        let mut source = open(&bytes);
        assert_eq!(source.top_level_image_count(), 2);
        assert_eq!(source.total_pages().unwrap(), 1);
        assert_eq!(
            source.warnings(),
            &[HeifSourceWarning::AdditionalTopLevelImagesIgnored {
                total_images: 2,
                ignored_images: 1
            }]
        );
        assert_eq!(source.render_page(1).unwrap().image.dimensions(), (64, 64));
        let mut corrupt = bytes;
        let data = corrupt.windows(4).position(|kind| kind == b"mdat").unwrap() + 4;
        corrupt[data..].fill(0);
        assert!(matches!(
            failure(
                &corrupt,
                HeifSourceLimits {
                    max_frames: 1,
                    ..Default::default()
                }
            ),
            HeifSourceError::ImageFrameLimitExceeded {
                actual_frames: 2,
                limit_frames: 1
            }
        ));
    }

    #[test]
    fn top_level_count_excludes_hidden_auxiliary_thumbnail_and_grid_tile_items() {
        let mut context = Context::open(ALPHA).unwrap();
        assert_eq!(top_level_image_count(&context), 1);
        context
            .meta
            .items
            .iter_mut()
            .for_each(|item| item.hidden = false);
        assert_eq!(
            top_level_image_count(&context),
            1,
            "auxl even without hidden flag"
        );
        let auxiliary = context
            .meta
            .refs
            .iter()
            .find(|reference| reference.kind == RefKind::Auxl)
            .unwrap()
            .from;
        context.meta.refs.clear();
        assert_eq!(top_level_image_count(&context), 2);
        context.meta.refs.push(ItemReference {
            kind: RefKind::Thmb,
            from: auxiliary,
            to: vec![context.meta.primary],
        });
        assert_eq!(top_level_image_count(&context), 1);
        context.meta.refs.clear();
        context.meta.refs.push(ItemReference {
            kind: RefKind::Dimg,
            from: context.meta.primary,
            to: vec![auxiliary],
        });
        assert_eq!(top_level_image_count(&context), 1);
        let grid = Context::open(GRID).unwrap();
        assert!(grid.meta.items.len() > 1);
        assert_eq!(top_level_image_count(&grid), 1, "real multi-tile container");
    }

    #[test]
    fn invalid_options_bad_input_and_avif_are_rejected() {
        for dpi in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                validate_configuration(dpi, Default::default()),
                Err(HeifSourceError::InvalidConfiguration { .. })
            ));
        }
        for limits in [
            HeifSourceLimits {
                max_file_bytes: 0,
                ..Default::default()
            },
            HeifSourceLimits {
                max_pixels: 0,
                ..Default::default()
            },
            HeifSourceLimits {
                max_frames: 0,
                ..Default::default()
            },
        ] {
            assert!(matches!(
                validate_configuration(300.0, limits),
                Err(HeifSourceError::InvalidConfiguration { .. })
            ));
        }
        for bytes in [&b""[..], &b"not an image"[..], &FLAT[..20]] {
            assert_eq!(
                failure(bytes, Default::default()).code(),
                "heif_decode_failed"
            );
        }
        for avif in [b"avif", b"avis"] {
            let mut bytes = FLAT.to_vec();
            assert_eq!(&bytes[4..8], b"ftyp");
            bytes[8..12].copy_from_slice(avif);
            let error = failure(&bytes, Default::default());
            assert!(
                matches!(error, HeifSourceError::HeifDecodeFailed { stage: "signature", message } if message.contains("AVIF"))
            );
        }
        let mut compatible_avif = FLAT.to_vec();
        compatible_avif[16..20].copy_from_slice(b"avif");
        assert!(matches!(
            failure(&compatible_avif, Default::default()),
            HeifSourceError::HeifDecodeFailed {
                stage: "signature",
                ..
            }
        ));
    }
}
