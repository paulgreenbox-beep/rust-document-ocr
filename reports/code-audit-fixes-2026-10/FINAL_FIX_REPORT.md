# OCR CORE AUDIT FIXES — READY FOR REVIEW

Final review follow-up: see [FINAL_REVIEW_ADDENDUM.md](FINAL_REVIEW_ADDENDUM.md).
It supersedes the original warning-compatibility claim below: a new enum
variant in `warnings` was not readable by the baseline Serde reader. The
follow-up restores that enum and adds optional typed `diagnostics` instead.

Date: 2026-10-28. Repository: `paulgreenbox-beep/rust-document-ocr`.
Baseline HEAD exactly matches the audited commit
`6699a10dbfeff2471471b5eb2688e4d5394fe671`. Work is isolated on
`fix/ocr-core-audit-2026-10`; main and pre-existing production/research
installations were not edited. Source candidate: `c785091`.

## Delivered

Both P1 defects are fixed: recoverable supplemental OCR errors retain the
original cell and document, and incomplete-grid placeholders carry their
actual matrix indices without invented physical geometry.

Confirmed P2 fixes cover region adaptive-state isolation, dense TitleBlock
misclassification, read-only early output validation with final race checks,
and observable non-fatal progress write failures. An opt-in, manually
dispatched native Windows workflow was added without changing existing CI.

The remaining observations have explicit measured/research/deferred decisions
in [AUDIT_FINDINGS_REVALIDATION.md](AUDIT_FINDINGS_REVALIDATION.md). They are
not claimed as fixed. In particular, no confidence/assignment tuning,
spatial-index rewrite, global memory budget or PDFium lifetime refactor was
promoted.

## Verification actually executed

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --offline --locked --all-targets -j 1` | PASS |
| `cargo test --offline --locked -j 1 -- --test-threads=1` | PASS: 200 tests, 3 diagnostic tests ignored by default |
| `cargo clippy --offline --locked --all-targets -j 1 -- -D warnings` | PASS |
| `cargo check --offline --locked --features native -j 1` | PASS |
| `cargo test --offline --locked --features native -j 1 -- --test-threads=1` | PASS: 280 tests, 4 ignored by default |
| Isolated PDFium text-extraction test | PASS: 1 test |
| Three ignored grid diagnostics explicitly executed | PASS: 3 tests |
| Synthetic native-text+grid CLI | PASS, exit 0; JSON v1, legacy JSON, DOCX, XLSX |
| Synthetic raster OCR+grid CLI | PASS, exit 0; JSON v1, legacy JSON, DOCX, XLSX |
| Baseline/candidate text/geometry/export comparator | PASS, equal for both fixtures |
| CLI help comparison | PASS, byte-identical |
| New self-hosted native workflow | NOT RUN; requires a provisioned runner |

Baseline checks passed 179 source-only tests and 258 native tests (one
isolated native test ignored). Initial sandbox-only XLSX write failures were
also reproduced on the unchanged baseline; rerunning with approved temporary
filesystem access passed. New test-fixture and lint issues found during
development were corrected before the final passing gates.

## Baseline comparison

| Fixture | Pages | Route | Characters | Tables | Baseline/candidate |
|---|---:|---|---:|---:|---|
| committed synthetic-text-table | 1 | native_text+grid | 343 | 1 | identical text, cell values, confidence, routing, geometry and exports |
| committed synthetic-raster | 1 | ocr+grid | 627 | 1 | identical text, cell values, confidence, routing, geometry and exports |

The comparison excludes timing counters and volatile Office metadata. It
compares legacy JSON, stable page/table geometry, cell values/confidence,
language/routing, warnings, DOCX document XML and XLSX workbook/worksheet/
shared-string XML. These two fixtures are bounded compatibility controls,
not proof of accuracy on a private customer corpus.

Intentional changes are missing-cell row/column correction, local recovery
and warnings on injected OCR failures, early invalid-output rejection,
region reset lifecycle and the supported dense data/stamp role correction.
Physical bbox, grid topology, spans, OCR strings on successful controls,
models, routing, worker defaults, Cargo features and Cargo.lock are unchanged.

## Changed source and relevant locations

- `src/ocr.rs:138`: conservative default recoverability hook and routed delegation.
- `src/native/tesseract_engine.rs:238`: typed native error classification and tests.
- `src/pipeline.rs:112`: warning; fallback transaction/rollback around line 2374;
  two-page document regression around line 4209 and related mock tests.
- `src/api.rs:271`: additive warning; projection around line 980 and wire test.
- `src/table_grid.rs:199`: placeholder coordinates; classification at line 256;
  physical/stable geometry and bounded diagnostics tests.
- `src/region.rs:204`: one-reset lifecycle; fallback diagnostics/fatal propagation;
  sanitized region warning helper around line 689 and lifecycle tests.
- `src/main.rs:229`: early preflight; reporter at line 396; transaction commit
  at line 613; read-only validation at line 641; six CLI helper tests.
- `tests/semantic_table_contract.rs`: sparse-grid JSON and DOCX/XLSX filtering regressions.
- `examples/audit_perf_probe.rs`: bounded native conversion/recognition/memory diagnostics.
- `scripts/compare_audit_results.py`: reproducible sanitized compatibility comparison.
- `.github/workflows/native-windows.yml`, `docs/NATIVE_CI.md`: opt-in provisioned native CI.
- Nine sanitized report files in this directory.

## Separate commits

1. `08d590b` — isolate recoverable supplemental cell OCR failures.
2. `d9786a5` — preserve incomplete-grid placeholder coordinates.
3. `5649cf1` — reset adaptive OCR state once per region request.
4. `7d7ccaf` — output preflight and progress failures.
5. `f771d6d` — dense drawing title-block evidence.
6. `52e9c93` — bounded performance and compatibility diagnostics.
7. `c785091` — opt-in native Windows CI.

The subsequent report commit records this evidence. The branch is intended
for a pull request into main, with no automatic merge or GitHub Release.

## Remaining limitations

- Low-confidence fill and word-overlap policy need independently reviewed
  labels before altering successful OCR outputs.
- Dense title-block detection remains heuristic; the pre-existing sparse
  lower-right rule is retained.
- Allocation counts and full-pipeline peak RAM across all worker counts are
  not measured. Safe conversion-only RAM measurements are explicitly labeled.
- PNG conversion overhead and quadratic grouping are confirmed; their
  optimizations require separate equivalence/memory reviews.
- Reopen-only PDFium cost is not isolated from extraction/render work.
- Superseded by final review: cell failures now use the additive `diagnostics`
  field, not a new v1 `warnings` enum variant. See the linked compatibility
  report for JSON, strict-reader, and Rust struct-literal boundaries.
- Native self-hosted CI execution remains pending runner provisioning.

Raw local logs, runtime copies, binary controls and synthetic output documents
remain ignored and are not included in the public commits. No user text,
private paths, models, traineddata or DLLs are published by this patch.
