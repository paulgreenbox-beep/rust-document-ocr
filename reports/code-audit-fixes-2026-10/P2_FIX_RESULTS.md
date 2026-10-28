# P2 fix results and decisions

## Implemented reliability fixes

- Region OCR starts its page lifecycle exactly once before the first OCR
  operation. Native-text-only requests retain lazy OCR initialization, and
  subsequent cells reuse state within that region. The instrumented A-B-A
  lifecycle test passes; this is not a claim of general transcription accuracy.
- Dense lower-right grids require two different generic stamp label groups
  supported by different cells. Whole-token/label boundaries reject words
  such as `candidate`, `upscaled`, and `sheet metal`. The existing sparse
  rule is retained. Geometry is untouched. Paired dense data/stamp tests and
  DOCX/XLSX `include_title_blocks=false` tests pass.
- CLI preflight checks the target and parent before progress creation or OCR,
  without creating output. Staging and commit recheck. Concurrent populated
  and empty destinations are preserved. The atomic no-replace race guarantee
  is scoped to the validated Windows directory-rename behavior; this work
  does not certify equivalent POSIX race semantics.
- Progress write/newline/flush failures keep their first cause and disable
  the optional sink. A single best-effort diagnostic avoids recursive logging
  and does not panic on broken stderr. Creation failure also disables optional
  telemetry instead of aborting document processing. The reporter trait and
  JSONL event format are unchanged.
- A manual-only provisioned Windows native workflow is available, with no
  model/runtime downloads or uploads. Existing source CI is unchanged. The
  new self-hosted workflow has not been dispatched in this task.

## Investigated without production algorithm changes

The minimum cell confidence stays 0.05; the separate grid-noise gate stays
0.65. Available public data does not provide independently reviewed labels
for a reliable confidence-threshold A/B. Model agreement/confidence is not
ground truth. No false-fill reduction or useful-value retention claim is made.

Six fixed-geometry assignment cases compare absolute area, a normalized
guard, center containment and a combined criterion. All retain unique
assignment. A normalized guard rejects the almost-outside sliver currently
assigned; center-only also changes boundary behavior. These are policy
differences, not independently verified transcription fixes. Existing
production assignment is retained.

O(N²) grouping, PNG conversion, global memory budget and repeated PDF opens
have measured diagnostics in `PERFORMANCE_DIAGNOSTICS.json`. No spatial-index
rewrite, direct grayscale/Pix unsafe adapter, worker-default change or PDFium
handle-lifetime rewrite was introduced. Allocation counts and full-page OCR
peak memory at all worker levels remain unmeasured; conversion-only peaks
must not be presented as those values.

## Compatibility limits

The result and region schema versions remain one. Old JSON fixtures still
deserialize. New full-document warning variants are additive on the wire;
Rust consumers exhaustively matching the public `OcrWarning` enum need to
handle the new variant when rebuilding. Existing OCR engine implementations
inherit the conservative default classification method.
