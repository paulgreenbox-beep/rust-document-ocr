# HEIC research outcome — no RC2 promoted

This is an experimental source delivery in [draft PR #7](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/7),
not release notes for a distributable Windows RC2.

- PR #6's fail-closed packaging fixes were reviewed and merged only into the
  RC branch. Rights, verified baseline, integrity and real clean-receipt gates
  remain in place.
- The confirmed source-ownership projection defect was fixed in PR #5, with
  generated-PDF and compatibility regressions. Canonical NativeText, Word
  geometry/confidence and JSON/DOCX/XLSX serializer contracts remain intact.
- Published HEIC-RS 0.1.4 was evaluated in a separate pinned test package. It
  decodes six 8-bit synthetic examples, but complete format/OCR equivalence
  and a caller-bounded raw decoder allocation route are not qualified.
- Pixel differences, the native decoder's white16 error and the small
  metadata/SPS mismatch are retained in the evidence. Relative regression
  against the old decoder is not established.
- No production HEIF adapter or feature graph change, full OCR release EXE,
  full-app LGPL removal proof, 40-PDF candidate qualification or RC2 package
  is claimed. The old RC1 receipts are not reused for a new executable.

Windows Sandbox: **NOT RUN — DEFERRED BY OWNER**.
Separate clean Windows x64: **NOT RUN — DEFERRED BY OWNER**.
No new receipt or PASS/FAIL result is assigned to either environment.

**WINDOWS PORTABLE RELEASE — BLOCKED: LICENSING**. PDFium notice closure,
applicable static HEIF LGPL obligations, MSVC publisher eligibility, EG79
rights and HEVC patent review remain separate decisions. No main merge,
tag, public runtime ZIP or GitHub Release was made.

Details and exact source/evidence references:
[RC1_TO_RC2_PROGRESS_REPORT.md](RC1_TO_RC2_PROGRESS_REPORT.md).
