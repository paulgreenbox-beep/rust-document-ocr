# Safe example configuration for the real CLI; this is a PowerShell wrapper,
# not an application JSON configuration format.
$PdfiumLibrary = './local-runtime/pdfium/7881/bin/pdfium.dll'
$TessdataDirectory = './local-runtime/tessdata'
$InputDocument = './examples/documents/synthetic-text-table.pdf'
$NewOutputDirectory = './target/example-run'

cargo run --locked --features native --bin ocr-pipeline -- `
    --input $InputDocument `
    --output-dir $NewOutputDirectory `
    --pdfium-dll $PdfiumLibrary `
    --tessdata-path $TessdataDirectory `
    --languages 'rus+eng' `
    --ocr-dpi 300 `
    --grid-dpi 100 `
    --max-page-workers 4 `
    --enable-cell-ocr-fallback true
