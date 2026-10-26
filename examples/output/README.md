# Recorded synthetic outputs

These JSON files are unmodified copies of `result.v1.json` produced by actual
native CLI runs of this source snapshot, not hand-written expected OCR text.

| Output | Input | Observed mode | Pages | Grids | Cells |
| --- | --- | --- | ---: | ---: | ---: |
| result.v1.json | ../documents/synthetic-text-table.pdf | native_text+grid | 1 | 1 | 9 |
| raster-result.v1.json | ../documents/synthetic-raster.pdf | ocr+grid | 1 | 1 | 9 |

Both runs used `--languages eng --ocr-dpi 300 --grid-dpi 100
--max-page-workers 4 --enable-cell-ocr-fallback true`, an explicitly provided
PDFium API-7881 DLL and official external English language data. The standard
CLI arguments are documented in `docs/OCR_PIPELINE.md` at the repository root.
Each output directory was new. No traineddata or native binaries are included.

The same inputs were processed with the accepted baseline and the prepared
source snapshot. Page text, complete non-timing JSON content and geometry,
legacy JSON, DOCX document XML and XLSX worksheet XML matched. Timing fields
are observed execution measurements and may differ across runs.

These are original synthetic examples for execution and schema inspection.
They do not establish real-world transcription accuracy, general table recall,
or a CER/WER score. DOCX/XLSX and local validation logs are not committed.
