# Bounded HEVC decode and extended formats — research result

Date: 2026-11-07. Branch: `experiment/heic-rs-rc2`.

**Scoped image-buffer gate and named format tests: PASS. Production adapter,
full OCR equivalence and RC2 promotion: HELD.** Working OCR and RC1 are unchanged.
The local derivative is `heic-rs 0.1.4-ocr.1`, not an upstream release.

## What changed

The original caller limit was checked against container dimensions, while raw
HEVC buffers could be allocated before a later colour-output check. The local
derivative passes a checked raw-SPS budget through the codec boundary. A serial
preflight inventories the **entire requested decode** before thread-pool
creation, tile dispatch or Geometry/Picture/filter allocation:

- primary picture and every primary grid tile occurrence;
- requested auxiliary alpha and every alpha-grid tile occurrence;
- repeated references charged per occurrence, not deduplicated;
- largest raw luma area of **every** supplied SPS definition, including unused
  and same-ID overwritten definitions, before conformance cropping;
- checked aggregate arithmetic, independent of strict-decoding settings.

Every actual codec call also rechecks the raw maximum before slice RBSP copying
and checks the selected SPS before Geometry/Picture work. `None` explicitly opts
out of the caller ceilings; the default remains finite. The limited seam returns
a typed `ResourceLimit { stage, required, limit }`, with raw luma-site units.

For the retained tiny mismatch, container16x16 / public cropped-SPS64x64 hides
raw-SPS160x64. At max_pixels256 the unmodified decoder still reports the late
PixelLimit4096; the derivative rejects required10240 before image buffers, for
both strict=false and true. Raw cropping and aggregate grid/alpha cases are
separate tests, not inferred solely from this mismatch.

### What this limit does not prove

`Some(N)` separately bounds output canvas pixels and aggregate untrimmed luma
sample sites. It is **not N bytes of heap**. Chroma, u16 planes, geometry/filter
maps, retained tiles, colour output and transform copies have different bounded
multipliers. Input bytes, metadata parsing allocations and caller thread-count
policy remain separate concerns. No whole-parser memory safety/resource proof,
allocator-byte/RSS measurement, giant allocation or OOM experiment is claimed.

## Tests and independent comparisons

Windows x64; Rust/Cargo1.97.1; offline and locked. Source-only debug test builds,
not an OCR release build. The final local run:

| Gate | Result |
| --- | --- |
| Isolated rustfmt | PASS, exit0 |
| Vendor std-only library | 83 passed, exit0 |
| Vendor parallel-feature resource subset | 10 passed, exit0 (overlaps std tests) |
| Vendor no-std compile | PASS, exit0 |
| Evaluator + unmodified upstream reference | 11 unit +6 extended passed, exit0 |
| Evaluator + native +upstream references | 13 unit +7 extended passed, exit0 |
| Scoped evaluator Clippy, all targets, no-deps, -D warnings | PASS, exit0 |

The per-thread test counters are at actual Geometry/Picture constructor entry,
before image-vector creation. Rejected cases observe zero entries; positive
controls reach both. Serial preflight is tested with default/one/two-thread
options; a valid two-thread grid exercises admitted parallel dispatch. These
counters are absent from ordinary library builds.

Frozen new fixtures contain real Main10 (matching VPS/SPS/hvcC and decoded
10-bit planes), HEVC auxiliary alpha128, nonsquare irot90/180/270 and both imir
axes. The retained generator is a test-only bitstream/container writer; it does
not merely relabel an8-bit stream. Its lineage is shared with the candidate,
so independent libheif validation is recorded rather than treating encoder/
decoder agreement as an independent oracle. No codecs were installed.

| Case | Comparison | Result |
| --- | --- | --- |
| Original six fixtures | Local vs unmodified0.1.4, complete RGB8 arrays | Byte-identical |
| Genuine Main10 | Local vs libheif1.23.1 RGBA8 | MAE0.75; max1 |
| Genuine Main10 | Local vs libheif1.23.2 RGB16 PNG,12,288 channels | 33516 vs33536; MAE20; max20 in16-bit units |
| Nonopaque alpha128 | Local vs libheif1.23.1/1.23.2 RGBA8 | Alpha exact128; full RGBA MAE0.00390625; max2 |
| irot90/180/270 | Local vs independent pixel mapping, transforms-disabled controls | Byte-exact geometry/order, expected nonsquare dimensions |
| irot90/180/270 | Local vs libheif1.23.1 RGBA8 | Each MAE1.484375; max188 at strip boundaries |
| Both imir axes, before fix | Published0.1.4 vs libheif1.23.1 RGBA8 | Each MAE64.144147873; max255; wrong axis |
| Both imir axes, after fix | Local vs libheif1.23.1 RGBA8 | Each MAE0.160366058; max3; independent geometry exact |

The imir test first failed with corrected independent expectations (exit101),
while the native reference agreed with them. A minimal local parser change
maps code0 to top/bottom exchange and code1 to left/right exchange, matching
the pinned [libavifv1.3.0 HEIF explanation](https://github.com/AOMediaCodec/libavif/blob/v1.3.0/include/avif/avif.h)
and [libheifv1.23.1 implementation](https://github.com/strukturag/libheif/blob/v1.23.1/libheif/box.cc).
The optional upstream-reference test reproduces the original swapped axis.
Fixtures and tolerances were not changed to conceal the failure.

All seven new fixtures decode with the existing libheif1.23.1 baseline, strict
mode and unchanged native limits. For the original six, native success remains
**5/6**: white16 is rejected by its existing160x64 vs6400 security guard. That
failure is retained, not substituted with candidate pixels or labelled PASS.
Neither the20-level RGB16 difference nor max188 strip-edge delta is hidden by
a general equivalence threshold. OCR/photographic accuracy is not established.

## Reproduce without touching OCR

Run inside this tool directory, with cached packages and an explicit target
directory outside the checkout (`$target`). No fetch/install or OCR rebuild is
part of these commands:

```powershell
cargo fmt --manifest-path ./Cargo.toml --all --check
cargo test --offline --locked --manifest-path ./Cargo.toml -p heic-rs@0.1.4-ocr.1 --no-default-features --features std --lib --target-dir $target
cargo test --offline --locked --manifest-path ./Cargo.toml -p heic-rs@0.1.4-ocr.1 --no-default-features --features std,parallel --lib resource_limit --target-dir $target
cargo check --offline --locked --manifest-path ./Cargo.toml -p heic-rs@0.1.4-ocr.1 --no-default-features --lib --target-dir $target
cargo test --offline --locked --manifest-path ./Cargo.toml --features upstream-reference --target-dir $target -- --nocapture --test-threads=1
cargo test --offline --locked --manifest-path ./Cargo.toml --features native-reference,upstream-reference --target-dir $target -- --nocapture --test-threads=1
cargo clippy --offline --locked --manifest-path ./Cargo.toml --all-targets --no-deps --features native-reference,upstream-reference --target-dir $target -- -D warnings
```

The optional native run needs the existing process-local MSVC/SDK/vcpkg
environment. Default normal dependencies are just local heic-rs/std, no native
library or Rayon. `fixture-generation` is separate and not used in regressions.
CI exercises pure vendor/resource/evaluator tests, not Windows native A/B or
clean-host acceptance. Updated-head CI is tracked in draft PR7.

## Evidence and remaining work

- [bounded-evaluation.json](bounded-evaluation.json): gates, private-log hashes,
  input and debug test executable hashes; no private paths or binaries.
- [bounded-pixel-metrics.json](bounded-pixel-metrics.json): actual arrays of
  measurements, reference versions/depths and preserved before/after values.
- [vendor-upstream-manifest.json](vendor-upstream-manifest.json):92 original
  source identities rehashed against unchanged registry cache (zero mismatches).
- [vendor-current-manifest.json](vendor-current-manifest.json):100 current
  source/notice/metadata files; [vendor-patch.diff](vendor-patch.diff): review
  diff for11 modified original source files. Local new resource tests and
  manifest are separately visible, not falsely attributed upstream.
- [provenance_extended.md](provenance_extended.md): frozen fixture identities,
  generator source, native library/tool versions and licensed provenance.

Historical `evaluation.json` and `pixel-metrics.json` are unchanged. The source
licenses remain MIT OR Apache-2.0 with original NOTICE; HEVC patent rights are
not granted by that license. No full-app LGPL-removal claim follows from an
isolated permissive decoder.

Next, separately review the vendored patch and broaden malformed/sequence/
chroma/combined-transform/fine-font coverage before considering an opt-in OCR
adapter with full EXE/linker and OCR comparisons. Do not promote from these
tiny decoder fixtures alone. RC1 and the installed OCR retain their hashes and
configuration. Windows Sandbox and separate clean Windows x64 remain
**NOT RUN — DEFERRED BY OWNER**. Binary release remains **BLOCKED: LICENSING**.
