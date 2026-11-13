# Editable Word export

The existing `result.docx`, `result.v1.json`, legacy JSON and XLSX exports remain unchanged. Build with `editable-word` plus your existing native profile and opt in at runtime:

```powershell
cargo build --locked --offline --features heic-rs-input,editable-word --bin ocr-pipeline
ocr-pipeline --input input.pdf --output-dir new-output --editable-word
```

The opt-in additionally writes `result.editable.docx` and `result.word-report.json`. The report includes graphics/geometry limitations. The Rust APIs are `export_word::export_word` (an existing OCR result and optional page graphics) and, in a native build, `export_input_word` / `export_pdf_word`. OCR classification, NativeText, Tesseract, EG79, and JSON/XLSX schemas are unchanged. The new feature is not part of the accepted portable profile.

## What Word receives

- Full page text, including NativeText and table-classified pages, as editable paragraphs. Original table-related text can also appear in editable tables; the exporter intentionally does not remove it from the complete text projection.
- Data tables and optionally title blocks as `w:tbl`. Valid, non-overlapping spans whose structured projection matches the source matrix become Word merges. Ambiguous spans stay in an editable matrix and produce a report warning instead of discarding text. Table placement is in flow; unsupported precise text/column positioning is not inferred.
- PDF graphics use displayed crop coordinates in points, mapped to Word anchors and page dimensions. Simple recoverable solid paths have an SVG part and a local PNG fallback. Bounded dashed paths are rasterized individually to transparent PNG. SVG is picture content, not an editable Word drawing. Reader support varies. The report distinguishes media and geometry limitations; no page screenshot replaces text or tables.
- Supported embedded image objects keep their proportions and color. Eligible original JPEG can be copied without re-encoding; cases requiring pixel transforms are PNG. Unsupported or unsafe clip/compositing/transform cases can be skipped with an explicit warning. Complex drawings, overlapping layers and exact pagination are not guaranteed; Word reflows editable content.
- Whole-page scan imagery is not overlaid on OCR text by default. `--word-scan-reference` can append a separate inline reference for a bounded single-image JPEG/PNG/BMP/WebP source. Embedded PDF page backgrounds and multi-frame formats are not enabled by this option.

## Optional Word 97–2003

Pass `--word-doc-converter <absolute-path-to-trusted-soffice>` together with `--editable-word`. The converter is not discovered or installed, and is not bundled. It runs headless with an isolated user profile and the `MS Word 97` filter. Output is accepted only when it is a compound binary Word file with its `WordDocument` and referenced table stream; an extension alone is not accepted. Failed conversion does not publish the staging output as a finished job.

Legacy Word can change layout and convert vector graphics to other formats. Binary validation does not prove semantic round-trip fidelity; verify `.doc` separately with the chosen converter and reader before depending on preservation of merged cells or SVG.
