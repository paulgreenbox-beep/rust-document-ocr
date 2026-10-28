# OCR pipeline and public API

The Cargo package is `ocr-pipeline` version `0.1.0`. Real document processing
requires the `native` feature. The default feature set is empty and is useful
for portable compilation and tests, but it does not perform real OCR.

The validated release target is Windows x64. Other operating systems are not
verified by this release.

## Input formats and native dependencies

With `native`, the pipeline supports PDF, JPEG, PNG, BMP, TIFF, WebP, HEIF, and
DjVu inputs. Dispatch is based on the content signature rather than the file
extension. PDF input uses PDFium API revision 7881. Raster and scanned content
uses Tesseract on the CPU.

Provide PDFium when processing PDFs, Tesseract traineddata for the requested
languages, and the required native runtime libraries. No executable, native
DLL, traineddata, or model weights are distributed in this source package.
The optional technical-cell model and Chinese language data must be obtained
and licensed separately if a deployment uses them.

## CLI

The CLI writes into a new output directory and does not overwrite an existing
one.

| Argument | Default | Purpose |
| --- | --- | --- |
| `--input <PATH>` | required | Input PDF, image, TIFF, HEIF, or DjVu document. |
| `--output-dir <PATH>` | required | New directory for the generated outputs. |
| `--pdfium-dll <PATH>` | resolved from the documented local runtime layout | Explicit PDFium DLL path; used for PDF input. |
| `--ocr-dpi <DPI>` | `300` | Full-resolution render density for OCR pages. `--dpi` is an alias. |
| `--grid-dpi <DPI>` | `100` | Initial render density for table-grid detection. |
| `--languages <LIST>` | `rus+eng` | Tesseract language codes joined with `+`. `--language` is an alias. |
| `--language-routing <MODE>` | `configured` | Per-page language route: `configured`, `simplified-chinese`, or `auto-chinese`. |
| `--tessdata-path <PATH>` | process-local `TESSDATA_PREFIX`, then local runtime lookup | Directory containing the requested `*.traineddata` files. |
| `--page-limit <N>` | no limit | Optional page limit for a bounded run. |
| `--max-page-workers <N>` | `4` | Bounded CPU page-worker count. `--parallelism` is an alias. |
| `--enable-cell-ocr-fallback <BOOL>` | `true` | OCR visually non-empty grid cells that lack positioned native text. |
| `--progress-jsonl <PATH>` | disabled | Write schema-v1 progress events to a separate JSON Lines file. |

The configured language route keeps the requested language string, normally
`rus+eng`. `simplified-chinese` explicitly selects `chi_sim+eng` for pages.
`auto-chinese` switches only when reliable evidence is found in an existing
PDF text layer; it does not probe raster-only pages. Raster scans that need
Chinese recognition require explicit language selection and the corresponding
external traineddata.

## Output files

The CLI writes:

- `result.v1.json`: schema-version-1 result envelope, including tables,
  processing diagnostics, warnings, and additive semantic fields;
- `result.json`: compatibility JSON document;
- `result.docx`: text and supported title-block projection;
- `result.xlsx`: detected table worksheets.

Optional typed cell-failure `diagnostics` are separate from the frozen v1
`warnings` enum. See [JSON v1 compatibility](JSON_V1_COMPATIBILITY.md) for
old-reader behavior and the distinct Rust source-compatibility boundary.

Table structure, recognized text, and inferred meaning remain separate in the
JSON. Confidence and page-quality fields are diagnostic signals; they are not
an accuracy guarantee.

The repository's public example pairs a
[synthetic input document](../examples/documents/synthetic-text-table.pdf) with
its [recorded JSON output](../examples/output/result.v1.json).

## Rust library API

The crate exports the full-document API:

```rust
pub fn process_document(
    input: &std::path::Path,
    options: &OcrOptions,
) -> Result<OcrResult, OcrError>;
```

`process_document_with_progress` accepts an `OcrProgressReporter`. Export
helpers write the versioned JSON, compatibility JSON, DOCX, and XLSX formats.
There is no HTTP or RPC API.

The separate `process_region` library API accepts a PDF, `RegionRequest`, and
`OcrOptions`. Region rectangles use normalized top-left page coordinates.
Its `Text`, `Auto`, and `Table` modes process one selected page region and
return a `RegionResult`; they do not invoke full-document exporters.

## Table classification

Structural `kind` describes table layout (`data_table`, `title_block`, or
`unknown`). Probabilistic `semantic_type` describes likely subject matter.
Classification is local and rule-based, using headers, column value patterns,
geometry, and detector/OCR confidence. Low or conflicting evidence remains
generic or unknown. The classifier does not alter OCR text or infer missing
cell values. See [EXPERIMENTAL_FEATURES.md](EXPERIMENTAL_FEATURES.md) for
limitations.
