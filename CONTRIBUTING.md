# Contributing

Contributions to Rust Document OCR should keep the local processing boundary,
the versioned JSON contract, and the distinction between OCR output and
reviewed source data clear.

## Development environment

The only validated build target for this release is Windows x64. Use Rust and
Cargo 1.97.1, the MSVC C++ build tools and Windows SDK, and LLVM/libclang 22.1.6
for the native build. The validated vcpkg triplet is
`x64-windows-static-md`; the native dependency set used Tesseract 5.5.2 and
Leptonica 1.87.0.

The following PowerShell example keeps the environment process-local. It
assumes a developer has placed vcpkg and LLVM under a local `.tools` directory;
those tools are not included in the repository. Use equivalent local paths if
your tools are installed elsewhere.

```powershell
$toolRoot = Join-Path $PWD '.tools'
$env:VCPKG_ROOT = Join-Path $toolRoot 'vcpkg'
$env:LIBCLANG_PATH = Join-Path $toolRoot 'llvm/bin'
$env:PATH = "$env:LIBCLANG_PATH;$env:PATH"
$triplet = 'x64-windows-static-md'
$env:VCPKGRS_TRIPLET = $triplet

& (Join-Path $env:VCPKG_ROOT 'vcpkg.exe') install "tesseract:$triplet" "libheif:$triplet"
```

`LIBCLANG_PATH` must point to the directory containing libclang. Configure the
native dependency build for the validated triplet. Do not use `setx` or change
the machine-wide `PATH`.

The locally validated build uses `x64-windows-static-md` for Tesseract,
Leptonica and libheif. A deployment using shared libraries must provide the
corresponding DLLs to its own child process. Native libraries retain their own
terms, including LGPL obligations for libheif/libde265; this repository's MIT
license does not license a resulting native binary solely under MIT. Check the
final executable's complete import and license closure before distribution;
no such binary bundle is published here.

## Build and verification commands

Fetch the locked Rust dependencies once before using offline mode:
`cargo fetch --locked`. Prepare the external native runtime as described in
[INSTALL_WINDOWS.md](docs/INSTALL_WINDOWS.md); the public repository does not
contain it. See [FIXTURE_PROVENANCE.md](docs/FIXTURE_PROVENANCE.md) for the
documented exclusions from the private local integration corpus.

The default feature set builds portable code and tests; it does not perform
real PDF or image OCR. Native processing requires the `native` feature and the
local build dependencies above.

```powershell
cargo fmt --all -- --check
cargo check --offline --locked --all-targets --features native -j 1
cargo test --offline --locked --features native -j 1 -- --test-threads=1
cargo clippy --offline --locked --all-targets --features native -j 1 -- -D warnings
```

These commands check formatting, compilation, the Rust test suite, and lints.
They are not document-accuracy certification. Do not infer broad OCR accuracy
from a successful build or test run. Keep private validation documents and
outputs out of public test fixtures and pull requests.

## Change guidelines

- Preserve the schema-version-1 result envelope and make compatible additions
  additive where possible.
- Keep native text, OCR text, structural table type, and probabilistic semantic
  classification distinct.
- Add deterministic unit or integration tests for behavior changes. Use only
  fixtures that may be redistributed with the project.
- Do not add user documents, OCR outputs, traineddata, model weights, local
  runtime DLLs, credentials, or machine-specific paths.
- Document platform and runtime requirements for changes to native code.
- Do not describe confidence values as proof of transcription correctness.

Project source is licensed under MIT; third-party components retain their own
license terms. See [LICENSE](LICENSE) and
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
