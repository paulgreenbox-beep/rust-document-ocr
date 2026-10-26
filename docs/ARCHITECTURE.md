# Architecture

Rust Document OCR is a local CPU pipeline exposed through the
`ocr-pipeline` Rust library and CLI. There is no network service layer.

## Main path

1. The input dispatcher identifies supported formats from their content
   signature.
2. For PDFs, PDFium opens the document and exposes its text layer and page
   rendering. Other supported inputs use their local image, TIFF, HEIF, WebP,
   or DjVu adapters.
3. Page analysis assesses available native text and chooses the processing
   route. Usable PDF text is retained as page text; grid geometry does not
   replace it.
4. Raster preparation and Tesseract run where OCR is needed. A bounded pool
   processes page CPU/OCR work, with four workers by default. PDFium access
   remains serialized.
5. Table detection uses ruled-grid geometry and, in the configured fallback
   mode, repeated alignment of positioned words. Recognized words are assigned
   to cells without changing the source text.
6. The library returns an `OcrResult`; the CLI writes JSON, DOCX, and XLSX
   outputs as one staged output transaction.

The `native` Cargo feature enables the real PDFium/Tesseract pipeline and the
local format adapters. The default Cargo feature set is empty and does not
process real documents.

## Library boundaries

The host-facing full-document API is
`process_document(&Path, &OcrOptions) -> Result<OcrResult, OcrError>`. A
progress-aware variant reports structured events without OCR text.

`process_region` is a separate in-process API for one rectangle on one PDF
page. It accepts normalized, top-left page coordinates and supports text,
automatic, and table modes. It does not create document-level DOCX or XLSX
exports, and it is not exposed as a CLI endpoint.

PDF rendering, OCR, page analysis, table detection, result models, and exporters
are separated behind Rust modules and traits. The JSON result envelope is
schema version 1; the Cargo crate version is 0.1.0.

## Table structure and meaning

Structural table `kind` values are `data_table`, `title_block`, and
`unknown`. A separate local rule-based classifier may add a probable
`semantic_type`, header rows, column roles, confidence scores, and
classification evidence. It combines header aliases, value patterns, table
geometry, and detector/OCR confidence. This classification does not rewrite
recognized text or fill empty cells.

Cell, grid, page-quality, and semantic confidence fields support diagnostics
and downstream review. They are internal signals, not guarantees of correct
transcription or document-wide accuracy.

## Runtime boundary

The validated target is Windows x64. A PDF run needs a PDFium DLL matching API
revision 7881. OCR needs the Tesseract runtime and traineddata for each
requested language. HEIF processing also needs its native codec runtime.
These binaries and language/model files are not part of this source package.

The engine is CPU-only. It has no GPU backend, multi-document service queue,
cloud OCR provider, or hosted API endpoint.
