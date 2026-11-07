# RC1 → RC2 progress report

Current decision: **do not promote the HEIC experiment to an OCR replacement
or create a purported RC2 binary package**. This report separates tested source
changes, decoder observations and release approval.

| Stage | Status | Result |
| --- | --- | --- |
| PR #6 review/integration | REVIEWED / INTEGRATED INTO RC BRANCH | Merged only into release/practical-rc1; post-integration 50 PowerShell checks and both CI jobs passed |
| PR #5 source review | FIXED, READY FOR REVIEW | Actual generated-PDF owner displacement reproduced and fixed; PR stays open/draft, no main merge |
| HEIC-RS implementation | ISOLATED EVALUATOR IMPLEMENTED; ADAPTER HELD | Published 0.1.4 tested without changing the OCR dependency/feature configuration |
| HEIC compatibility | PARTIAL FORMAT SUPPORT | Six 8-bit fixtures decode; comparison with Apple PNGs and the old decoder is recorded; required wider corpus is not qualified |
| Actual removal of libheif/libde265 | NOT REMOVED FROM OCR | No replacement full OCR EXE was built; the current native configuration still uses them |
| Source/native regression | PASS at PR #5 scope | 238 source tests, 352 native tests, two further separate-process PDFium tests; no claim of a new 40-PDF binary qualification |
| PDFium licensing | INCOMPLETE | Notice packet/evidence retained; actual retained runtime/font/NOTICE scope and owner approval remain open |
| MSVC redistribution | OWNER/COMPLIANCE DECISION PENDING | File identities confirmed; installed retail files alone do not prove publisher rights |
| EG79 rights | OWNER/COMPLIANCE DECISION PENDING | Base identity/license known; redistribution/training-input rights not inferred |
| RC2 packaging | NOT CREATED | Candidate safety/format qualification incomplete; existing rights/clean-receipt gates stay closed |
| Windows Sandbox | NOT RUN — DEFERRED BY OWNER | No Sandbox operation or fabricated receipt |
| Separate clean Windows x64 | NOT RUN — DEFERRED BY OWNER | No VM operation; developer-host tests do not substitute |

## 1. RC integration and source review

- [PR #6](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/6),
  reviewed head `43bd90d7d653cc3d02d77edc3c0e62a125f23f7f`.
- Integration commit `7d7195bafe6090e82238343763cde7eb5ada5973`;
  Git tree `8618fd7c7c9a60b9130a686ae97055a4954069a7`.
- New integration [CI](https://github.com/paulgreenbox-beep/rust-document-ocr/actions/runs/42347150286)
  passed both source-only jobs; post-merge PowerShell suite exited 0 (50 checks).
  No OCR build was needed for that integration.
- [PR #5](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/5)
  follow-up `55895fc1ae13c6c5a333259be7bcc17fb57d213b`.
  Both [PR CI](https://github.com/paulgreenbox-beep/rust-document-ocr/actions/runs/42348629657)
  and [push CI](https://github.com/paulgreenbox-beep/rust-document-ocr/actions/runs/42348625861)
  passed at that exact head.

The generated-PDF regression showed source cell 0 becoming cell 1 after word
projection: before `["", "A"]` (test exit 1), after `["A", ""]` (exit 0).
Word bbox `[240,284,264,310]` and confidence remained unchanged. An additive,
non-serialized envelope preserves source ownership through both pipeline routes,
assignment and fallback preparation. It maps filtered bbox indices to actual
grid/cell indices, including merged/sparse/multiple-grid cases. Old custom
PageSource hooks retain their geometric fallback. Canonical page NativeText,
Word DTO and JSON/DOCX/XLSX serializer code are unchanged.

Formatting, offline/locked native check and default all-target Clippy passed.
Source tests: 238 passed; native suite: 352 passed; the new owner regression and
existing native-text initializer each passed in a separate process. Three
diagnostic-only tests are not acceptance tests. Initial native attempts lacked
the worktree's expected runtime/control/model paths; the unchanged test inputs
were hash-verified and copied locally, and the exact tests then passed. Initial
failure logs were retained. This is not a clean-machine qualification.

The experimental branch starts from the integrated RC and carries the source
fix as `8c2991556a2e9686e1610fe4207725e8edac5a38`. Only a documentation
cherry-pick conflict needed resolution; the legacy/custom-source duplicate
limitation was preserved. PR #5 was not merged into main or into the RC branch.

## 2. Decoder prerequisite and why no adapter was promoted

See [the isolated evaluator](tools/heic-rs-evaluation/README.md) and its
[evidence](tools/heic-rs-evaluation/evaluation.json). This is a cheap prerequisite
experiment, not a production HEIF refactor.

- Published heic-rs `0.1.4`, non-yanked; package archive 209,012 bytes.
- Registry/archive SHA-256:
  `8883c3bfa6b4ff19a40e52a6dc051c5612cbe21bf408fc6d4875e8dfd777cb66`.
- Packaged VCS identity verified:
  `999470f514c8e9a1a7c37ea44ba70cc32b1308fe`.
- Declared crate MSRV 1.85; isolated PNG test package MSRV 1.88.
  Actual toolchain rustc/Cargo 1.97.1.
- Pure probe uses only std, with default/parallel features disabled.
  An optional native-reference configuration exists solely to compare libheif;
  it is not a pure-Rust full OCR configuration or a distribution artifact.
- Production defaults, including four page workers, and the root Cargo
  manifest/lockfile were not changed by the HEIC evaluator.

A safe small malformed-dimension case declares container ispe 16x16 while HEVC
SPS remains 64x64. With max_pixels=256 and strict mode, the published decoder
later reports `PixelLimit { pixels: 4096, max_pixels: 256 }`. Matched published
source allocates raw SPS-sized planes before the colour-conversion limit.
This is runtime observation plus exact source tracing, not a peak-memory/OOM
measurement. The public SPS probe reports cropped dimensions and does not
provide the full raw allocation boundary needed for a general guard.

The six well-formed 8-bit examples include a 2x2 grid. Against the upstream
Apple-decoded reference PNGs, pure-Rust MAE / maximum channel difference is:

| Synthetic image | Mean absolute error | Maximum channel difference |
| --- | ---: | ---: |
| White 16 | 0 | 0 |
| Checker 64 | 0 | 0 |
| Checker grid 1024 | 0 | 0 |
| Flat 64 | 0.669515 | 2 |
| RGB strips | 0.784722 | 2 |
| Gradient 512 | 0.700551 | 5 |

The old libheif 1.23.1 reference decodes five of six and rejects white-16 under
unchanged native security limits. Candidate/native RGB-strip maximum difference
is 188 despite MAE 1.979167. Such colour differences are retained, not normalized
away or called an OCR quality improvement. The forged-size native decode also
rejects under its security limit. These observations do **not** prove a
relative memory/quality regression versus libheif.

10-bit, alpha, true irot/imir metadata, sequences, overlay and fine-text OCR
holdout remain unqualified. AVIF rejection was observed. The upstream
rotated-named fixture is not nonzero rotation-metadata proof.

Disposition: **HEIC-RS — PARTIAL FORMAT SUPPORT; NOT QUALIFIED AS A DROP-IN
REPLACEMENT**. The preallocation prerequisite remains on hold. Do not silently
add a native fallback or patch a private parser to bypass it.

Therefore no production adapter, full OCR release EXE, linker map, PE removal
proof, 40-PDF/345-page candidate rerun or RC2 portable package was fabricated.
The old RC1 receipts are not used as new-EXE evidence. Probe/test executable
hashes and selected dependency graphs belong to the evaluator receipts, not to
a claimed replacement ocr-pipeline.exe.

## 3. License and environment decisions

[Current review](docs/compliance/HEIC_RS_EXPERIMENT_REVIEW.md) records exact CRT
file hashes, pinned source/license links and the remaining owner decisions.
The previous PDFium notice and HEIF LGPL packets remain applicable. No
distribution_status was changed to verified.

A permissive decoder source license does not itself resolve third-party HEVC
patents. The upstream NOTICE and release-owner/legal review remain necessary.
EG79 exact base bytes and Apache license do not establish rights in training
inputs or an affirmative grant for modified weights. Microsoft REDIST
technical provenance does not alone establish publisher eligibility.

**WINDOWS PORTABLE RELEASE — BLOCKED: LICENSING**.

Both deferred Windows environments retain the exact owner status above; no
license gate or actual clean-Windows receipt requirement is weakened.

## 4. Preservation and next safe action

The original RC1 release EXE SHA-256 remains
`c305d60ec3861ebb5cf4e9532f346559be4ebc1b5989da31412c1c36f71f8cb4`.
Its original 546-file staged package, manifest and historical receipts remain
unchanged. No working ONLYOFFICE OCR, DB, service, retained backup, models or
PDFium DLL were modified. No public ZIP, tag, release or main merge was made.

Next: review the source ownership fix; obtain a decoder with a tested limit
before raw SPS/plane allocation (including conformance cropping and aggregate
tiles), complete the missing format/text corpus, then implement the opt-in
adapter and build a separately receipted full EXE with actual linker/PE
verification. If that succeeds, run limited synthetic PDF/table checks and
then the available 40-PDF control set. Only afterwards reconsider RC2 and the
specific new configuration's LGPL applicability. Keep the existing RC1 intact.
