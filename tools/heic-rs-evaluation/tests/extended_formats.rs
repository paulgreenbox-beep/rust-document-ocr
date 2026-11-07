//! End-to-end frozen synthetic HEIF inputs, not mocked decoded Frames.
//! These tests qualify only their named cases, not a general accuracy rate.

use heic_rs::context::Context;
use heic_rs::{DecodeOptions, Image, PixelLayout};

const TEN: &[u8] = include_bytes!("../fixtures/extended/gray-main10-64.heic");
const ALPHA: &[u8] = include_bytes!("../fixtures/extended/flat-alpha128-64.heic");
const FLAT: &[u8] = include_bytes!("../fixtures/flat-64.heic");
const STRIPS: &[u8] = include_bytes!("../fixtures/rgb-strips-96.heic");
const GRADIENT: &[u8] = include_bytes!("../fixtures/gradient-512.heic");
const ROTATED: [(u8, &[u8]); 3] = [
    (1, include_bytes!("../fixtures/extended/strips-irot90.heic")),
    (
        2,
        include_bytes!("../fixtures/extended/strips-irot180.heic"),
    ),
    (
        3,
        include_bytes!("../fixtures/extended/strips-irot270.heic"),
    ),
];
const MIRRORED: [(u8, &[u8]); 2] = [
    (
        0,
        include_bytes!("../fixtures/extended/gradient-imir0.heic"),
    ),
    (
        1,
        include_bytes!("../fixtures/extended/gradient-imir1.heic"),
    ),
];

fn options(layout: PixelLayout, raw_pixel_budget: u64) -> DecodeOptions {
    DecodeOptions::default()
        .with_layout(layout)
        .with_max_pixels(Some(raw_pixel_budget))
        .with_strict(true)
        .with_threads(Some(1))
}

fn coded_budget(bytes: &[u8], include_alpha: bool) -> u64 {
    let context = Context::open(bytes).unwrap();
    let item_cost = |id| {
        let items = context
            .grid(id)
            .unwrap()
            .map_or_else(|| vec![id], |(_, tiles)| tiles);
        items
            .into_iter()
            .map(|item| {
                let props = context.props(item).unwrap();
                let coded =
                    heic_rs::hevc::probe_coded(&props.hvcc.unwrap().parameter_sets()).unwrap();
                coded.max_raw_pixels
            })
            .sum::<u64>()
    };
    let primary = item_cost(context.meta.primary);
    let alpha = if include_alpha {
        context
            .alpha_item(context.meta.primary)
            .unwrap()
            .map_or(0, item_cost)
    } else {
        0
    };
    let total = primary.checked_add(alpha).unwrap();
    assert!(
        total <= 4_194_304,
        "only the admitted tiny frozen corpus may derive its raw test budget"
    );
    total
}

fn decode(bytes: &[u8], layout: PixelLayout) -> Image {
    heic_rs::decode(
        bytes,
        &options(layout, coded_budget(bytes, layout.has_alpha())),
    )
    .unwrap_or_else(|error| panic!("extended fixture UNSUPPORTED_OR_FAILED: {error:?}"))
}

fn raw_frame(bytes: &[u8], item: u32) -> heic_rs::hevc::Frame {
    let context = Context::open(bytes).unwrap();
    let props = context.props(item).unwrap();
    let config = props.hvcc.as_ref().unwrap();
    let data = context.item_data(item).unwrap();
    let slices = config.split_nals(&data).unwrap();
    heic_rs::hevc::decode_still(&config.parameter_sets(), &slices).unwrap()
}

/// Independent destination-to-source coordinate mapping, deliberately not
/// calling the dependency's transform implementation or image helpers.
fn transform(
    data: &[u8],
    width: u32,
    height: u32,
    channels: usize,
    turns: u8,
    mirror: Option<u8>,
) -> (u32, u32, Vec<u8>) {
    let (out_width, out_height) = if turns % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    };
    let mut out = vec![0; data.len()];
    for y in 0..out_height {
        for x in 0..out_width {
            let (mut sx, mut sy) = match turns {
                0 => (x, y),
                1 => (width - 1 - y, x),
                2 => (width - 1 - x, height - 1 - y),
                3 => (y, height - 1 - x),
                _ => unreachable!(),
            };
            // ISO/IEC23008-12:2022 6.5.12 as documented by libavif v1.3:
            // imir0 exchanges top/bottom; imir1 exchanges left/right.
            // Do not inherit the reversed axis labels of heic-rs0.1.4.
            if mirror == Some(1) {
                sx = width - 1 - sx;
            }
            if mirror == Some(0) {
                sy = height - 1 - sy;
            }
            let source = (sy as usize * width as usize + sx as usize) * channels;
            let dest = (y as usize * out_width as usize + x as usize) * channels;
            out[dest..dest + channels].copy_from_slice(&data[source..source + channels]);
        }
    }
    (out_width, out_height, out)
}

#[test]
fn genuine_main10_hevc_decodes_to_ten_bit_samples_and_rgb16() {
    let info = heic_rs::probe(TEN).unwrap();
    assert_eq!((info.width, info.height, info.bit_depth), (64, 64, 10));
    let context = Context::open(TEN).unwrap();
    let props = context.props(context.meta.primary).unwrap();
    let config = props.hvcc.as_ref().unwrap();
    assert_eq!((config.general_profile_idc, config.bit_depth_luma), (2, 10));
    let frame = raw_frame(TEN, context.meta.primary);
    assert_eq!((frame.width, frame.height, frame.bit_depth), (64, 64, 10));
    assert!(
        frame
            .y
            .iter()
            .chain(&frame.cb)
            .chain(&frame.cr)
            .all(|&sample| sample == 512)
    );
    assert_eq!(coded_budget(TEN, false), 4096);
    let wide = decode(TEN, PixelLayout::Rgb16);
    assert_eq!(wide.data.len(), 64 * 64 * 3 * 2);
    // Neutral Main10 samples Y=Cb=Cr=512, default limited range. Compute
    // expected 16-bit gray independently from the limited-range rational.
    let expected = ((512u64 - 64) * 65_535 + 876 / 2) / 876;
    for sample in wide.data.chunks_exact(2) {
        assert!(u16::from_ne_bytes([sample[0], sample[1]]).abs_diff(expected as u16) <= 1);
    }
    let narrow = decode(TEN, PixelLayout::Rgb8);
    assert!(narrow.data.iter().all(|&sample| sample == 130));
    println!(
        "EXTENDED Main10 PASS: actual HEVC frame10bit all512; RGB16 gray~{expected}; RGB8gray130"
    );
}

#[test]
fn frozen_libheif_1_23_2_pngs_confirm_main10_and_alpha_pixels() {
    let gray = image::load_from_memory(include_bytes!(
        "../fixtures/extended/gray-main10-64.libheif-1.23.2.png"
    ))
    .unwrap();
    assert_eq!(
        gray.color(),
        image::ColorType::Rgb16,
        "independent decoder retained wide samples"
    );
    let gray = gray.to_rgb16();
    assert_eq!(gray.dimensions(), (64, 64));
    // One 10-bit source quantization step becomes at most 64/65535 in PNG16.
    assert!(
        gray.as_raw()
            .iter()
            .all(|&value| value.abs_diff(33516) <= 64)
    );
    let candidate = decode(TEN, PixelLayout::Rgb16);
    let candidate_samples = candidate
        .data
        .chunks_exact(2)
        .map(|value| u16::from_ne_bytes([value[0], value[1]]))
        .collect::<Vec<_>>();
    assert_eq!(candidate_samples.len(), gray.as_raw().len());
    let (sum16, max16) = candidate_samples.iter().zip(gray.as_raw()).fold(
        (0u64, 0u16),
        |(sum, max), (&left, &right)| {
            let delta = left.abs_diff(right);
            (sum + u64::from(delta), max.max(delta))
        },
    );
    assert!(
        max16 <= 64,
        "full-array error exceeds one original 10-bit sample step"
    );
    let alpha = image::load_from_memory(include_bytes!(
        "../fixtures/extended/flat-alpha128-64.libheif-1.23.2.png"
    ))
    .unwrap();
    assert_eq!(alpha.color(), image::ColorType::Rgba8);
    let alpha = alpha.to_rgba8();
    assert_eq!(alpha.dimensions(), (64, 64));
    assert!(alpha.pixels().all(|pixel| pixel[3] == 128));
    let candidate_alpha = decode(ALPHA, PixelLayout::Rgba8);
    assert_eq!(
        (candidate_alpha.width, candidate_alpha.height),
        alpha.dimensions()
    );
    assert_eq!(candidate_alpha.data.len(), alpha.as_raw().len());
    for (candidate, reference) in candidate_alpha
        .data
        .chunks_exact(4)
        .zip(alpha.as_raw().chunks_exact(4))
    {
        assert_eq!(candidate[3], reference[3], "independent alpha sample");
    }
    let (sum_alpha, max_alpha) = candidate_alpha.data.iter().zip(alpha.as_raw()).fold(
        (0u64, 0u8),
        |(sum, max), (&left, &right)| {
            let delta = left.abs_diff(right);
            (sum + u64::from(delta), max.max(delta))
        },
    );
    // A narrow regression bound for this frozen flat-colour fixture, not a
    // general decoder equivalence threshold. The actual delta is still logged.
    assert!(
        max_alpha <= 2,
        "frozen alpha fixture RGB/RGBA delta changed"
    );
    println!(
        "EXTENDED independentCLI libheif1.23.2 PNG16 gray={}..{}; PNG8 alpha128; both64x64",
        gray.as_raw().iter().min().unwrap(),
        gray.as_raw().iter().max().unwrap()
    );
    println!(
        "CLI_REFERENCE_AB libheif1.23.2 RGB16samples={} candidate={}..{} native={}..{} MAE={:.9} max={max16}; alphaRGBA8_MAE={:.9} alphaRGBA8_max={max_alpha}; alphaexact128",
        candidate_samples.len(),
        candidate_samples.iter().min().unwrap(),
        candidate_samples.iter().max().unwrap(),
        gray.as_raw().iter().min().unwrap(),
        gray.as_raw().iter().max().unwrap(),
        sum16 as f64 / candidate_samples.len() as f64,
        sum_alpha as f64 / candidate_alpha.data.len() as f64
    );
}

#[test]
fn auxiliary_hevc_alpha_is_nonopaque_and_can_be_disabled() {
    let info = heic_rs::probe(ALPHA).unwrap();
    assert!(info.has_alpha);
    let context = Context::open(ALPHA).unwrap();
    let auxiliary = context
        .alpha_item(context.meta.primary)
        .unwrap()
        .expect("real auxl alpha reference");
    assert_ne!(auxiliary, context.meta.primary);
    let frame = raw_frame(ALPHA, auxiliary);
    assert_eq!(frame.chroma, heic_rs::hevc::ChromaFormat::Monochrome);
    assert_eq!((frame.width, frame.height, frame.bit_depth), (64, 64, 8));
    assert!(frame.y.iter().all(|&sample| sample == 128));
    // Explicit aggregate budget includes BOTH the primary and alpha raw images.
    let aggregate = coded_budget(ALPHA, true);
    assert_eq!(aggregate, coded_budget(FLAT, false) + 4096);
    let rgba = decode(ALPHA, PixelLayout::Rgba8);
    let base = decode(FLAT, PixelLayout::Rgb8);
    assert_eq!((rgba.width, rgba.height), (64, 64));
    for (actual, original) in rgba.data.chunks_exact(4).zip(base.data.chunks_exact(3)) {
        assert_eq!(&actual[..3], original);
        assert_eq!(actual[3], 128);
    }
    let opaque = heic_rs::decode(
        ALPHA,
        &options(PixelLayout::Rgba8, aggregate).with_alpha(false),
    )
    .unwrap();
    assert!(opaque.data.chunks_exact(4).all(|pixel| pixel[3] == 255));
    assert_eq!(decode(ALPHA, PixelLayout::Rgb8).data, base.data);
    println!(
        "EXTENDED alpha PASS: real auxiliary HEVC mono128; alphaon128 alphaoff255; RGB preserved; raw_aggregate={aggregate}"
    );
}

#[test]
fn real_irot_quarter_turns_match_independent_pixel_transforms() {
    let raw_budget = coded_budget(STRIPS, false);
    let base = decode(STRIPS, PixelLayout::Rgb8);
    assert_eq!((base.width, base.height), (96, 32));
    for (turns, bytes) in ROTATED {
        let context = Context::open(bytes).unwrap();
        assert_eq!(
            context
                .props(context.meta.primary)
                .unwrap()
                .rotation()
                .degrees(),
            u16::from(turns) * 90
        );
        let image = decode(bytes, PixelLayout::Rgb8);
        let (width, height, expected) = transform(&base.data, 96, 32, 3, turns, None);
        assert_eq!((image.width, image.height), (width, height));
        assert!(
            image.data == expected,
            "irot{turns}: byte-exact independent mapping mismatch"
        );
        assert_ne!(
            image.data, base.data,
            "nonzero rotation must alter this asymmetric fixture"
        );
        let ignored = heic_rs::decode(
            bytes,
            &options(PixelLayout::Rgb8, raw_budget).with_transforms(false),
        )
        .unwrap();
        assert_eq!(
            (ignored.width, ignored.height, ignored.data),
            (96, 32, base.data.clone())
        );
        println!(
            "EXTENDED irot{} PASS byte-exact independent mapping {width}x{height}; raw_budget={raw_budget}",
            u16::from(turns) * 90
        );
    }
}

#[test]
fn real_imir_both_axes_match_independent_pixel_transforms() {
    let budget = coded_budget(GRADIENT, false);
    let base = decode(GRADIENT, PixelLayout::Rgb8);
    for (axis, bytes) in MIRRORED {
        let context = Context::open(bytes).unwrap();
        assert!(
            context
                .props(context.meta.primary)
                .unwrap()
                .mirror()
                .is_some()
        );
        let image = decode(bytes, PixelLayout::Rgb8);
        let (_, _, expected) = transform(&base.data, 512, 512, 3, 0, Some(axis));
        assert!(
            image.data == expected,
            "imir{axis}: byte-exact independent mapping mismatch"
        );
        assert_ne!(
            image.data, base.data,
            "mirror must not be an unobservable symmetry"
        );
        let ignored = heic_rs::decode(
            bytes,
            &options(PixelLayout::Rgb8, budget).with_transforms(false),
        )
        .unwrap();
        assert_eq!(ignored.data, base.data);
        println!("EXTENDED imir{axis} PASS byte-exact independent mapping");
    }
}

#[cfg(feature = "upstream-reference")]
#[test]
fn published_upstream_mirror_axis_defect_is_distinguished_from_the_fix() {
    let old_options = heic_rs_upstream::DecodeOptions::default()
        .with_max_pixels(Some(coded_budget(GRADIENT, false)))
        .with_strict(true)
        .with_threads(Some(1));
    let base = heic_rs_upstream::decode(GRADIENT, &old_options).unwrap();
    for (axis, bytes) in MIRRORED {
        let upstream = heic_rs_upstream::decode(bytes, &old_options).unwrap();
        let (_, _, correct) = transform(&base.data, base.width, base.height, 3, 0, Some(axis));
        let (_, _, wrong_axis) =
            transform(&base.data, base.width, base.height, 3, 0, Some(1 - axis));
        assert!(
            upstream.data != correct,
            "pinned upstream0.1.4 defect fingerprint changed"
        );
        assert!(
            upstream.data == wrong_axis,
            "known upstream defect must be precisely the swapped mirror axis"
        );
        assert!(
            decode(bytes, PixelLayout::Rgb8).data == correct,
            "vendored fix must match the independent HEIF mapping"
        );
        println!(
            "UPSTREAM_KNOWN_DEFECT imir{axis}: published0.1.4 swapped axis reproduced; vendored correct mapping PASS"
        );
    }
}

#[cfg(feature = "native-reference")]
#[path = "common/native_reference.rs"]
mod native_reference;

#[cfg(feature = "native-reference")]
#[test]
fn independent_libheif_validates_main10_alpha_and_transforms() {
    let gray = native_reference::decode_rgba(TEN).expect("native independent Main10 decode");
    assert_eq!((gray.width, gray.height, gray.bit_depth), (64, 64, 10));
    assert!(
        gray.data
            .chunks_exact(4)
            .all(
                |pixel| pixel[..3].iter().all(|&value| value.abs_diff(130) <= 1) && pixel[3] == 255
            )
    );
    let alpha =
        native_reference::decode_rgba(ALPHA).expect("native independent auxiliary alpha decode");
    assert!(alpha.has_alpha);
    assert!(alpha.data.chunks_exact(4).all(|pixel| pixel[3] == 128));
    let base = native_reference::decode_rgba(STRIPS).expect("native original strips");
    for (turns, bytes) in ROTATED {
        let actual = native_reference::decode_rgba(bytes).expect("native rotated strips");
        let (width, height, expected) =
            transform(&base.data, base.width, base.height, 4, turns, None);
        assert_eq!((actual.width, actual.height), (width, height));
        assert!(
            actual.data == expected,
            "native rotation must agree with independent mapping"
        );
        native_reference::compare(
            &format!("irot{}", u16::from(turns) * 90),
            &decode(bytes, PixelLayout::Rgba8).data,
            &actual,
        );
    }
    let base = native_reference::decode_rgba(GRADIENT).expect("native original gradient");
    for (axis, bytes) in MIRRORED {
        let actual = native_reference::decode_rgba(bytes).expect("native mirrored gradient");
        let (_, _, expected) = transform(&base.data, base.width, base.height, 4, 0, Some(axis));
        assert!(
            actual.data == expected,
            "native mirror must agree with independent mapping"
        );
        native_reference::compare(
            &format!("imir{axis}"),
            &decode(bytes, PixelLayout::Rgba8).data,
            &actual,
        );
    }
    native_reference::compare("main10", &decode(TEN, PixelLayout::Rgba8).data, &gray);
    native_reference::compare("alpha128", &decode(ALPHA, PixelLayout::Rgba8).data, &alpha);
}
