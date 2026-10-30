# Changelog

## 0.1.0 — Windows portable release candidate (unreleased)

- Add offline Windows x64 native release packaging, PE import-closure inspection,
  hash manifests, local smoke and an opt-in pre-provisioned runner workflow.
- Resolve tessdata from an explicit path, process environment, EXE-adjacent
  directory, then development fallback; invalid overrides fail clearly.
- Handle Windows Cyrillic model paths using API-owned temporary ASCII copies
  with unchanged traineddata bytes and no global environment changes.
- Prevent marked portable layouts from borrowing missing PDFium/tessdata from
  the compiled-in development tree.
- Require license reviews, accepted semantic regression controls and genuine
  clean-Windows evidence before ZIP/publication acceptance.

OCR routing, thresholds, segmentation, table geometry, model contents, JSON v1
and DOCX/XLSX contracts are unchanged. No binary GitHub Release is implied by
this changelog; see the release report for actual gates and blockers.
