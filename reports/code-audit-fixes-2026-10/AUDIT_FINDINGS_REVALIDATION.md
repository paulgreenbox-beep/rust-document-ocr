# Audit findings revalidation — 2026-10-28

Repository: `paulgreenbox-beep/rust-document-ocr`.
Audited and reproduced baseline: `6699a10dbfeff2471471b5eb2688e4d5394fe671`.
Source candidate: `c785091` on `fix/ocr-core-audit-2026-10`.
The baseline is retained as a detached worktree and as a separately copied
native executable. The original OCR development/research installations were
not edited. No customer documents were used.

CHEAPEST_VERIFICATION_PATH: named pure Rust mock/unit regressions (L0), then
the requested source-only gates and native gates once the final source was
stable. No release build or upstream workspace build was necessary.

| ID | Priority | Issue | Reproduced | Fixed | Regression / evidence | Decision |
|---|---|---|---|---|---|---|
| 01 | P1 | Cell OCR failure abort | Yes: recognition `?` propagated through both finish paths | Yes | Required/optional failures, rollback after later passes, two-page completion | FIXED |
| 02 | P1 | Placeholder coordinates | Yes: missing (2,1) projected as (0,0) | Yes | Sparse 3x3, all eight physical cells and stable/legacy JSON | FIXED |
| 03 | P2 | Region Tesseract state | Yes: region bypassed begin_page | Yes | Instrumented A-B-A lifecycle; native region suite | FIXED |
| 04 | P2 | Dense table classified as TitleBlock | Yes: lower-right 4x4 geometry sufficed | Yes | Paired same-geometry dense data/stamp, substring negatives, DOCX/XLSX filtering | FIXED |
| 05 | P2 | Low-confidence cell fill | Permissive policy confirmed; false fills not independently labeled | No | Existing confidence/noise gates retained | RESEARCH ONLY |
| 06 | P2 | Word/cell microscopic intersection | Yes: sliver assigned despite center outside | No | Six deterministic strategies/cases, uniqueness and geometry invariants | CONFIRMED — DEFERRED |
| 07 | P2 | Late output path validation | Yes: OCR preceded staging validation | Yes | Read-only preflight, staging, populated/empty concurrent destination | FIXED |
| 08 | P2 | Lost progress write/flush failures | Yes: ignored errors | Yes | Failed write and flush retain first cause and disable sink without retry | FIXED |
| 09 | P2 | Grid O(N²) | Yes | No | 100/500/1000/5000-cell measurements; same algorithm retained | CONFIRMED — DEFERRED |
| 10 | P2 | PNG OCR overhead | Yes | No | Clone/encode/decode/raw recognition measured separately | RESEARCH ONLY |
| 11 | P2 | Global RAM budget | Yes: only per-page bound | No | Safe 1/2/4/8/16 conversion probes, not full-document stress | CONFIRMED — DEFERRED |
| 12 | P2 | Native CI coverage | Yes: original CI source-only | Opt-in workflow added | Local native gates pass; self-hosted workflow not dispatched | FIXED (workflow execution NOT RUN) |
| 13 | P3 | PDFium reopening | Yes by call-path inspection | No | Bounded repeated extract/render timings | CONFIRMED — DEFERRED |

## Frozen baseline contract

- Default Cargo features empty; native features and Cargo.lock unchanged.
- Rust/Cargo 1.97.1. Native build uses the existing process-local LLVM/vcpkg/MSVC setup.
- Schema version 1, legacy document projection, table/cell field names, bbox,
  spans, semantic classifications and export options retained.
- Default languages `rus+eng`, OCR DPI 300, grid DPI 100, four page workers,
  optional cell fallback enabled. No model/profile/threshold changes.
- Physical grid detection, native-text routing, orientation and word
  assignment algorithms unchanged. The intentional classification fix changes
  only the structural role of unsupported dense title-block candidates.
- Baseline CLI `--help` and candidate `--help` hashes match.
- Baseline: 179 source-only tests; 258 native tests, one native test ignored
  for isolated binding initialization. Both committed synthetic PDFs completed.

Only the sanitized Markdown/JSON reports are published. Raw logs, binaries,
runtime files and generated DOCX/XLSX remain ignored local artifacts.
