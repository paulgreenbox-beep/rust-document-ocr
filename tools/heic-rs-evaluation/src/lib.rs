//! Isolated tests of a bounded research derivative and the published baseline.
//! These tests do not approve a production OCR adapter or binary release.

#[cfg(test)]
mod tests {
    use heic_rs::boxes::BoxIter;
    use heic_rs::context::Context;
    use heic_rs::{DecodeOptions, Error, PixelLayout};

    const FLAT: &[u8] = include_bytes!("../fixtures/flat-64.heic");
    // These reviewed fixtures include cropped coded pictures. This explicit
    // test-only RAW aggregate cap is distinct from the asserted display size.
    const FIXTURE_RAW_BUDGET: u64 = 4 * 1024 * 1024;

    fn reference_png(name: &str) -> &'static [u8] {
        match name {
            "flat-white-16" => include_bytes!("../fixtures/flat-white-16.ref.png"),
            "flat-64" => include_bytes!("../fixtures/flat-64.ref.png"),
            "checker-64" => include_bytes!("../fixtures/checker-64.ref.png"),
            "rgb-strips-96" => include_bytes!("../fixtures/rgb-strips-96.ref.png"),
            "gradient-512" => include_bytes!("../fixtures/gradient-512.ref.png"),
            "checker-1024" => include_bytes!("../fixtures/checker-1024.ref.png"),
            _ => panic!("not in the reviewed synthetic corpus"),
        }
    }

    fn difference(left: &[u8], right: &[u8]) -> (f64, u8) {
        assert_eq!(left.len(), right.len(), "compare equal RGB buffers only");
        assert!(!left.is_empty());
        let mut sum = 0_u64;
        let mut max = 0_u8;
        for (&a, &b) in left.iter().zip(right) {
            let delta = a.abs_diff(b);
            sum += u64::from(delta);
            max = max.max(delta);
        }
        (sum as f64 / left.len() as f64, max)
    }

    fn inspect(name: &str, bytes: &[u8], width: u32, height: u32, grid: bool) {
        let info = heic_rs::probe(bytes).expect("reviewed synthetic fixture must probe");
        assert_eq!((info.width, info.height), (width, height), "{name}");
        assert_eq!(info.bit_depth, 8, "{name}: this corpus is 8-bit only");
        assert_eq!(info.is_grid, grid, "{name}");
        assert!(!info.has_alpha, "{name}: this corpus has no alpha");
        let options = DecodeOptions::default()
            .with_layout(PixelLayout::Rgb8)
            .with_max_pixels(Some(FIXTURE_RAW_BUDGET))
            .with_strict(true)
            .with_threads(Some(1));
        let image = heic_rs::decode(bytes, &options).expect("reviewed fixture must decode");
        assert_eq!((image.width, image.height), (width, height), "{name}");
        assert_eq!(image.layout, PixelLayout::Rgb8, "{name}");
        assert_eq!(image.row_bytes(), width as usize * 3, "{name}");
        assert_eq!(
            image.data.len(),
            width as usize * height as usize * 3,
            "{name}"
        );
        let min = *image.data.iter().min().expect("nonempty pixels");
        let max = *image.data.iter().max().expect("nonempty pixels");
        let reference =
            image::load_from_memory_with_format(reference_png(name), image::ImageFormat::Png)
                .expect("verified platform-reference PNG")
                .to_rgb8();
        assert_eq!(reference.dimensions(), (width, height));
        let (mae, max_difference) = difference(&image.data, reference.as_raw());
        println!(
            "METRICS {{\"fixture\":\"{name}\",\"width\":{width},\"height\":{height},\"candidate_min\":{min},\"candidate_max\":{max},\"reference_min\":{},\"reference_max\":{},\"mean_absolute_error\":{mae:.9},\"max_absolute_difference\":{max_difference}}}",
            reference.as_raw().iter().min().unwrap(),
            reference.as_raw().iter().max().unwrap()
        );
        if name.contains("checker") || name.contains("gradient") {
            assert!(
                min <= 8 && max >= 247,
                "{name}: insufficient expected contrast {min}..{max}"
            );
        }
        if name == "flat-white-16" {
            assert_eq!((min, max), (255, 255), "known white pixels");
        }
        if name == "flat-64" {
            let center = ((height as usize / 2) * width as usize + width as usize / 2) * 3;
            for (actual, expected) in image.data[center..center + 3].iter().zip([200_u8, 30, 60]) {
                assert!(actual.abs_diff(expected) <= 8, "known synthetic RGB centre");
            }
        }
        println!(
            "FIXTURE {name}: PASS {width}x{height} grid={grid} RGB8 bytes={} min={min} max={max}",
            image.data.len()
        );
    }

    #[test]
    fn flat_white_16_decodes() {
        inspect(
            "flat-white-16",
            include_bytes!("../fixtures/flat-white-16.heic"),
            16,
            16,
            false,
        );
    }

    #[test]
    fn flat_64_decodes() {
        inspect("flat-64", FLAT, 64, 64, false);
    }

    #[test]
    fn checker_64_decodes() {
        inspect(
            "checker-64",
            include_bytes!("../fixtures/checker-64.heic"),
            64,
            64,
            false,
        );
    }

    #[test]
    fn rgb_strips_96_decodes() {
        inspect(
            "rgb-strips-96",
            include_bytes!("../fixtures/rgb-strips-96.heic"),
            96,
            32,
            false,
        );
    }

    #[test]
    fn gradient_512_decodes() {
        inspect(
            "gradient-512",
            include_bytes!("../fixtures/gradient-512.heic"),
            512,
            512,
            false,
        );
    }

    #[test]
    fn checker_1024_grid_decodes() {
        inspect(
            "checker-1024",
            include_bytes!("../fixtures/checker-1024.heic"),
            1024,
            1024,
            true,
        );
    }

    // Use the dependency's own public, bounds-checked box traversal. No HEVC parser fork,
    // encoded stream rewrite, unsafe code, huge dimensions, or private image data.
    fn small_ispe_mutation() -> Vec<u8> {
        let meta = BoxIter::new(FLAT)
            .require(b"meta", "meta")
            .expect("fixture meta");
        let (_, _, meta_body) = meta.full_box("meta").expect("meta FullBox");
        let iprp = BoxIter::new(meta_body.rest())
            .require(b"iprp", "iprp")
            .expect("fixture iprp");
        let ipco = BoxIter::new(iprp.payload)
            .require(b"ipco", "ipco")
            .expect("fixture ipco");
        let entries: Vec<_> = BoxIter::new(ipco.payload)
            .map(|entry| entry.expect("reviewed property"))
            .filter(|entry| entry.is(b"ispe"))
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "mutate only the reviewed single-image fixture"
        );
        let property = entries[0];
        assert_eq!(property.payload.len(), 12);
        assert_eq!(&property.payload[4..8], &64_u32.to_be_bytes());
        assert_eq!(&property.payload[8..12], &64_u32.to_be_bytes());
        let offset = property.payload.as_ptr() as usize - FLAT.as_ptr() as usize;
        let mut bytes = FLAT.to_vec();
        bytes[offset + 4..offset + 8].copy_from_slice(&16_u32.to_be_bytes());
        bytes[offset + 8..offset + 12].copy_from_slice(&16_u32.to_be_bytes());
        bytes
    }

    #[test]
    fn small_ispe_is_rejected_by_raw_picture_limit() {
        let bytes = small_ispe_mutation();
        let info = heic_rs::probe(&bytes).expect("published probe accepts modified ispe");
        assert_eq!((info.coded_width, info.coded_height), (16, 16));
        let context = Context::open(&bytes).expect("published container parser");
        let props = context
            .props(context.meta.primary)
            .expect("primary properties");
        let hvcc = props.hvcc.as_ref().expect("HEVC configuration");
        let actual = heic_rs::hevc::probe(&hvcc.parameter_sets()).expect("SPS probe");
        assert_eq!((actual.width, actual.height), (64, 64));
        let raw = heic_rs::hevc::probe_coded(&hvcc.parameter_sets()).expect("raw SPS probe");
        let required = raw.max_raw_pixels;
        assert!(required >= 4096);
        for strict in [false, true] {
            let options = DecodeOptions::default()
                .with_max_pixels(Some(256))
                .with_strict(strict)
                .with_threads(Some(1));
            let error =
                heic_rs::decode(&bytes, &options).expect_err("raw SPS exceeds the tiny cap");
            assert!(
                matches!(error, Error::ResourceLimit {
                    stage: heic_rs::ResourceStage::CodedPicture,
                    required: actual_required,
                    limit: 256
                } if actual_required == required),
                "unexpected early rejection: {error:?}"
            );
            println!("BOUNDED RESOURCE: strict={strict} {error:?}");
        }
    }

    #[test]
    #[cfg(feature = "upstream-reference")]
    fn unpatched_small_ispe_baseline_still_reaches_late_pixel_check() {
        let options = heic_rs_upstream::DecodeOptions::default()
            .with_max_pixels(Some(256))
            .with_strict(true)
            .with_threads(Some(1));
        let error = heic_rs_upstream::decode(&small_ispe_mutation(), &options).unwrap_err();
        assert!(matches!(
            error,
            heic_rs_upstream::Error::PixelLimit {
                pixels: 4096,
                max_pixels: 256
            }
        ));
        println!("UNMODIFIED 0.1.4 BASELINE: {error:?}; old observation retained");
    }

    #[test]
    #[cfg(feature = "upstream-reference")]
    fn bounded_patch_does_not_change_six_original_fixture_pixels() {
        for (name, bytes) in [
            (
                "white",
                include_bytes!("../fixtures/flat-white-16.heic").as_slice(),
            ),
            ("flat", FLAT),
            (
                "checker",
                include_bytes!("../fixtures/checker-64.heic").as_slice(),
            ),
            (
                "strips",
                include_bytes!("../fixtures/rgb-strips-96.heic").as_slice(),
            ),
            (
                "gradient",
                include_bytes!("../fixtures/gradient-512.heic").as_slice(),
            ),
            (
                "grid",
                include_bytes!("../fixtures/checker-1024.heic").as_slice(),
            ),
        ] {
            let bounded = heic_rs::decode(
                bytes,
                &DecodeOptions::default()
                    .with_layout(PixelLayout::Rgb8)
                    .with_max_pixels(Some(FIXTURE_RAW_BUDGET))
                    .with_strict(true)
                    .with_threads(Some(1)),
            )
            .unwrap();
            let original = heic_rs_upstream::decode(
                bytes,
                &heic_rs_upstream::DecodeOptions::default()
                    .with_layout(heic_rs_upstream::PixelLayout::Rgb8)
                    .with_max_pixels(Some(FIXTURE_RAW_BUDGET))
                    .with_strict(true)
                    .with_threads(Some(1)),
            )
            .unwrap();
            assert_eq!(
                (bounded.width, bounded.height),
                (original.width, original.height),
                "{name}"
            );
            assert_eq!(
                bounded.data, original.data,
                "{name}: patch must not change accepted pixels"
            );
        }
    }

    #[test]
    fn ordinary_metadata_limit_rejects_before_decode() {
        let options = DecodeOptions::default().with_max_pixels(Some(256));
        let error = heic_rs::decode(FLAT, &options).expect_err("64x64 metadata exceeds tiny cap");
        assert!(matches!(
            error,
            Error::PixelLimit {
                pixels: 4096,
                max_pixels: 256
            }
        ));
    }

    #[test]
    fn avif_brand_is_explicitly_unsupported() {
        let mut bytes = FLAT.to_vec();
        assert_eq!(&bytes[4..8], b"ftyp");
        bytes[8..12].copy_from_slice(b"avif");
        let error = heic_rs::probe(&bytes).expect_err("AVIF is not an HEVC still image");
        assert!(matches!(error, Error::Unsupported(message) if message.contains("AVIF")));
    }

    #[cfg(feature = "native-reference")]
    type NativeRgbObservation = (String, (u32, u32), (u32, u32), Vec<u8>);

    #[cfg(feature = "native-reference")]
    fn native_rgb(bytes: &[u8]) -> Result<NativeRgbObservation, String> {
        use libheif_rs::{
            ColorSpace, DecodingOptions, HeifContext, LibHeif, RgbChroma, StreamReader,
        };
        let libheif = LibHeif::new();
        let mut context = HeifContext::new().map_err(|error| format!("context: {error:?}"))?;
        context.set_max_decoding_threads(1);
        context
            .read_reader(Box::new(StreamReader::new(
                std::io::Cursor::new(bytes.to_vec()),
                bytes.len() as u64,
            )))
            .map_err(|error| format!("read: {error:?}"))?;
        assert_eq!(
            context.image_ids().len(),
            1,
            "one top-level image, not tile count"
        );
        let handle = context
            .primary_image_handle()
            .map_err(|error| format!("primary: {error:?}"))?;
        let handle_size = (handle.width(), handle.height());
        let mut options = DecodingOptions::new().ok_or("native options allocation failed")?;
        options.set_ignore_transformations(false);
        options.set_convert_hdr_to_8bit(true);
        options.set_strict_decoding(true);
        let decoded = libheif
            .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), Some(options))
            .map_err(|error| format!("decode: {error:?}"))?;
        let size = (decoded.width(), decoded.height());
        let plane = decoded
            .planes()
            .interleaved
            .expect("native interleaved RGBA");
        assert!(plane.stride >= size.0 as usize * 4);
        let mut rgb = Vec::with_capacity(size.0 as usize * size.1 as usize * 3);
        for row in 0..size.1 as usize {
            for pixel in plane.data[row * plane.stride..row * plane.stride + size.0 as usize * 4]
                .chunks_exact(4)
            {
                assert_eq!(pixel[3], 255, "the reviewed corpus has no alpha");
                rgb.extend_from_slice(&pixel[..3]);
            }
        }
        let version = libheif.version();
        Ok((
            format!("{}.{}.{}", version[0], version[1], version[2]),
            handle_size,
            size,
            rgb,
        ))
    }

    #[test]
    #[cfg(feature = "native-reference")]
    fn native_reference_comparison() {
        let fixtures: [(&str, &[u8]); 6] = [
            (
                "flat-white-16",
                include_bytes!("../fixtures/flat-white-16.heic"),
            ),
            ("flat-64", FLAT),
            ("checker-64", include_bytes!("../fixtures/checker-64.heic")),
            (
                "rgb-strips-96",
                include_bytes!("../fixtures/rgb-strips-96.heic"),
            ),
            (
                "gradient-512",
                include_bytes!("../fixtures/gradient-512.heic"),
            ),
            (
                "checker-1024",
                include_bytes!("../fixtures/checker-1024.heic"),
            ),
        ];
        let mut successful = 0;
        let mut unsuccessful = 0;
        for (name, bytes) in fixtures {
            let (version, handle_size, size, native) = match native_rgb(bytes) {
                Ok(result) => result,
                Err(error) => {
                    unsuccessful += 1;
                    println!(
                        "NATIVE_OUTCOME {{\"fixture\":\"{name}\",\"status\":\"DECODE_ERROR\"}}"
                    );
                    println!("NATIVE_ERROR fixture={name}: {error}");
                    continue;
                }
            };
            successful += 1;
            let pure = heic_rs::decode(
                bytes,
                &DecodeOptions::default()
                    .with_strict(true)
                    .with_threads(Some(1)),
            )
            .expect("pure comparison decode");
            assert_eq!(size, (pure.width, pure.height));
            let reference =
                image::load_from_memory_with_format(reference_png(name), image::ImageFormat::Png)
                    .expect("reference PNG")
                    .to_rgb8();
            assert_eq!(reference.dimensions(), size);
            let (native_reference_mae, native_reference_max) =
                difference(&native, reference.as_raw());
            let (pure_native_mae, pure_native_max) = difference(&pure.data, &native);
            println!(
                "NATIVE_METRICS {{\"fixture\":\"{name}\",\"libheif\":\"{version}\",\"handle_width\":{},\"handle_height\":{},\"decoded_width\":{},\"decoded_height\":{},\"native_min\":{},\"native_max\":{},\"native_reference_mae\":{native_reference_mae:.9},\"native_reference_max_difference\":{native_reference_max},\"pure_native_mae\":{pure_native_mae:.9},\"pure_native_max_difference\":{pure_native_max}}}",
                handle_size.0,
                handle_size.1,
                size.0,
                size.1,
                native.iter().min().unwrap(),
                native.iter().max().unwrap()
            );
        }
        println!(
            "NATIVE_DECODE_SUMMARY successful={successful} unsuccessful={unsuccessful}; passing the observation test does not approve failed decodes"
        );
    }

    #[test]
    #[cfg(feature = "native-reference")]
    fn native_small_ispe_observation() {
        use libheif_rs::{
            ColorSpace, DecodingOptions, HeifContext, LibHeif, RgbChroma, StreamReader,
        };
        let bytes = small_ispe_mutation();
        let libheif = LibHeif::new();
        let mut context = HeifContext::new().expect("native context");
        context.set_max_decoding_threads(1);
        if let Err(error) = context.read_reader(Box::new(StreamReader::new(
            std::io::Cursor::new(bytes.clone()),
            bytes.len() as u64,
        ))) {
            println!("NATIVE_RESOURCE: container rejected forged small ispe: {error:?}");
            return;
        }
        let handle = match context.primary_image_handle() {
            Ok(handle) => handle,
            Err(error) => {
                println!("NATIVE_RESOURCE: primary rejected forged small ispe: {error:?}");
                return;
            }
        };
        let pixels = u64::from(handle.width()) * u64::from(handle.height());
        println!(
            "NATIVE_RESOURCE: libheif={:?} handle={}x{} max_pixels=256 precheck_allows={}",
            libheif.version(),
            handle.width(),
            handle.height(),
            pixels <= 256
        );
        if pixels > 256 {
            return;
        }
        let mut options = DecodingOptions::new().expect("native options");
        options.set_ignore_transformations(false);
        options.set_convert_hdr_to_8bit(true);
        options.set_strict_decoding(true);
        match libheif.decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), Some(options)) {
            Ok(decoded) => println!(
                "NATIVE_RESOURCE: decoded={}x{} postcheck_allows={}; peak internal allocation NOT_MEASURED",
                decoded.width(),
                decoded.height(),
                u64::from(decoded.width()) * u64::from(decoded.height()) <= 256
            ),
            Err(error) => println!("NATIVE_RESOURCE: decode rejected: {error:?}"),
        }
    }
}
