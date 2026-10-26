//! Page source abstraction.
//!
//! `PageSource` yields one rendered page at a time so the pipeline can
//! drop each page's bitmap as soon as the next stage has consumed it.

use image::{imageops, GrayImage, Luma};

use crate::dto::Word;
use crate::page_analysis::{NativeTextLayer, NativeTextLayout, PageOrientation};

/// A single rendered page returned by a [`PageSource`].
pub struct RenderedPage {
    pub page_num: u32,
    pub image: GrayImage,
    pub dpi: f64,
}

/// A page region rendered at an explicit density.
///
/// `full_width`/`full_height` describe the virtual full-page bitmap at the
/// same density, while `x`/`y` locate this owned crop inside it. This lets the
/// table pipeline map grid coordinates from its compact thumbnail into a
/// high-detail OCR crop without allocating a full-resolution page.
pub struct RenderedPageRegion {
    pub image: GrayImage,
    pub dpi: u32,
    pub full_width: u32,
    pub full_height: u32,
    pub x: u32,
    pub y: u32,
}

impl RenderedPage {
    pub fn new(page_num: u32, image: GrayImage, dpi: f64) -> Self {
        Self {
            page_num,
            image,
            dpi,
        }
    }
}

/// Streams PDF pages (or pre-rendered bitmaps) one at a time.
pub trait PageSource {
    fn total_pages(&self) -> Result<u32, Box<dyn std::error::Error>>;

    fn render_page(&mut self, page_num: u32) -> Result<RenderedPage, Box<dyn std::error::Error>>;

    /// Renders a page while respecting the pipeline's per-page pixel budget.
    ///
    /// Native PDF sources can override this to choose a bounded render target
    /// before allocating a bitmap. The fallback remains safe for custom/test
    /// sources by resizing an already-rendered image when necessary.
    fn render_page_with_max_pixels(
        &mut self,
        page_num: u32,
        max_pixels: usize,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        let RenderedPage {
            page_num,
            image,
            dpi,
        } = self.render_page(page_num)?;
        let (target_width, target_height) =
            dimensions_within_pixel_limit(image.width(), image.height(), max_pixels)?;
        if (target_width, target_height) == image.dimensions() {
            return Ok(RenderedPage::new(page_num, image, dpi));
        }

        let scale = target_width as f64 / image.width() as f64;
        let resized = imageops::resize(
            &image,
            target_width,
            target_height,
            imageops::FilterType::Triangle,
        );
        Ok(RenderedPage::new(page_num, resized, dpi * scale))
    }

    /// Renders a page at an explicit density. Native PDF sources override this
    /// so low-resolution grid routing does not allocate a full OCR bitmap.
    fn render_page_at_dpi(
        &mut self,
        page_num: u32,
        target_dpi: u32,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        if target_dpi == 0 {
            return Err("render DPI must be greater than zero".into());
        }
        let RenderedPage {
            page_num,
            image,
            dpi,
        } = self.render_page(page_num)?;
        let scale = f64::from(target_dpi) / dpi;
        if !scale.is_finite() || scale <= 0.0 {
            return Err(
                format!("invalid render scale {scale} from {dpi} to {target_dpi} DPI").into(),
            );
        }
        let width = (f64::from(image.width()) * scale).round().max(1.0) as u32;
        let height = (f64::from(image.height()) * scale).round().max(1.0) as u32;
        let image = if (width, height) == image.dimensions() {
            image
        } else {
            imageops::resize(&image, width, height, imageops::FilterType::Triangle)
        };
        Ok(RenderedPage::new(page_num, image, f64::from(target_dpi)))
    }

    /// Renders at an explicit density while enforcing the pixel limit on the
    /// returned bitmap. PDF sources override this method so the bounded
    /// dimensions are chosen before PDFium allocates the bitmap.
    fn render_page_at_dpi_with_max_pixels(
        &mut self,
        page_num: u32,
        target_dpi: u32,
        max_pixels: usize,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        let rendered = self.render_page_at_dpi(page_num, target_dpi)?;
        let (target_width, target_height) = dimensions_within_pixel_limit(
            rendered.image.width(),
            rendered.image.height(),
            max_pixels,
        )?;
        if (target_width, target_height) == rendered.image.dimensions() {
            return Ok(rendered);
        }
        let scale = f64::from(target_width) / f64::from(rendered.image.width());
        Ok(RenderedPage::new(
            rendered.page_num,
            imageops::resize(
                &rendered.image,
                target_width,
                target_height,
                imageops::FilterType::Triangle,
            ),
            rendered.dpi * scale,
        ))
    }

    /// Optionally renders only a normalized top-left page region.
    ///
    /// PDF sources override this with a true clipped render. Other sources
    /// return `None`; their existing full-page OCR path remains unchanged.
    #[expect(
        clippy::too_many_arguments,
        reason = "Preserve the accepted OCR baseline during source-only publication."
    )]
    fn render_region_at_dpi(
        &mut self,
        _page_num: u32,
        _target_dpi: u32,
        _x: f32,
        _y: f32,
        _width: f32,
        _height: f32,
        _max_pixels: usize,
    ) -> Result<Option<RenderedPageRegion>, Box<dyn std::error::Error>> {
        Ok(None)
    }

    /// Returns a native PDF text layer when this source can provide one.
    ///
    /// The default keeps custom and test sources source-compatible while
    /// allowing production PDF sources to opt into automatic NativeText/OCR
    /// selection.
    fn native_text_for_page(
        &self,
        _page_num: u32,
    ) -> Result<Option<NativeTextLayer>, Box<dyn std::error::Error>> {
        Ok(None)
    }

    /// Returns geometry-only native-text evidence, when the source can expose
    /// it without rendering the page. The default keeps existing custom page
    /// sources source-compatible.
    fn native_text_layout_for_page(
        &self,
        _page_num: u32,
    ) -> Result<Option<NativeTextLayout>, Box<dyn std::error::Error>> {
        Ok(None)
    }

    /// Returns the intrinsic clockwise page rotation recorded by the source.
    ///
    /// Raster-only/custom sources default to no metadata rotation. PDF sources
    /// override this with the page's `/Rotate` value so searchable pages never
    /// need OCR-based orientation detection.
    fn page_orientation_from_metadata(
        &self,
        _page_num: u32,
    ) -> Result<PageOrientation, Box<dyn std::error::Error>> {
        Ok(PageOrientation::default())
    }

    /// Returns native text words mapped into a previously rendered bitmap.
    /// Sources without coordinate-bearing native text return `None`.
    fn native_words_for_rendered_page(
        &self,
        _page_num: u32,
        _width: u32,
        _height: u32,
    ) -> Result<Option<Vec<Word>>, Box<dyn std::error::Error>> {
        Ok(None)
    }

    /// Renders a bounded grayscale image for the ruled-grid routing preflight.
    ///
    /// Production PDF sources may override this to render directly at the
    /// requested scale. The default keeps test/custom sources compatible by
    /// downscaling their normal render while preserving aspect ratio.
    fn render_table_thumbnail(
        &mut self,
        page_num: u32,
        max_dimension: u32,
    ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        if max_dimension == 0 {
            return Err("table-thumbnail max dimension must be greater than zero".into());
        }

        let RenderedPage {
            page_num,
            image,
            dpi,
        } = self.render_page(page_num)?;
        let longest_side = image.width().max(image.height());
        let image = if longest_side <= max_dimension {
            image
        } else {
            let scale = max_dimension as f64 / longest_side as f64;
            let width = (image.width() as f64 * scale).round().max(1.0) as u32;
            let height = (image.height() as f64 * scale).round().max(1.0) as u32;
            imageops::resize(&image, width, height, imageops::FilterType::Triangle)
        };

        Ok(RenderedPage::new(page_num, image, dpi))
    }
}

/// Returns aspect-preserving dimensions that do not exceed `max_pixels`.
pub(crate) fn dimensions_within_pixel_limit(
    width: u32,
    height: u32,
    max_pixels: usize,
) -> Result<(u32, u32), Box<dyn std::error::Error>> {
    if width == 0 || height == 0 {
        return Err("cannot bound a zero-sized page image".into());
    }
    if max_pixels == 0 {
        return Err("page pixel limit must be greater than zero".into());
    }

    let pixels = usize::try_from(width)?
        .checked_mul(usize::try_from(height)?)
        .ok_or("page pixel count overflow")?;
    if pixels <= max_pixels {
        return Ok((width, height));
    }

    let scale = (max_pixels as f64 / pixels as f64).sqrt();
    let target_width = (width as f64 * scale).floor().max(1.0) as u32;
    let target_height = (height as f64 * scale).floor().max(1.0) as u32;
    Ok((target_width, target_height))
}

/// In-memory page source useful for tests and stub builds.
pub struct StubPageSource {
    pages: Vec<GrayImage>,
    dpi: f64,
}

impl StubPageSource {
    pub fn new(pages: Vec<GrayImage>) -> Self {
        Self { pages, dpi: 200.0 }
    }

    pub fn with_dpi(mut self, dpi: f64) -> Self {
        self.dpi = dpi;
        self
    }

    /// Create a stub source with a single solid-color page.
    pub fn single(width: u32, height: u32, value: u8) -> Self {
        let img = GrayImage::from_pixel(width, height, Luma([value]));
        Self::new(vec![img])
    }
}

impl PageSource for StubPageSource {
    fn total_pages(&self) -> Result<u32, Box<dyn std::error::Error>> {
        Ok(self.pages.len() as u32)
    }

    fn render_page(&mut self, page_num: u32) -> Result<RenderedPage, Box<dyn std::error::Error>> {
        if page_num == 0 {
            return Err("page numbers are 1-indexed".into());
        }
        let idx = (page_num - 1) as usize;
        let image = self
            .pages
            .get(idx)
            .ok_or_else(|| format!("page {page_num} out of range"))?
            .clone();
        Ok(RenderedPage::new(page_num, image, self.dpi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_returns_correct_total() {
        let src = StubPageSource::single(10, 10, 255);
        assert_eq!(src.total_pages().unwrap(), 1);
    }

    #[test]
    fn stub_renders_requested_page() {
        let mut src = StubPageSource::single(8, 6, 0);
        let page = src.render_page(1).unwrap();
        assert_eq!(page.page_num, 1);
        assert_eq!(page.image.width(), 8);
        assert_eq!(page.image.height(), 6);
    }

    #[test]
    fn stub_errors_on_zero_page() {
        let mut src = StubPageSource::single(4, 4, 0);
        assert!(src.render_page(0).is_err());
    }

    #[test]
    fn stub_errors_on_out_of_range() {
        let mut src = StubPageSource::single(4, 4, 0);
        assert!(src.render_page(7).is_err());
    }

    #[test]
    fn bounded_dimensions_preserve_aspect_and_pixel_limit() {
        let (width, height) = super::dimensions_within_pixel_limit(8_000, 8_000, 50_000_000)
            .expect("dimensions should be bounded");
        assert!(usize::try_from(width).unwrap() * usize::try_from(height).unwrap() <= 50_000_000);
        assert_eq!(width, height);
        assert!(width < 8_000);
    }

    #[test]
    fn explicit_dpi_render_is_bounded_and_reports_effective_density() {
        let mut source = StubPageSource::single(100, 100, 255).with_dpi(100.0);
        let rendered = source
            .render_page_at_dpi_with_max_pixels(1, 200, 10_000)
            .expect("bounded explicit-DPI render");
        assert_eq!(rendered.image.dimensions(), (100, 100));
        assert!((rendered.dpi - 100.0).abs() < f64::EPSILON);
    }
}
