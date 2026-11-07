# Windows OCR features and release readiness

Review date: 2026-11-07. This is source/documentation and local-package work,
not permission to distribute or install a binary release.

**STAMP/TABLE FEATURES — DOCUMENTED WITH VERIFIED SCOPE**

**EG79 — RETAINED AS CORE TECHNICAL OCR COMPONENT**

**PURE HEIC WINDOWS PROFILE — REVIEWED / TESTED**

**WINDOWS PORTABLE RELEASE — BLOCKED: LICENSING**

## Source and artifact identities

- Starting point: [PR #8](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/8),
  `a784ab553c8a7634db9b2af7363bc59791b1bc0d`; all four exact-head workflows
  passed before work began. PRs #5–8 and their history were not rewritten.
- Documentation/test branch: `release/pure-heic-profile-capabilities`,
  [PR #9](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/9),
  base `a784ab553c8a7634db9b2af7363bc59791b1bc0d`,
  HEAD `389665e71b80edf2768e05e1dc11184b6c114514`.
- Separate stacked packaging branch: `fix/pure-heic-packaging-profile`,
  base `389665e71b80edf2768e05e1dc11184b6c114514`. Tested packaging-code commit:
  `b546ef125fdaa83f48d9009056e7d1c01f1d1a4b`; the final report-only commit is
  identified by the packaging PR's current HEAD and exact-head CI links.
- Reused release EXE source: `3185a5f5f1c79a5036b2a0c6d63e6334c4f66f26`,
  tree `3349ed2e8a8cee6a9b14f2d55649173dd6479052`.
- EXE SHA-256: `c6cddc7516d5e44b225cde2fabb35e534016e05a7c900774b087019e538dfc23`.
- Cargo.lock SHA-256 unchanged:
  `890b2dfb70b4049c3feb72892d231ddc28730896701e4d88d4136761de080376`.

No Rust OCR implementation, Cargo features/dependencies, PDFium DLL, model,
traineddata, thresholds, routing, orientation, assignment or four-worker default
was changed. Test binaries were compiled; **no release EXE was rebuilt**.

## What is implemented and what is not

[FEATURES.md](FEATURES.md) maps each capability to actual modules and named
controls. The key findings are:

| Capability | Actual implementation and boundary |
|---|---|
| Drawing title blocks | `table_grid::classify_grid` uses lower-right compact geometry plus sparse occupancy or supporting label groups; text/cell geometry are retained as `title_block`. No GOST-form, signature/seal or guaranteed requisite extractor. |
| Ruled tables | Raster adaptive threshold, directional opening, observed axes/intersections and closed-cell connectivity in `table_grid`; NativeText or OCR words populate cells. Grid detection is not text correctness. |
| Incomplete/sparse grids and spans | Specific shared-edge/missing-slot/observed-span rules have synthetic tests. Arbitrary incomplete grids and complex merges remain partial; physical merges are not reproduced as native Word/Excel merged cells. |
| Vector boundaries | PDFium source paths can refine already detected raster edges; this is not a standalone vector-table detector and does not walk arbitrary nested forms. |
| Borderless tables | Conservative repeated positioned-word alignment fallback, not a universal layout or neural detector. |
| NativeText/word ownership | Full page text is preserved alongside tables; PR #5's source glyph/cell ownership and custom-PageSource compatibility remain intact. |
| Semantics | Local uncertain domain/header-role classification is separate from structural kind; it does not invent or correct source values. |
| Export | JSON schema v1 plus legacy JSON remain compatible; multiple data tables export, title blocks can be separated/omitted by option. DOCX/XLSX are content projections, not drawing facsimiles. |

Overall stamp OCR outside the named bounded controls remains
**IMPLEMENTED — VALIDATION INCOMPLETE**. The prior 40-PDF counts are not evidence
of universal stamp/table accuracy.

EG79 is discovered as optional `eng_technical_eg79_v1` by the engine, while the
**full pure portable profile requires its actual retained payload**. It is used
only in bounded suspicious nonempty technical data-cell rechecks; structural
title blocks are excluded. It is not the stamp detector or a replacement for
full-page rus+eng. Selector tests do not establish real model-weight accuracy.

The base is the pinned English `tessdata_best` artifact, not the separately used
standard `tessdata_fast` language files. Exact retained derived-weight SHA-256:
`7ceee3f56f290c3b90c1670d8e0c7dd86e344759ff03915407c53369f1c8437c`.
See [the rights matrix](docs/compliance/TECHNICAL_OCR_AND_EG79_RIGHTS.md) for
source/license evidence, UNKNOWN permissions, and concrete owner questions.

## Pure-HEIC packaging gates

`-BuildProfile pure-heic` uses only
`--no-default-features --features heic-rs-input` with release/offline/locked and
explicit Windows x64 MSVC target. Default `legacy-native` behavior is retained.

Pure reuse requires an independently pinned authorization hash, a versioned
receipt, original build receipt, actual linker analysis and actual map, source
commit/tree/lock correspondence, exact EXE, exact runtime inventory and EG79
payload. Duplicate/ambiguous JSON and alternate mixed Cargo flags are rejected.
The packager consumes the exact hash-validated runtime snapshot and checks
**staged EXE and every staged runtime file** before executing smoke. Source
replacement between validation/copy cannot silently turn different copied bytes
into the authorized payload.

Independent review found and then confirmed closure of the unchecked early
runtime-snapshot and unchecked staged-EXE gaps. These are new packaging fixes;
they do not change OCR. Unsigned receipts still require a trusted owner review:
the expected hash is not a signature or legal certificate. A normal rebuild
must match prequalified evidence; fresh/different EXEs need new external linker
qualification and authorization. No auto-approval based on Cargo success.

Existing rights, baseline hashes, PE closure, forbidden-path/overwrite,
transactional ZIP and real clean-Windows receipt gates remain enabled. No
distribution status was set to verified, no fake clean receipt was made.

## Checks actually performed

| Check | Result and scope |
|---|---|
| Pure native unit suite | 322 passed, 5 intentionally ignored; existing source/model-selector/grid/geometry/export controls |
| Semantic/export integration contract | 6 passed |
| Focused real PDFium native controls | 3 passed: source-owned glyph boundaries, selected table region, new generated technical page |
| New technical-page fixture | Known NativeText/outside note, independent 4×4 body/title grids, bbox/confidence, JSON and DOCX/XLSX inclusion/omission checked; not scanned-stamp/EG79 accuracy |
| Packager synthetic suite | 105 PASS assertions (50 existing + 55 profile/snapshot/staged-byte checks), exit 0 |
| Pure local directory | Controlled PATH, Cyrillic/space path, complete inventory/hash/PE and accepted two-fixture semantic smoke PASS |
| Full 40-PDF replay in this task | NOT RUN — unchanged exact EXE, models and OCR inputs; prior qualified receipt retained, not relabelled as a new execution |
| Release rebuild | NOT RUN — exact qualified EXE reused |
| Windows Sandbox | NOT RUN — DEFERRED BY OWNER |
| Separate clean Windows x64 | NOT RUN — DEFERRED BY OWNER |

The first new XLSX test assertion incorrectly searched worksheet XML for inline
strings; it was corrected to read shared-string-backed cell values using the
existing workbook reader. Production export code was not changed. Earlier
permission/long-path fixture failures and intermediate logs were retained; the
final complete suite passed after owned-fixture-only environment adjustments.

The accepted PR #8 result remains 40 PDF / 345 pages / 4,071 positions /
36,633 raw and 36,639 reviewed fields, with six known oracle errors preserved.
It is exact-artifact historical acceptance, not a fresh corpus run or universal
accuracy claim. See [its bounded link/regression report](HEIC_PURE_NATIVE_LINK_REPORT.md).

Documentation PR #9 exact-HEAD CI passed:
[push](https://github.com/paulgreenbox-beep/rust-document-ocr/actions/runs/42373763034),
[PR](https://github.com/paulgreenbox-beep/rust-document-ocr/actions/runs/42373795021).
Final packaging-HEAD CI links are recorded in its PR once complete, never
substituted with those older runs. CI is source-level, not native clean-VM proof.

## Local package and preservation

The verified directory is **local only**, 551 files / 54,228,208 bytes. Its
release-manifest SHA-256 is
`7f2381924d4521feef2b4bebc0b69d9d95d99fb03e0efbed254c64fd390d6801`.
It retains exact c6 EXE and EG79 hashes above, distribution status `blocked`,
and records `pure-heic`, one feature in an array, source/build/runtime/link
bindings and separate packaging-time provenance. Recipient quick-start text
now describes technical title-block/table scope and limitations.

Original RC1/working OCR are not replaced. No DB/service/global environment,
model, historical baseline/receipt, PR #5–8 history or existing backup was
modified; no caches were cleaned. Private runtime and authorization receipts,
EXEs/DLLs/PDB/models, customer PDFs and training records remain local. Source
PRs are separate/open; no main merge, tag, public ZIP or GitHub Release.

## Remaining release-owner/compliance blockers

1. Final PDFium retained-component/recipient-notice closure.
2. Publisher's applicable Microsoft CRT redistribution rights.
3. Exact EG79 derived-weight and training-image/label/font permissions, plus
   base attribution/modification notice. Do not solve this by omitting EG79.
4. Applicable HEVC patent review, separate from permissive source licenses.
5. Final modified heic-rs license/NOTICE/modification review.
6. Complete final dependency/notice composition and project contribution rights.
7. Release owner's publication decision. Existing clean-Windows ZIP gate is
   still unmet; owner deferral is not a PASS or FAIL and is not a waiver.

The pure EXE's old HEIF LGPL-chain exclusion is bounded to that artifact, not
legacy RC1/native/mixed builds. No technical test grants distribution rights.
