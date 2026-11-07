// Added 2026-11-07 for the OCR research derivative. Small synthetic allocation-boundary regressions.
//! No external fixtures, unsafe allocator, native library, huge allocation or OOM test.

use super::bits::BitWriter;
use super::synth;
use crate::{DecodeOptions, Error, PixelLayout, ResourceStage};

fn sets_owned() -> (Vec<Vec<u8>>, Vec<u8>) {
    synth::picture_fmt(64, 64, 0, 8)
}
fn refs(sets: &[Vec<u8>]) -> Vec<&[u8]> {
    sets.iter().map(Vec::as_slice).collect()
}
fn reset() {
    super::reset_picture_count();
    super::reset_image_buffer_count();
}
fn no_image_buffers() {
    assert_eq!(
        super::image_buffer_count(),
        0,
        "entered Geometry/Picture/filter allocation phase"
    );
    assert_eq!(
        super::picture_count(),
        0,
        "constructed a Picture before resource rejection"
    );
}

// Same minimal synthetic SPS syntax as synth::build_sps, with explicit id and
// conformance-window control. This is a test writer, not a production parser.
fn sps(raw_w: u32, raw_h: u32, id: u32, right: u32, bottom: u32) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.put(0, 4);
    w.put(0, 3);
    w.put(1, 1);
    w.put(0, 2);
    w.put(0, 1);
    w.put(3, 5);
    for i in 0..32 {
        w.put(u32::from(i == 3), 1);
    }
    w.put(0b1101, 4);
    w.put(0, 22);
    w.put(0, 21);
    w.put(0, 1);
    w.put(30, 8);
    w.ue(id);
    w.ue(0); // monochrome: crop offsets are luma samples
    w.ue(raw_w);
    w.ue(raw_h);
    w.put(u32::from(right != 0 || bottom != 0), 1);
    if right != 0 || bottom != 0 {
        w.ue(0);
        w.ue(right);
        w.ue(0);
        w.ue(bottom);
    }
    w.ue(0);
    w.ue(0);
    w.ue(4);
    w.put(1, 1);
    w.ue(0);
    w.ue(0);
    w.ue(0);
    w.ue(0);
    w.ue(3);
    w.ue(0);
    w.ue(3);
    w.ue(0);
    w.ue(0);
    for _ in 0..4 {
        w.put(0, 1);
    } // scaling, AMP, SAO, PCM
    w.ue(0); // short-term ref sets
    for _ in 0..5 {
        w.put(0, 1);
    } // long term, temporal MVP, smoothing, VUI, extension
    w.rbsp_trailing();
    synth::to_nal(33, &w.bytes)
}

#[test]
fn resource_limit_raw_sps_precedes_even_invalid_slice_copy_and_geometry() {
    let (sets, _) = sets_owned();
    reset();
    assert_eq!(
        super::decode_still_limited(&refs(&sets), &[&[0xff]], 256).unwrap_err(),
        Error::ResourceLimit {
            stage: ResourceStage::CodedPicture,
            required: 4096,
            limit: 256,
        }
    );
    no_image_buffers();
}

#[test]
fn resource_limit_counts_conformance_crop_raw_area_not_probe_output() {
    let (mut sets, slice) = sets_owned();
    sets[1] = sps(64, 64, 0, 48, 48);
    let info = super::probe_coded(&refs(&sets)).expect("small cropped SPS");
    assert_eq!(
        (
            info.raw_width,
            info.raw_height,
            info.width,
            info.height,
            info.max_raw_pixels
        ),
        (64, 64, 16, 16, 4096)
    );
    reset();
    assert_eq!(
        super::decode_still_limited(&refs(&sets), &[&slice], 256).unwrap_err(),
        Error::ResourceLimit {
            stage: ResourceStage::CodedPicture,
            required: 4096,
            limit: 256,
        }
    );
    no_image_buffers();
    let frame = super::decode_still_limited(&refs(&sets), &[&slice], 4096)
        .expect("explicit sufficient raw budget");
    assert_eq!((frame.width, frame.height), (16, 16));
    assert_eq!(super::image_buffer_count(), 1);
    assert_eq!(super::picture_count(), 1);
}

#[test]
fn resource_limit_accounts_for_unused_larger_sps_configuration() {
    for same_id_replaced in [false, true] {
        let (mut sets, slice) = sets_owned();
        if same_id_replaced {
            sets.insert(1, sps(128, 64, 0, 0, 0));
        } else {
            sets.push(sps(128, 64, 1, 0, 0));
        }
        assert_eq!(
            super::probe_coded(&refs(&sets)).unwrap().max_raw_pixels,
            8192
        );
        reset();
        assert_eq!(
            super::decode_still_limited(&refs(&sets), &[&slice], 4096).unwrap_err(),
            Error::ResourceLimit {
                stage: ResourceStage::CodedPicture,
                required: 8192,
                limit: 4096,
            }
        );
        no_image_buffers();
    }
}

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}
fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut out = vec![version];
    out.extend_from_slice(&flags.to_be_bytes()[1..]);
    out.extend_from_slice(body);
    bx(kind, &out)
}
fn hvcc(sets: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![1, 3];
    out.extend_from_slice(&[0; 10]);
    out.push(30);
    out.extend_from_slice(&0xf000_u16.to_be_bytes());
    out.extend_from_slice(&[0xfc, 0xfc, 0xf8, 0xf8, 0, 0, 0x0b]);
    out.push(sets.len() as u8);
    for set in sets {
        out.push(0x80 | ((set[0] >> 1) & 0x3f));
        out.extend_from_slice(&1_u16.to_be_bytes());
        out.extend_from_slice(&(set.len() as u16).to_be_bytes());
        out.extend_from_slice(set);
    }
    bx(b"hvcC", &out)
}

#[derive(Clone, Copy)]
enum Request {
    Single,
    Grid,
    RepeatedGrid,
    Alpha,
    AlphaGrid,
}

// Compact generated HEIF with method-1 idat locations; every coded picture is
// an independently valid 64x64 monochrome intra frame and no file I/O occurs.
fn file(request: Request, ispe: u32, grid_output: u16, rotation: bool) -> Vec<u8> {
    let (sets, slice) = sets_owned();
    let coded = {
        let mut x = (slice.len() as u32).to_be_bytes().to_vec();
        x.extend_from_slice(&slice);
        x
    };
    let grid = {
        let mut x = vec![0, 0, 0, 1];
        x.extend_from_slice(&grid_output.to_be_bytes());
        x.extend_from_slice(&grid_output.to_be_bytes());
        x
    };
    let entries: Vec<(u16, &[u8; 4], Vec<u8>)> = match request {
        Request::Single => vec![(1, b"hvc1", coded.clone())],
        Request::Grid => vec![
            (1, b"grid", grid.clone()),
            (2, b"hvc1", coded.clone()),
            (3, b"hvc1", coded.clone()),
        ],
        Request::RepeatedGrid => vec![(1, b"grid", grid.clone()), (2, b"hvc1", coded.clone())],
        Request::Alpha => vec![(1, b"hvc1", coded.clone()), (2, b"hvc1", coded.clone())],
        Request::AlphaGrid => vec![
            (1, b"hvc1", coded.clone()),
            (2, b"grid", grid.clone()),
            (3, b"hvc1", coded.clone()),
            (4, b"hvc1", coded.clone()),
        ],
    };
    let mut catalogue = (entries.len() as u16).to_be_bytes().to_vec();
    let mut locations = vec![0x44, 0];
    locations.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    let mut idat = Vec::new();
    let mut properties = Vec::new();
    let mut associations = (entries.len() as u32).to_be_bytes().to_vec();
    let mut index = 0_u8;
    for (id, kind, payload) in &entries {
        let mut infe = id.to_be_bytes().to_vec();
        infe.extend_from_slice(&0_u16.to_be_bytes());
        infe.extend_from_slice(*kind);
        infe.push(0);
        catalogue.extend_from_slice(&full(b"infe", 2, u32::from(*id != 1), &infe));
        locations.extend_from_slice(&id.to_be_bytes());
        locations.extend_from_slice(&1_u16.to_be_bytes());
        locations.extend_from_slice(&0_u16.to_be_bytes());
        locations.extend_from_slice(&1_u16.to_be_bytes());
        locations.extend_from_slice(&(idat.len() as u32).to_be_bytes());
        locations.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        idat.extend_from_slice(payload);
        let mut record = Vec::new();
        let mut size = ispe.to_be_bytes().to_vec();
        size.extend_from_slice(&ispe.to_be_bytes());
        properties.extend_from_slice(&full(b"ispe", 0, 0, &size));
        index += 1;
        record.push(index | 0x80);
        if *kind == b"hvc1" {
            properties.extend_from_slice(&hvcc(&sets));
            index += 1;
            record.push(index | 0x80);
        }
        if *id == 2 && matches!(request, Request::Alpha | Request::AlphaGrid) {
            properties.extend_from_slice(&full(b"auxC", 0, 0, b"urn:mpeg:hevc:2015:auxid:1\0"));
            index += 1;
            record.push(index | 0x80);
        }
        if *id == 1 && rotation {
            properties.extend_from_slice(&bx(b"irot", &[1]));
            index += 1;
            record.push(index);
        }
        associations.extend_from_slice(&id.to_be_bytes());
        associations.push(record.len() as u8);
        associations.extend_from_slice(&record);
    }
    let mut references = Vec::new();
    fn reference(out: &mut Vec<u8>, kind: &[u8; 4], from: u16, to: &[u16]) {
        let mut x = from.to_be_bytes().to_vec();
        x.extend_from_slice(&(to.len() as u16).to_be_bytes());
        for id in to {
            x.extend_from_slice(&id.to_be_bytes());
        }
        out.extend_from_slice(&bx(kind, &x));
    }
    match request {
        Request::Grid => reference(&mut references, b"dimg", 1, &[2, 3]),
        Request::RepeatedGrid => reference(&mut references, b"dimg", 1, &[2, 2]),
        Request::Alpha => reference(&mut references, b"auxl", 2, &[1]),
        Request::AlphaGrid => {
            reference(&mut references, b"auxl", 2, &[1]);
            reference(&mut references, b"dimg", 2, &[3, 4]);
        }
        Request::Single => {}
    }
    let mut meta = full(b"hdlr", 0, 0, &[0, 0, 0, 0, b'p', b'i', b'c', b't']);
    meta.extend_from_slice(&full(b"pitm", 0, 0, &1_u16.to_be_bytes()));
    meta.extend_from_slice(&full(b"iinf", 0, 0, &catalogue));
    meta.extend_from_slice(&full(b"iloc", 1, 0, &locations));
    if !references.is_empty() {
        meta.extend_from_slice(&full(b"iref", 0, 0, &references));
    }
    let mut iprp = bx(b"ipco", &properties);
    iprp.extend_from_slice(&full(b"ipma", 0, 0, &associations));
    meta.extend_from_slice(&bx(b"iprp", &iprp));
    meta.extend_from_slice(&bx(b"idat", &idat));
    let mut out = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
    out.extend_from_slice(&full(b"meta", 0, 0, &meta));
    out
}

#[test]
fn resource_limit_container_mismatch_has_no_strict_or_parallel_bypass() {
    let bytes = file(Request::Single, 16, 16, true);
    for strict in [false, true] {
        for threads in [Some(1), Some(2), None] {
            reset();
            let options = DecodeOptions::default()
                .with_max_pixels(Some(256))
                .with_strict(strict)
                .with_threads(threads);
            assert_eq!(
                crate::decode(&bytes, &options),
                Err(Error::ResourceLimit {
                    stage: ResourceStage::CodedPicture,
                    required: 4096,
                    limit: 256
                })
            );
            no_image_buffers();
        }
    }
}

#[test]
fn resource_limit_aggregate_grid_and_repeated_tile_reject_before_first_picture() {
    for request in [Request::Grid, Request::RepeatedGrid] {
        reset();
        let bytes = file(request, 64, 64, false);
        let options = DecodeOptions::default()
            .with_max_pixels(Some(4096))
            .with_threads(Some(1));
        assert_eq!(
            crate::decode(&bytes, &options),
            Err(Error::ResourceLimit {
                stage: ResourceStage::AggregateCodedPictures,
                required: 8192,
                limit: 4096
            })
        );
        no_image_buffers();
    }
}

#[test]
fn resource_limit_main_plus_alpha_and_alpha_grid_share_budget() {
    for (request, required) in [(Request::Alpha, 8192), (Request::AlphaGrid, 12288)] {
        reset();
        let bytes = file(request, 64, 64, false);
        let options = DecodeOptions::default()
            .with_layout(PixelLayout::Rgba8)
            .with_max_pixels(Some(4096))
            .with_threads(Some(1));
        // Both variants fail after at least two raw pictures have been charged;
        // alpha-grid's full requirement is encountered at its second tile.
        let error = crate::decode(&bytes, &options).expect_err("aggregate exceeds cap");
        assert!(matches!(
            error,
            Error::ResourceLimit {
                stage: ResourceStage::AggregateCodedPictures,
                required: 8192,
                limit: 4096
            }
        ));
        no_image_buffers();
        let accepted = options.with_max_pixels(Some(required));
        let image = crate::decode(&bytes, &accepted).expect("explicit sufficient aggregate budget");
        assert_eq!((image.width, image.height), (64, 64));
        assert_eq!(super::picture_count(), (required / 4096) as usize);
    }
}

#[test]
fn resource_limit_grid_output_guard_precedes_image_work() {
    let bytes = file(Request::Grid, 16, 128, false);
    reset();
    let error = crate::decode(
        &bytes,
        &DecodeOptions::default().with_max_pixels(Some(8192)),
    )
    .expect_err("forged small ispe cannot hide grid output");
    assert_eq!(
        error,
        Error::PixelLimit {
            pixels: 16384,
            max_pixels: 8192
        }
    );
    no_image_buffers();
}

#[test]
fn resource_limit_legacy_default_and_explicit_none_scope() {
    let (sets, slice) = sets_owned();
    reset();
    assert_eq!(
        super::decode_still(&refs(&sets), &[&slice]).unwrap().width,
        64
    );
    assert_eq!(super::image_buffer_count(), 1);
    reset();
    let bytes = file(Request::Alpha, 64, 64, false);
    let options = DecodeOptions::default()
        .with_layout(PixelLayout::Rgba8)
        .with_max_pixels(None)
        .with_threads(Some(1));
    assert_eq!(crate::decode(&bytes, &options).unwrap().width, 64);
    assert_eq!(
        super::picture_count(),
        2,
        "explicit None is an intentional documented opt-out"
    );
}

#[test]
#[cfg(feature = "parallel")]
fn resource_limit_parallel_grid_with_exact_aggregate_budget_decodes() {
    let bytes = file(Request::Grid, 64, 64, false);
    let options = DecodeOptions::default()
        .with_max_pixels(Some(8192))
        .with_threads(Some(2));
    let image = crate::decode(&bytes, &options)
        .expect("whole request admitted before parallel tile dispatch");
    assert_eq!((image.width, image.height), (64, 64));
}
