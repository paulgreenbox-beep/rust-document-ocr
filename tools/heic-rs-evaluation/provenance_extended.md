# Extended synthetic HEIF research corpus

These files are research fixtures, not photographs, production runtime assets,
or a general HEIC accuracy certificate. No encoder was installed, and no model,
database, service, plugin registration or global setting was changed.

## Sources and generator

`tests/common/generate_extended.rs` and `tests/common/container.rs` are retained
source for the complete container construction. Generation uses the published
heic-rs 0.1.4 bitstream/CABAC test writer from upstream commit
`999470f514c8e9a1a7c37ea44ba70cc32b1308fe`, exposed only by the evaluation
`fixture-generation` feature. The source and license notices remain under
`vendor/heic-rs`; upstream copyright is Thomas Braun, 2026, MIT OR Apache-2.0.
The source encoder and candidate decoder share an implementation lineage;
their agreement alone is not independent decoder verification.

The Main10 file contains newly generated HEVC parameter sets and an IDR I slice,
not an old 8-bit payload relabeled by container metadata. SPS, VPS and hvcC all
identify profile 2 with matching compatibility flags; SPS codes 10-bit samples.
The generator completes mandatory VPS ordering, layer and timing fields that
the upstream decoder-only test writer otherwise skims. The resulting raw
64x64 Y/Cb/Cr planes actually decode to 10-bit sample value 512. The monochrome
alpha stream uses profile 4, 8-bit samples and a distinct coded item, with a
real auxC alpha property and auxl reference to the primary item.

The primary color payload for alpha is preserved from `fixtures/flat-64.heic`.
The non-square rotation payload is preserved from `fixtures/rgb-strips-96.heic`;
mirrors preserve `fixtures/gradient-512.heic`. Those original synthetic files
and their existing Apple-encoder provenance are documented by
`upstream-source.json`. Containers are rebuilt with genuinely nonzero irot
codes 1/2/3 or imir codes 0/1, without inheriting an irot=0 property. Original
encoded item bytes and associated colr data are retained.

Generator-relevant SHA-256:

| Source | SHA-256 |
| --- | --- |
| `vendor/heic-rs/src/hevc/synth.rs` | `1995b3c5faa15eed393af8c6e51091e07036968357821c0da611206405f96a95` |
| `vendor/heic-rs/src/hevc/cabac/encoder.rs` | `32ed9b7fd342e2779dc2348f80aa7f6511ff876cf388fc5199315eeea98f120e` |
| `tests/common/generate_extended.rs` | `8102efc965803db95b2f5fe669e8e893018767f4a2c34d9d559b6a9fea88eac0` |
| `tests/common/container.rs` | `c1ce9b5a8b6623497fa2c881aabbd328ff4b3272ecbccc6abc9bb5f312a2285d` |

To reproduce, build the isolated library with
`cargo build --offline --locked --lib --features fixture-generation`, compile
the standalone generator against that heic-rs rlib with `rustc --edition=2024`,
then run it with the explicit evaluation directory as its sole argument.
The generator checks existing fixtures for exact identity and refuses to change
them. Normal regression tests read frozen files and do not need the bench/test
writer or the fixture-generation feature.

## Frozen files

All paths below are relative to `fixtures/extended`.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `gray-main10-64.heic` | 343 | `a89e36be51e534f457fe4020c8ee5204afcb25831d01f5ca5676fe9cb2c698b9` |
| `flat-alpha128-64.heic` | 663 | `be642c699dfa1ca175b026a5eab706f70103b66152a950cd19cf5346743018c6` |
| `strips-irot90.heic` | 458 | `2227bc8c3f52cfeef217f46149353e76fe60b83cc4d5ffbbfd1dfbbe374bebfb` |
| `strips-irot180.heic` | 458 | `8d2fc0d83cd4e380cb543cf7ef795b9cd15187dd43fdcd4950b67c346672da3f` |
| `strips-irot270.heic` | 458 | `0859fbd6a287752811f3ca37205fbdefed329714be82d2511daf5fa07407f16b` |
| `gradient-imir0.heic` | 4309 | `d781c4fd167cd8ee13801753702da6dcf6d7bc0488a9193564606ec1f3d36e6e` |
| `gradient-imir1.heic` | 4309 | `a30710c5f81064aef672e996d609643541218c49162945e1be47d8c3bfc6b137` |
| `gray-main10-64.libheif-1.23.2.png` | 223 | `268297eb7548cc9bbe6ccc1a39852559e09cfc83009121eb59085e346334e7ec` |
| `flat-alpha128-64.libheif-1.23.2.png` | 225 | `340618955f7c1f1288568b7399e6479ee827a6c6addb20a7483f9f1f991ebf16` |

## Independent references and observations

Two PNG references were generated with the existing research-only heif-convert
1.23.2 / libheif 1.23.2, plugins disabled, using libde265 1.1.1. The executable
SHA-256 is `1315298d9262779d8384d334b7d713dc99cd8147e46220eeb9c19c211ca10a67`.
Its runtime manifest binds source archive SHA-256
`8bd5d41d19dc84536d118b04774709f244df6104ef66d623dad5fa4650143405` to
[libheif v1.23.2](https://github.com/strukturag/libheif/releases/tag/v1.23.2).
The exact private machine path belongs only in the local evidence receipt,
not this public provenance. Both conversion processes exited 0; actual PNG
decoding verifies 64x64 RGB16 gray samples **33536** and 64x64 RGBA8 alpha
samples **128**. The RGB16 reference differs by 20/65535 from the candidate's
limited-range rational gray 33516; this is below one original 10-bit sample
quantization step, not byte-exact color agreement.

The reference tool is not distributed or used as a production fallback.
[Upstream license notices](https://raw.githubusercontent.com/strukturag/libheif/v1.23.2/COPYING)
identify the sample applications as MIT and the library as LGPLv3. These are
distinct from the pure Rust dependency's license and from redistribution rights
for any OCR model.

The separately linked `native-reference` feature uses the existing libheif
**1.23.1** baseline, strict decode, transformations enabled, HDR-to-8-bit enabled
and one decoding thread. Its targeted validity run exited 0 and independently
decoded Main10, nonopaque alpha, all three rotations and both mirrors. Do not
attribute the 1.23.2 PNGs to this 1.23.1 run.

Test budgets derive from `probe_coded().max_raw_pixels` across all parameter
sets, charged per primary/tile occurrence plus requested alpha. They are not
derived from display area: Main10 requires 4096 raw pixel sites; original
flat/strips require 10240 despite smaller displayed images; alpha requires
10240 + 4096 = **14336**; gradient mirrors require 262144. These test budgets do
not raise any production limit.
The derivation also asserts a fixed 4,194,304 raw-site ceiling for this admitted
tiny corpus, so it is not an example of raising a limit for arbitrary input.

`tests/extended_formats.rs` uses its own destination/source pixel-index mapping,
not the candidate's transform function. Rotation dimensions and pixel order
are checked byte-exact, with transformations-disabled controls. Mirror semantics
follow the pinned [libavif v1.3.0 header](https://github.com/AOMediaCodec/libavif/blob/v1.3.0/include/avif/avif.h),
which cites HEIF ISO/IEC23008-12:2022 section6.5.12: code0 exchanges top/bottom;
code1 exchanges left/right. The native baseline agrees with this mapping.

The first corrected-expectation run **failed** the candidate imir0 check
(exit101, four other pure tests passed), reproducing the reversed axis mapping
in upstream heic-rs0.1.4. The native-only independent validity run still passed.
Before correction, candidate/native RGBA errors for either mirror were
MAE64.144147873 / max255. This is a real failure, not an unsupported fixture or
a reason to normalize fixture bytes. Final corrected-decoder verification must
be recorded separately; the frozen files above are unchanged.

Other measured pre-correction candidate/native RGBA errors were:

| Case | Mean absolute error | Maximum difference |
| --- | ---: | ---: |
| Each irot90/180/270 | 1.484375000 | 188 |
| Main10 | 0.750000000 | 1 |
| Alpha128 | 0.003906250 | 2 |

These observations do not approve the large edge differences in the strip
corpus or establish general pixel equivalence. No test substitutes candidate
pixels when the native decoder fails.

## Corrected decoder verification

After the minimal vendored imir parser correction, the same frozen inputs and
independent mapping passed:

- `cargo test --offline --locked --test extended_formats`: 5 passed, exit0.
- `cargo test --offline --locked --features native-reference,upstream-reference --test extended_formats`:
  7 passed, exit0, including independent libheif1.23.1 decoding and a separately
  linked published0.1.4 regression fingerprint.
- Scoped Clippy for the same featured integration test with `-D warnings`:
  exit0.

Both candidate mirrors now match the independent pixel mapping byte-exact.
Their candidate/libheif1.23.1 RGBA error dropped to MAE0.160366058 / max3.
The earlier MAE64.144147873 / max255 failure remains evidence above; neither
frozen fixture bytes nor comparison tolerances were changed to obtain a pass.
The optional upstream-reference test independently confirms that published
0.1.4 still mirrors on precisely the opposite axis, while the vendored decoder
uses the correct mapping for both codes.

The full 12,288-channel RGB16 array comparison to the independent libheif1.23.2
PNG produced candidate33516 versus native33536, **MAE20.000000000 / max20** in
16-bit channel units. This is not byte-exact agreement. The same1.23.2 PNG alpha
reference has exactly128 for every alpha sample; complete RGBA8 comparison
produced MAE0.003906250 / max2, with the difference confined to color. The
separate1.23.1 Main10 RGBA8 comparison remains MAE0.750000000 / max1. Reference
versions, sample depths and numerical units are intentionally kept separate.
