# PR #1 final review and compatibility gate

**PR #1 FINAL REVIEW — READY TO MERGE**

Date: 2026-10-28. PR: <https://github.com/paulgreenbox-beep/rust-document-ocr/pull/1>.

- Original PR HEAD: `3b83b375f656719a371efcf180c0b0a2cd33113e`.
- Audited baseline: `6699a10dbfeff2471471b5eb2688e4d5394fe671`.
- New reviewed code HEAD: `61450625fd2b260762e28205386611730cd886da`.
- Code commits: `01f8c978b31af18444ebe3cbe718dcb1877743c7` (late dependency)
  and `61450625fd2b260762e28205386611730cd886da` (v1 compatibility).
- A report-only commit follows the code HEAD. Its hash is the final PR tip,
  available through `gh pr view 1 --json headRefOid`; it changes no tested code.
- Work is confined to `fix/ocr-core-audit-2026-10`; no merge or force push.

## A. Reproduction and correction

The hypothesized late consensus failure **was reproduced**, not dismissed by
inspection. The synthetic 6-row, 9-column engineering specification has the
real numbered header and confirmed manufacturer column 4. Two original `ABC`
values plus a provisional OCR `ABC` create the required three votes. The
table-wide consensus changes `ABD` to `ABC`; a later recoverable technical
request fails on the provisional source. Before correction the source rolled
back, but the dependent `ABC` survived instead of the original `ABD`.

The integrated mock asserts the actual OCR request sequence and identical
prepared synthetic crops. It also contains an independent successful rescue.
Before-fix execution failed with actual `ABC`, expected `ABD`, at row 5,
column 4, confidence 0.60. The corrected full matrix matches expectation.

A further executable regression found a transitive dependency: generic and
technical OCR agree on `A1C` at 0.90. The existing selector accepts this from
consensus-derived `ABC` at 0.60 (one edit), but rejects it from original `ABD`
at 0.60 (two edits). Merely exempting technical replacements from rollback
left an invalid `A1C` after the supporting third vote failed. This second
regression was also RED before its correction.

The minimal correction retains successful-path request order and selectors:

1. Record pre-consensus candidates and which indices already represent an
   independent successful OCR rescue.
2. Run existing bounded rechecks, retaining actual generic/technical evidence
   for accepted technical replacements.
3. On a late recoverable failure only, restore failed cells and undo
   consensus-only edits. Revalidate dependent technical replacements against
   the pre-consensus primary with the **unchanged** selector and saved evidence.
4. Recompute manufacturer consensus excluding failed coordinates. Preserve
   independently admissible replacements; never roll back the entire table.
5. Compute fallback confidence from surviving indices only. The failed
   hypothesis and cancelled consensus-only contributions are excluded.

There are no extra OCR calls, changed thresholds, new semantic roles,
manufacturer dictionaries, profile changes, or physical geometry changes.
Fatal required-engine errors still propagate. Optional-model behavior is
unchanged. The failure fixture's aggregate fallback confidence is 0.91 from
the independent rescue alone, not inflated by the cancelled candidates.

Implementation: [transaction/revalidation](../../src/pipeline.rs#L2659),
[private evidence context](../../src/pipeline.rs#L2800).
Regression tests start at [pipeline.rs:4798](../../src/pipeline.rs#L4798):

- `late_cell_failure_invalidates_dependent_consensus`
- `late_cell_failure_restores_source_cell`
- `late_cell_failure_preserves_independent_rescues`
- `successful_consensus_remains_unchanged`
- `fatal_cell_error_still_propagates`
- `rollback_preserves_bbox_spans_and_confidence`
- `failure_warning_has_no_ocr_text`
- `late_cell_failure_revalidates_consensus_dependent_technical_replacement`
- `late_cell_failure_preserves_independently_admissible_technical_rescue`
- `successful_consensus_dependent_technical_rescue_remains_unchanged`

The matrix assertions include text, confidence, bbox and spans; grid bbox,
lines, intersections and detector confidence are also checked. Both original
review hypotheses were confirmed; neither is marked NOT REPRODUCED.

## B. Real old-reader outcome and compatibility policy

The real baseline `OcrResult` and `OcrWarning` definitions are frozen in
[tests/support/baseline_v1.rs](../../tests/support/baseline_v1.rs). Supporting
DTO definitions are unchanged and shared, not aliases for the candidate
envelope/enum. Before correction, serializing the new cell warning and reading
it with the baseline yielded Serde `unknown variant cell_ocr_fallback_failed`
(the initial string-based run reported line 1, column 123). A retained test
asserts this rejected pre-fix shape.

**Option A is implemented.** The eight baseline `warnings` kinds are restored.
Cell failures are instead retained in typed optional `diagnostics`, omitted
when empty and defaulting to empty when absent. Example element:

```json
{"kind":"cell_ocr_fallback_failed","page_number":1,"table_index":0,"row":1,"column":2,"reason_code":"recognition_failed"}
```

`schema_version` remains 1. The baseline Serde reader now successfully reads
both success and cell-failure results. The new reader retains the diagnostic
through round-trip serialization; actual native pipeline projection is tested.
Existing warnings and their ordering are retained, without copying private
OCR/error text into diagnostics. Legacy `result.json` remains `document` only.

Compatibility is **not** the same for every consumer:

- Standard baseline Serde envelope readers and tolerant JSON clients work.
- Strict unknown-field-rejecting clients must admit the additive field.
- Baseline exhaustive `OcrWarning` matches compile unchanged; consumers of
  the short-lived pre-fix variant must use `OcrDiagnostic` instead.
- Public `OcrResult` struct literals/exhaustive destructuring require the new
  field or a constructor/`..`. This is an explicit Rust source break, not a
  claim of total Rust source compatibility.
- The earlier low-level `PipelineWarning` enum addition also needs a new arm
  in exhaustive low-level consumers. No application ABI compatibility is claimed.

See [JSON_V1_COMPATIBILITY.md](../../docs/JSON_V1_COMPATIBILITY.md),
[typed API](../../src/api.rs#L306), [diagnostics field](../../src/api.rs#L334),
and [native projection test](../../src/api.rs#L1182).
All eight [compatibility tests](../../tests/v1_warning_compat.rs#L37) pass,
including the six required names, pre-fix rejection witness and exhaustive
enum consumer compilation.

## C. Final verification actually run

| Command/check | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --offline --locked --all-targets -j 1` | PASS |
| `cargo test --offline --locked -j 1 -- --test-threads=1` | PASS: 218 passed, 3 diagnostic tests ignored |
| `cargo clippy --offline --locked --all-targets -j 1 -- -D warnings` | PASS |
| `cargo check --offline --locked --features native -j 1` | PASS |
| `cargo test --offline --locked --features native -j 1 -- --test-threads=1` | PASS: 299 passed, 4 ignored |
| Native isolated PDFium extraction, fully qualified exact test with `--ignored --lib` | PASS: 1 test actually executed |
| Native targeted `pipeline::tests::` | PASS: 51 tests |
| Native targeted `v1_warning_compat` | PASS: 8 tests |
| Native pipeline warning-to-diagnostic projection | PASS in full native suite |
| Fresh baseline/candidate CLI synthetic controls | PASS: four runs, exit 0, no timeout |
| Synthetic text/geometry/confidence/export comparison | PASS: equal for both fixtures |

The full suites rerun both earlier P1 fixes (cell failure isolation and missing
cell coordinates) and the existing P2 region lifecycle, dense title-block,
output preflight and progress write/flush regressions. No private PDF was used.
Models, DLLs, dependencies and global environment were not changed.

During development Clippy caught an eight-argument private helper; failures
and evidence were grouped in a private diagnostic context rather than suppressing
the lint. Windows incremental-cache hard-link warnings fell back to file copying;
tests passed. An initial abbreviated `--exact` PDFium filter selected zero
tests; the corrected fully-qualified isolated invocation executed and passed one.
The unavailable system Python path was replaced by an existing local Python
3.11 interpreter for the standard-library comparator; nothing was installed.

## D. Fresh synthetic A/B

| Fixture | Pages | Route | Characters | Tables | Result |
|---|---:|---|---:|---:|---|
| `synthetic-text-table.pdf` | 1 | native_text+grid | 343 | 1 | equal |
| `synthetic-raster.pdf` | 1 | ocr+grid | 627 | 1 | equal |

Both sides emitted JSON v1, legacy JSON, DOCX and XLSX. Text, successful cell
values, page/cell/grid confidence, physical bbox, spans, routing, language and
warnings are equal. Legacy JSON and selected Office document/workbook/worksheet
XML are equal; volatile timings and Office metadata are excluded, not called
byte-identical exports. Successful controls have no diagnostics field.

Fresh baseline processes used the retained baseline executable from the prior
verified worktree, SHA-256
`839c964da0c5a61a23921c2cfe4d113c518f90a18128d4fc2d045459396777e7`.
The untouched baseline worktree still has HEAD `6699a10...` and clean status.
Candidate was rebuilt from the final source. Runtime hashes match the prior gate.
Local raw logs and comparator outputs remain ignored under this report directory's
`final-review-final/`; no document output or runtime binary is committed.
These two controls demonstrate bounded compatibility, not real-world OCR accuracy.

## E. GitHub and remaining limits

Both source CI checks on the new reviewed code HEAD passed:
[branch push](https://github.com/paulgreenbox-beep/rust-document-ocr/actions/runs/40671925314/job/122760586444)
and [PR](https://github.com/paulgreenbox-beep/rust-document-ocr/actions/runs/40671931932/job/122760607827).
GitHub reported `MERGEABLE`. The separate opt-in self-hosted native workflow
is **NOT RUN**; actual local native passes do not imply it ran. No merge was
performed. The report-only tip is checked separately before final handoff.

This is a source PR, not a new redistributable runtime or deployment. Deferred
quality, overlap, performance and native-runtime licensing work from the original
audit remains deferred. No algorithms or traineddata were promoted.

## Changed files in this follow-up

- `src/pipeline.rs`: bounded dependency rebase, evidence revalidation and 10 regression tests.
- `src/api.rs`: typed additive diagnostics and native projection regression.
- `src/lib.rs`: `OcrDiagnostic` reexport.
- `tests/support/baseline_v1.rs`: frozen baseline envelope/enum definitions.
- `tests/v1_warning_compat.rs`: 8 compatibility tests.
- `docs/JSON_V1_COMPATIBILITY.md`: policy and consumer-specific limits.
- `docs/OCR_PIPELINE.md`: compatibility reference.
- `reports/code-audit-fixes-2026-10/FINAL_FIX_REPORT.md`: superseded claim correction.
- This `FINAL_REVIEW_ADDENDUM.md`: measured final-review evidence.
