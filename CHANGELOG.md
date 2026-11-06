# Changelog

## 0.1.0 — Windows practical RC1 (unreleased)

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
- Preserve the native page text while assigning positioned glyphs to confirmed
  grid cells. Recover sparse closed rows and keep adjacent frames separate.
- Preserve cell reading order and suppress blank shifted rules before cell OCR.
  Add synthetic regressions for these cases and custom page sources.

Corrected cell text and grid geometry can differ from older outputs. Model
contents, JSON v1 and DOCX/XLSX serializers are unchanged. No binary GitHub
Release is implied; review the portable and component licensing gates.
