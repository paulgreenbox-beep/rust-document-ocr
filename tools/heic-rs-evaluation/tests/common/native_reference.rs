//! Independent installed libheif decoder; no fallback to candidate pixels.

pub struct NativeImage {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub has_alpha: bool,
    pub version: [u8; 3],
    pub data: Vec<u8>,
}

pub fn decode_rgba(bytes: &[u8]) -> Result<NativeImage, String> {
    use libheif_rs::{ColorSpace, DecodingOptions, HeifContext, LibHeif, RgbChroma, StreamReader};
    let library = LibHeif::new();
    let mut context = HeifContext::new().map_err(|error| format!("context: {error:?}"))?;
    context.set_max_decoding_threads(1);
    context
        .read_reader(Box::new(StreamReader::new(
            std::io::Cursor::new(bytes.to_vec()),
            bytes.len() as u64,
        )))
        .map_err(|error| format!("read: {error:?}"))?;
    let handle = context
        .primary_image_handle()
        .map_err(|error| format!("primary: {error:?}"))?;
    let bit_depth = handle.luma_bits_per_pixel();
    let has_alpha = handle.has_alpha_channel();
    let mut options = DecodingOptions::new().ok_or("native options allocation")?;
    options.set_ignore_transformations(false);
    options.set_convert_hdr_to_8bit(true);
    options.set_strict_decoding(true);
    let image = library
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), Some(options))
        .map_err(|error| format!("decode: {error:?}"))?;
    let (width, height) = (image.width(), image.height());
    let plane = image.planes().interleaved.ok_or("no RGBA plane")?;
    let row_bytes = width as usize * 4;
    if plane.stride < row_bytes {
        return Err("native stride too short".into());
    }
    let mut data = Vec::with_capacity(row_bytes * height as usize);
    for y in 0..height as usize {
        data.extend_from_slice(&plane.data[y * plane.stride..y * plane.stride + row_bytes]);
    }
    Ok(NativeImage {
        width,
        height,
        bit_depth,
        has_alpha,
        version: library.version(),
        data,
    })
}

pub fn compare(name: &str, candidate: &[u8], native: &NativeImage) {
    assert_eq!(candidate.len(), native.data.len());
    let mut max = 0;
    let mut sum = 0u64;
    for (&left, &right) in candidate.iter().zip(&native.data) {
        let delta = left.abs_diff(right);
        max = max.max(delta);
        sum += u64::from(delta);
    }
    println!(
        "EXTENDED_NATIVE {{\"fixture\":\"{name}\",\"libheif\":\"{}.{}.{}\",\"width\":{},\"height\":{},\"mae\":{:.9},\"max_difference\":{max}}}",
        native.version[0],
        native.version[1],
        native.version[2],
        native.width,
        native.height,
        sum as f64 / candidate.len() as f64
    );
}
