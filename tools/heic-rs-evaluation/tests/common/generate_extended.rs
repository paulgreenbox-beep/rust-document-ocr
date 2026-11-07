//! Standalone generator, linked once against heic-rs with its `bench` feature.
//! Upstream bitstream generator is MIT OR Apache-2.0, Thomas Braun (2026).
//! This file is not part of the decoder and is not compiled by normal tests.

mod container;

use heic_rs::hevc::synth;
use std::path::Path;

fn picture(depth: u8, chroma: u8) -> container::Item {
    let profile = if chroma == 0 {
        4
    } else if depth == 10 {
        2
    } else {
        3
    };
    let mut sps = synth::build_sps(64, 64, u32::from(chroma), u32::from(depth));
    // The upstream helper always labels Main Still Picture. Set a truthful
    // Main10/Range Extensions PTL; sample depth is genuinely coded in SPS.
    sps[1] = profile;
    sps[2..6].copy_from_slice(&(1u32 << (31 - profile)).to_be_bytes());
    // Complete mandatory VPS ordering/layer/timing fields, unlike the skimmed
    // VPS in the upstream decoder-only unit builder. Independently cross-decode.
    let mut vps = vec![0x0c, 0x01, 0xff, 0xff, profile];
    vps.extend_from_slice(&(1u32 << (31 - profile)).to_be_bytes());
    vps.extend_from_slice(&[0xd0, 0, 0, 0, 0, 0, 30, 0xf0, 0x24]);
    let sets = vec![
        synth::to_nal(32, &vps),
        synth::to_nal(33, &sps),
        synth::to_nal(34, &synth::build_pps()),
    ];
    let slice = synth::to_nal(19, &synth::build_slice(1, u32::from(chroma)));
    let mut data = (slice.len() as u32).to_be_bytes().to_vec();
    data.extend_from_slice(&slice);
    container::Item {
        width: 64,
        height: 64,
        depth,
        chroma,
        hvcc: container::hvcc(&sets, profile, depth, chroma),
        data,
        extra: vec![],
        alpha: false,
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("explicit evaluation root");
    let root = Path::new(&root);
    let directory = root.join("fixtures/extended");
    std::fs::create_dir_all(&directory).unwrap();
    let write = |name: &str, bytes: Vec<u8>| {
        let path = directory.join(name);
        if path.exists() {
            assert_eq!(
                std::fs::read(&path).unwrap(),
                bytes,
                "refuse to change existing frozen fixture: {}",
                path.display()
            );
            return;
        }
        std::fs::write(path, bytes).unwrap();
    };
    write("gray-main10-64.heic", container::file(&[picture(10, 1)]));
    let base = std::fs::read(root.join("fixtures/flat-64.heic")).unwrap();
    let mut alpha = picture(8, 0);
    alpha.alpha = true;
    write(
        "flat-alpha128-64.heic",
        container::file(&[container::Item::from_file(&base), alpha]),
    );
    let strips = std::fs::read(root.join("fixtures/rgb-strips-96.heic")).unwrap();
    for code in 1..=3 {
        write(
            &format!("strips-irot{}.heic", u16::from(code) * 90),
            container::transformed(&strips, b"irot", code),
        );
    }
    let gradient = std::fs::read(root.join("fixtures/gradient-512.heic")).unwrap();
    for code in 0..=1 {
        write(
            &format!("gradient-imir{code}.heic"),
            container::transformed(&gradient, b"imir", code),
        );
    }
    println!(
        "Generated seven genuine HEIF containers; next verify actual HEVC decode and native A/B."
    );
}
