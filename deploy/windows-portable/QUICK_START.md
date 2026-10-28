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
The package manifest records licensing decisions separately for each component;
the whole package is not MIT-only.
Windows supplies the system UCRT; non-system VC runtime DLLs must be included
app-locally with their reviewed redistribution terms. Maintainer smoke scripts
require PowerShell 7; the OCR executable itself does not require PowerShell.
