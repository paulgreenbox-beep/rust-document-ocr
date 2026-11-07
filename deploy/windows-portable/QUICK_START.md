# Windows 10/11 x64 portable

Extract the entire folder to a writable location. Do not run inside the ZIP.
In PowerShell, from that folder:

```powershell
.\verify.ps1
.\ocr-pipeline.exe --input .\examples\synthetic-text-table.pdf --output-dir "$env:TEMP\ocr-first-result"
```

The output directory must not already exist; its parent must exist. Outputs include
`result.json`, `result.v1.json`, `result.docx`, and `result.xlsx`.
PDFium and native DLLs belong beside the executable. English and Russian models
belong in `tessdata` beside it. No Cargo, Rust, vcpkg or global PATH changes are needed.
An explicitly set `TESSDATA_PREFIX` takes precedence over the portable models;
remove that variable for the current session if it points to an unrelated install.
Page OCR uses the requested languages (for example `--languages rus+eng`). An
optional `eng_technical_eg79_v1.traineddata` in that same tessdata directory can
be used for selected technical cells; it is not a page-language replacement.
The source repository does not supply or authorize distribution of that model.
The package manifest records licensing decisions separately for each component;
the whole package is not MIT-only.

## Technical documents, title blocks and tables

The Rust pipeline adds document/region analysis, ruled-grid reconstruction,
positioned-word assignment and technical-cell selection beyond Tesseract text
recognition. Supported drawing title blocks are compact lower-right ruled
regions identified by geometry and limited label/occupancy rules. They are not
arbitrary stamps, signatures, certified GOST forms or guaranteed extracted
requisites. Native page text remains separate from cell text.

Data tables export to XLSX `Page_*` sheets; title blocks use separate
`TitleBlock_*` sheets or can be omitted by the library export option without
removing JSON data. DOCX is a text/table projection, not a facsimile drawing.
Incomplete lines, complex merged cells, irregular borderless layouts, small
symbols and technical-code OCR remain review points. Geometry/confidence in
JSON are diagnostic signals, not correctness guarantees.

The proposed full pure-HEIC profile retains EG79 for selected technical cells;
it is not the title-block detector or a replacement for page languages. The
source repository contains no weights or training data and grants no automatic
right to redistribute them. Runtime rights, notices, model/input rights and
HEVC patent review remain separate from functional smoke-test success.

Windows supplies the system UCRT; non-system VC runtime DLLs must be included
app-locally with their reviewed redistribution terms. Maintainer smoke scripts
require PowerShell 7; the OCR executable itself does not require PowerShell.
