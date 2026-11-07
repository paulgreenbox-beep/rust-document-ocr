# OCR research derivative patch notes

Local version: `0.1.4-ocr.1`. This is not an upstream heic-rs release and is not
a production OCR replacement or release-acceptance claim.

Upstream origin: published heic-rs 0.1.4, archive SHA-256
`8883c3bfa6b4ff19a40e52a6dc051c5612cbe21bf408fc6d4875e8dfd777cb66`,
packaged VCS commit `999470f514c8e9a1a7c37ea44ba70cc32b1308fe`.
Original MIT/Apache-2.0 licences and NOTICE are retained. Modifications below
are marked at the start of each changed file, dated 2026-11-07. No upstream
registry-cache source was modified.

## Changed contract and scope

`DecodeOptions::max_pixels=Some(N)` now has two **separate** conservative units:

1. Each output/intermediate canvas must fit N displayed pixels (the existing
   PixelLimit guard remains).
2. The complete requested decode must fit N aggregate **untrimmed HEVC luma
   sample sites**. Every requested primary picture, primary grid tile,
   requested alpha picture, and alpha-grid tile occurrence is charged. Repeated
   references are charged repeatedly because they are decoded repeatedly.

Each coded occurrence is charged the largest raw width*height across every SPS
definition supplied in its configuration, including unused definitions and
earlier definitions subsequently replaced by the same SPS id. Conformance
cropping, tiny/forged ispe metadata, or disabled strictness do not reduce that
charge. Checked addition rejects overflow rather than wrapping.

This is **not an exact heap-byte or peak-RSS cap**. Chroma (up to 4:4:4), u16
sample storage, Geometry maps, filters, cropped frames, the retained set of
tiles, composed alpha, colour output and transient transform copies are bounded
multiples of admitted raw sites and bounded output canvases. Their constants
differ by format/stage. Encoded-input size, metadata parsing allocations, and
caller-selected thread-pool sizes are separate resource concerns; this patch
does not claim to bound all parser/host memory by N bytes. Callers must retain
their independent input-byte policy.

The preflight is serial and precedes private thread-pool creation and parallel
tile dispatch. Inputs are immutable; the complete set of coded occurrences
fits before any HEVC image buffers exist. Each actual codec call independently
rechecks the raw SPS bound, before slice RBSP copying at the public seam and
again immediately before Geometry/Picture/filter work. Thus parallel workers
cannot race a shared remaining-budget counter or allocate partially before a
later aggregate rejection.

Default options remain bounded by DEFAULT_MAX_PIXELS. The legacy public
`hevc::decode_still` now uses that same finite default instead of an unbounded
codec seam. An explicit `DecodeOptions::max_pixels=None` preserves upstream's
intentional opt-out from both caller ceilings and is not qualified for
untrusted inputs. No fallback or hidden inflation changes a supplied Some(N).

## New public APIs

- `ResourceStage::{CodedPicture, AggregateCodedPictures}`, re-exported at the
  crate root.
- `Error::ResourceLimit { stage, required, limit }`: units are untrimmed luma
  sample sites. `required=u64::MAX` is the documented overflow sentinel.
  Existing `Error::PixelLimit` remains unchanged.
- `hevc::decode_still_limited(parameter_sets, slices, max_coded_pixels)`.
- `hevc::probe_coded(parameter_sets) -> CodedImageInfo`: raw and conformance-
  cropped dimensions, sample depth/chroma, and `max_raw_pixels` across every
  supplied SPS. Dimension/depth fields describe the first PPS-selected SPS,
  as the existing probe does; max_raw_pixels is deliberately more conservative.

The existing synth/bench API is preserved for separate synthetic fixture work.
No production OCR source, root dependency features, or runtime assets are
changed by this source patch.

## Modified source files

- `src/error.rs`: typed stage/resource error and units.
- `src/lib.rs`: ResourceStage export.
- `src/image.rs`: explicit raw-aggregate/output/None contract documentation.
- `src/decode.rs`: complete main/alpha/grid preflight before dispatch, checked
  cumulative accounting, and bounded codec calls; overflow regression.
- `src/hevc/mod.rs`: bounded legacy seam, limited seam, and raw geometry probe;
  test-only counter interfaces.
- `src/hevc/error.rs`: codec-boundary resource error propagation.
- `src/hevc/decode/sets.rs`: largest raw area across every parsed SPS definition,
  before same-id replacement, using checked multiplication.
- `src/hevc/decode/mod.rs`: selected raw-SPS defence before image tables.
- `src/hevc/decode/geom.rs`: test-only Geometry-constructor entry counter.
- `src/hevc/picture.rs`: test-only Picture-constructor entry counter.
- `src/props/simple.rs`: independently reproduced imir axis mapping fix.
- New `src/hevc/resource_tests.rs`: small synthetic resource regressions.

## Reflection mapping correction (separate reproduced defect)

The extended A/B corpus exposed the published 0.1.4 imir interpretation as
reversed: both axes differed from libheif by MAE 64.144148, max 255. Corrected
independent pixel mappings failed before this change; the native reference
matched them. `src/props/simple.rs` now interprets 0 as top/bottom exchange and
1 as left/right exchange, matching ISO/IEC 23008-12:2022 as documented by
[libavif v1.3.0](https://github.com/AOMediaCodec/libavif/blob/v1.3.0/include/avif/avif.h)
and [libheif v1.23.1](https://github.com/strukturag/libheif/blob/v1.23.1/libheif/box.cc).
Fixtures and pixel observations were not rewritten to hide the mismatch.
Post-fix verification is recorded in the evaluator's new bounded receipt.

## Original limit-patch verification (before the reflection correction)

CHEAPEST_VERIFICATION_PATH: isolated focused L0 regressions, then vendor L1 lib
tests. Windows x64, Rust/Cargo 1.97.1, offline and locked, debug test profile.
No large allocation, OOM test, unsafe allocator instrumentation, native library,
or private image was used by these vendor tests.

- Focused std-only resource tests: **9 PASS**, exit 0.
- Full std-only library tests: **82 PASS**, exit 0.
- Parallel-feature resource tests: **10 PASS**, exit 0; includes a valid
  two-thread grid with an explicit sufficient aggregate budget.

Rejections assert zero Geometry-entry and zero Picture-constructor counts on
the calling test thread. Positive small cases assert that both allocation
boundaries are actually reached. The reject cases cover tiny ispe versus raw
SPS, conformance-cropped SPS, unused/overwritten larger SPS definitions,
strict=false, aggregate main/alpha/alpha-grid and repeated tile charges, hidden
larger grid output, and checked aggregate overflow. Parallel dispatch is only
entered after the tested serial request preflight; its valid path is tested
separately. Counters are cfg(test), per-thread, and absent in library builds.

These checks establish the stated algorithmic image-buffer gate, not an
allocator-byte measurement or a proof of every resource/property of hostile
HEIF input. Extended format/old-decoder A/B checks are separate evaluator work.
