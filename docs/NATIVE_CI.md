# Opt-in native Windows CI

`.github/workflows/native-windows.yml` is a manual integration gate for a
dedicated, pre-provisioned Windows x64 self-hosted runner. It is separate from
the public source-only workflow and can be dispatched only from the repository
default branch. It does not install tools, fetch Rust crates, download PDFium
or traineddata, or upload build outputs or OCR results.

## Provision the runner

Use a dedicated runner labeled `self-hosted`, `Windows`, `X64`, and
`ocr-native`. Restrict who can dispatch the workflow, keep the runner isolated
from unrelated jobs, and do not expose secrets to its job environment.

Pre-provision the runner through your trusted internal process with:

- Rust/Cargo 1.97.1, the MSVC C++ build tools and Windows SDK, and LLVM 22.1.6
  with `libclang.dll`.
- vcpkg and its `x64-windows-static-md` installed packages for Tesseract and
  libheif. Set `VCPKG_ROOT` and `LIBCLANG_PATH` in the runner service
  environment; the workflow checks the toolchain and installed package state.
- A compatible x64 PDFium DLL for the `pdfium_7881` binding, supplied through
  `OCR_NATIVE_PDFIUM_DLL`.
- A tessdata directory containing at least `eng.traineddata` and
  `rus.traineddata`, supplied through `OCR_NATIVE_TESSDATA_DIR`.
- A complete offline Cargo cache for the locked dependencies. Missing crates
  fail the offline check; the workflow will not fetch them.

Do not put the DLL or language models in Git or a workflow artifact. During
preflight, the workflow temporarily copies the runner-provided DLL and the
committed synthetic text-table PDF into the checkout's ignored `runtime/`
test layout because a few direct PDFium binding tests use that legacy path.
Environment variables point the rest of the native tests and the CLI smoke
test at the explicit runner-provided runtime. The checkout-local copy is not
committed or uploaded.

## Run and interpret it

From the repository default branch, select **Native Windows OCR (manual)** and
run `workflow_dispatch`. The job fails before compilation if required runner
settings, tools, native packages, traineddata, or PDFium are missing. It then
runs the native feature check, serialized native test suite, and a one-page
CLI smoke run on the committed `examples/documents/synthetic-raster.pdf`; the
smoke output is written under the runner's temporary directory.

This gate proves that the provisioned Windows native build and a bounded
synthetic PDF-to-OCR path execute together. It is not a general OCR accuracy
benchmark, does not validate customer documents, and does not certify a
redistributable native bundle or third-party license closure. The regular
`.github/workflows/ci.yml` intentionally remains source-only: its empty default
feature set does not load PDFium, Tesseract, Leptonica, or language models.
