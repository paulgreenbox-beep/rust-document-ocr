# Windows x64 installation and native tests

This source release was checked on Windows x64 with Rust/Cargo 1.97.1,
MSVC, LLVM/libclang 22.1.6, Tesseract 5.5.2 and Leptonica 1.87.0. Other
operating systems are not validated by this release. Use a compatible x64
PDFium DLL for the Cargo binding feature `pdfium_7881`.

## Build dependencies

Install the MSVC C++ build tools and Windows SDK, Rust, LLVM/libclang and
vcpkg using their own trusted installation channels. No installer is bundled.
Keep the environment process-local as shown in CONTRIBUTING. Install the
Tesseract and libheif build dependencies for the validated
`x64-windows-static-md` triplet. This also brings their dependent libraries.
The first Rust dependency download is `cargo fetch --locked`; offline checks
require that cache to exist already.

## PDFium and language data

PDFium and its complete third-party notices remain external. Obtain a compatible
build from a source you trust; the [PDFium project](https://pdfium.googlesource.com/pdfium/)
and [pdfium-render](https://github.com/ajrcarey/pdfium-render) describe the engine
and bindings. A binary package needs a separate architecture/version/hash and
license-closure review. Do not copy arbitrary DLLs into Windows system folders.

Tesseract needs language data in a directory passed with `--tessdata-path` or
process-local `TESSDATA_PREFIX`. Normal `rus+eng` needs both `rus.traineddata`
and `eng.traineddata`. `chi_sim+eng` additionally needs Chinese data.
Use official Tesseract data and retain its Apache-2.0 license; weights are not
included in this repository. Missing required files cause an explicit error.

The local eng/rus files used in validation were matched by size and Git-blob SHA
to official [tessdata_fast](https://github.com/tesseract-ocr/tessdata_fast/tree/87416418657359cb625c412a48b6e1d6d41c29bd)
commit `87416418657359cb625c412a48b6e1d6d41c29bd`:

| File | SHA-256 | Official Git blob SHA-1 |
| --- | --- | --- |
| eng.traineddata | `7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2` | `bbef4675053b5b468cdb477053e28b1c698ba08e` |
| rus.traineddata | `e16e5e036cce1d9ec2b00063cf8b54472625b9e14d893a169e2b0dedeb4df225` | `b146cb2263acbc6383f8e92ea0ce759537687bb8` |

The tested external PDFium DLL fingerprint is
`79d4676b656cfb1abcea88f9ade3b4b0826c5200382db5f4ec72a636c598c118`.
This fingerprint identifies the local validation input; it is not a permission
to redistribute PDFium or a substitute for verifying its third-party closure.

## Populate the ignored local test layout

The existing render tests expect a local `runtime/` layout. The helper below
copies only one separately obtained PDFium DLL, two stock language files and
the repository's own synthetic PDF. It refuses overwrite, verifies the stated
hashes and checks x64 PE architecture. The historical `w3c-dummy.pdf` test name
is populated from the new synthetic fixture, not an external document.

```powershell
./scripts/prepare-test-runtime.ps1 `
  -PdfiumLibrary './local-runtime/pdfium/7881/bin/pdfium.dll' `
  -TessdataDirectory './local-runtime/tessdata'

$env:OMP_THREAD_LIMIT = '1'
cargo test --offline --locked --features native -j 1 -- --test-threads=1
cargo test --offline --locked --features native --lib `
  native::pdfium_source::tests::extracts_native_text_from_synthetic_control_pdf `
  -- --ignored --exact --test-threads=1
```

If deliberately using a different verified compatible build, supply its trusted
expected hash via `-ExpectedPdfiumSha256` (and corresponding language hash
parameters if needed). Do not replace an expected hash merely to silence an
unexplained mismatch. `runtime/` stays ignored and must never be committed.

## Run a public example

```powershell
cargo run --locked --features native --bin ocr-pipeline -- `
  --input './examples/documents/synthetic-text-table.pdf' `
  --output-dir './target/new-ocr-output' `
  --pdfium-dll './runtime/pdfium/7881/bin/pdfium.dll' `
  --tessdata-path './runtime/tessdata' `
  --languages eng
```

The output directory must be new and its parent must exist. For the image-only
OCR path, replace the input with `examples/documents/synthetic-raster.pdf` and
choose another new output directory. Both examples are synthetic, not a
document-accuracy benchmark.
