#[cfg(feature = "djvu-input")]
pub mod djvu_source;
#[cfg(feature = "heif-input")]
pub mod heif_source;
pub mod input_source;
pub mod pdfium_render_ffi;
pub mod pdfium_source;
#[cfg(feature = "raster-input")]
pub mod raster_source;
pub mod tesseract_engine;
#[cfg(feature = "raster-input")]
pub mod tiff_source;

#[cfg(test)]
pub(crate) fn pdfium_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}
