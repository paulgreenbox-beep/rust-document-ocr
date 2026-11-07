# Isolated heic-rs evaluation

Disposition: **HOLD: PREALLOCATION GATE UNQUALIFIED** for the current
pipeline contract. This is an experimental decoder observation package, not a
production adapter, RC2 binary, OCR acceptance run, or redistribution approval.
The six small approved synthetic fixtures decode, but the required preallocation
boundary is not established by `DecodeOptions::max_pixels`.

The root OCR Cargo manifest, lockfile, features, and production source are not
changed by this package. It pins published `heic-rs = 0.1.4`, with only `std`,
without rayon. The default normal dependency graph contains only heic-rs;
PNG-only image 0.25.10 is a test dependency. The explicit optional
`native-reference` feature adds libheif-rs 2.7.0 with v1_19, no default features,
for A/B observations against existing native libraries. The lockfile contains
that optional graph too; its presence is not proof it is in the default build.
The probe MSRV declaration is 1.88 because of the PNG test dependency; the
decoder itself declares 1.85. No tool was installed. No full OCR dependency
chain has been removed or replaced.

## Reproduction

Run from this package's directory. Fetches are explicit and separate from offline
tests. Use a target directory outside the source checkout.

```powershell
pwsh -NoProfile -File ./fetch-fixtures.ps1
cargo fetch --manifest-path ./Cargo.toml
cargo fmt --manifest-path ./Cargo.toml --check
cargo test --locked --offline --manifest-path ./Cargo.toml --target-dir ../../../decoder-target -- --nocapture
```

With the existing process-local MSVC/Windows SDK/vcpkg environment described by
the RC1 build script (x64-windows-static-md, unchanged native security limits),
the optional reference run is:

```powershell
cargo test --locked --offline --manifest-path ./Cargo.toml --features native-reference --target-dir ../../../decoder-target -j 2 native_ -- --nocapture --test-threads=1
```

CHEAPEST_VERIFICATION_PATH: isolated L1 package tests. No native OCR build,
production pipeline, VM, or broad workspace verification is implied.

## Observed results

Recorded on Windows x64 with rustc 1.97.1 and Cargo 1.97.1. Nine observation tests
passed (exit 0). Six decode actual HEVC bytes into RGB8 pixels: white 16x16, flat
64x64, checker 64x64, RGB strips 96x32, gradient 512x512, and a four-tile checker
1024x1024. They check probe/decode dimensions, bit depth, grid reporting, buffer
layout/length, contrast, selected known flat colours, and complete RGB buffers
against the verified reference PNGs. Mean absolute error is per RGB channel on
the 0..255 scale, not a percentage. These are measurements, not broad format or
quality acceptance thresholds.

| Synthetic fixture | Pure vs Apple MAE / max difference | Native vs Apple MAE / max difference | Pure vs native MAE / max difference |
|---|---:|---:|---:|
| white 16x16 | 0 / 0 | native decode rejected | unavailable |
| flat 64x64 | 0.669515 / 2 | 0.664307 / 2 | 0.005208 / 2 |
| checker 64x64 | 0 / 0 | 0 / 0 | 0 / 0 |
| RGB strips 96x32 | 0.784722 / 2 | 2.729167 / 186 | 1.979167 / 188 |
| gradient 512x512 | 0.700551 / 5 | 0.534223 / 5 | 0.213821 / 3 |
| grid checker 1024x1024 | 0 / 0 | 0 / 0 | 0 / 0 |

The optional run observed libheif 1.23.1 through the same strict, HDR-to-8-bit,
transforms-enabled RGBA decode options as the current source, one decode thread,
and no native security-limit changes. The five successful native images match
candidate/reference dimensions and have opaque alpha. Two native observation
tests pass, but only **five of six native fixture decodes succeed**: white16 is
rejected with SecurityLimitExceeded, reporting an internal image size 160x64
above its 6400-pixel limit. That error is retained, not relabelled successful.
RGB is extracted from the opaque RGBA rows for the numerical comparison.

One test checks explicit AVIF rejection. Another checks the ordinary metadata
pixel limit. The ninth deliberately observes a mismatch in an owned, small copy
of flat-64.heic:

- The dependency's public BoxIter locates its single ispe property. Only its two
  dimension fields change, from 64x64 to 16x16; HEVC data remains untouched.
- Public probe reports coded dimensions 16x16. Public HEVC SPS probe still reports
  64x64. Decode uses `max_pixels=256`, `strict=true`, and one thread.
- Actual decode returns `PixelLimit { pixels: 4096, max_pixels: 256 }`.

The small runtime observation alone does not measure allocation order or peak
memory. The **checksum-verified published source** provides the matching call
trace: `src/decode.rs` checks container coded/transformed dimensions, then calls
decode_item and decode_coded; those call hevc::decode_still without a pixel-limit
argument. `src/hevc/decode/mod.rs` allocates
`Picture::new(sps.width, sps.height, ...)` before colour conversion. The later
`src/color.rs` check produces the observed 4096-pixel error. The internal plane
cap is 1<<30 u16 samples per plane, not the caller's max_pixels.

Consequently, the observation test passing means the documented behaviour was
reproduced, **not that the preallocation gate qualified**. No huge dimensions, OOM test,
unsafe allocator instrumentation, patched dependency, or custom HEVC parser was
used. A separate adapter could detect this particular uncropped mismatch through
the public SPS probe, but its dimensions are after SPS cropping; that API does
not expose raw SPS allocation dimensions or crop offsets. This probe does not
claim that such a preflight supplies the full required memory boundary.

The old native path's handle for the same forged small input reports 16x16, so
the application's 256-pixel precheck would allow it. Its strict decode then
rejects it with SecurityLimitExceeded (160x64 versus 6400), without changing its
upstream cap. Neither run measures peak internal allocation. The candidate's
unqualified caller-controlled preallocation boundary is therefore **not an
established relative memory/quality regression against the old decoder**. The
existing wrapper also relies on metadata prechecks and native security caps.

Cropping-specific reproduction, 10-bit, alpha, nonzero orientation metadata,
sequences, overlay, OCR, and candidate release build are NOT RUN. No RC2
completion, full format equivalence, or safe production replacement is claimed.

## Provenance and retained evidence

upstream-source.json binds the published package archive checksum and packaged
.cargo_vcs_info.json to commit
999470f514c8e9a1a7c37ea44ba70cc32b1308fe. Fixture downloads are restricted to that
commit and verified against reviewed Git blob SHA-1 plus exact byte lengths
before writing. fixtures-sha256.json additionally records the downloaded bytes.
evaluation.json and pixel-metrics.json record bounded results without local
developer paths. Executable hashes in evaluation.json are debug unit-test
executables only, not a newly built production OCR executable.

The fixtures are generated from synthetic PNG pixels, encoded by Apple's sips;
reference PNGs are sips decodes of those HEICs, not the original source PNGs.
All six are 8-bit 4:2:0 and contain no alpha. The exact upstream fixture
provenance and MIT/Apache-2.0 licences are retained under upstream/. Preserve the
notices. Upstream NOTICE does not grant HEVC patent rights. Branding/fonts and
unreviewed fuzz samples were not downloaded.

Ignored evidence/*.log are retained local diagnostics and can include local
filesystem paths. They are not part of a public source receipt. Earlier native
attempts include one probe compilation error (corrected version-array formatting)
and a test stopping at white16's native error; the final run collects all six
outcomes without weakening decoding or security settings. The final default run
has a nonfatal incremental-cache hard-link fallback warning.
