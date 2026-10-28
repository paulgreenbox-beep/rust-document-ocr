# Windows x64 portable candidate

Version is read from `Cargo.toml` (currently 0.1.0), not renamed to 1.0.0.
This work adds a **release-candidate gate**, not an automatic binary release.
The current local inventory is blocked for redistribution; see the measured
[release report](../reports/WINDOWS_PORTABLE_RELEASE_V1.md). Do not distribute
a validation directory merely because its OCR smoke passes.

## Runtime discovery

PDFium uses an explicit API/CLI override first, otherwise `pdfium.dll` or the
versioned runtime path beside the executable, then the historical development
fallback. It never searches global PATH for PDFium. The portable
`release-manifest.json` marker disables development fallback when its own DLL
is missing: a package must not silently borrow the developer's installation.

Tessdata precedence is:

1. Explicit `--tessdata-path` / library option.
2. Process-local `TESSDATA_PREFIX`.
3. `tessdata` beside the executable.
4. Historical development fallback, unless the portable marker is present.

An invalid explicit path or environment override fails clearly; it is not
silently ignored. Requested traineddata must exist. On Windows, a non-ASCII
path is mirrored byte-for-byte into an exclusively created ASCII temporary
directory owned by that cached Tesseract API. Native handles are dropped before
their temporary files. The implementation never alters global environment or
the user's original models. If TEMP is non-ASCII and Windows supplies no ASCII
short-path alias, an actionable process-local TEMP error is returned; this
unusual profile configuration is a stated limitation, not a silent fallback.

## Maintainer prerequisites

Use the existing documented Windows x64 MSVC/LLVM/vcpkg build environment and
offline locked Cargo cache. No runtime is downloaded by the release scripts.
Use `x64-windows-static-md`; do not assume it means every dependency is a DLL.
The script inventories normal **and delay-load PE imports**, plus the explicit
dynamic PDFium dependency. Static library inputs require separate records.

Prepare a private `runtime-manifest.local.json` with schema version 1:

- `distribution_status`: `verified` only after an actual rights review;
- `components`: exact version/source/license/review, files with absolute local
  `source`, safe portable `path`, SHA-256 and packaged notice paths;
- `static_components`: version, source/artifact SHA-256, license, review and
  notices for Rust and native static inputs. An empty inventory is rejected.

`scripts/collect-portable-inventory.py` is an optional maintainer-only Python
3.11+ helper. It records local inputs and notices conservatively as **blocked**;
it does not grant redistribution permission. Check its `--help`. A Cargo.lock
record or vcpkg port is not proof of retained linker members or legal clearance.
The public package manifest strips local input file paths.

## Build a local validation directory

```powershell
./scripts/build-windows-portable.ps1 `
  -RuntimeManifest 'C:\private-release-inputs\runtime-manifest.local.json' `
  -ApprovedOutputRoot 'C:\ocr-release-validation' `
  -Destination 'C:\ocr-release-validation\rust-document-ocr-v0.1.0-windows-x64' `
  -BaselineManifest 'C:\accepted-control\baseline-manifest.json' `
  -BaselineRoot 'C:\accepted-control\outputs' `
  -PythonPath 'C:\maintainer-python\python.exe'
```

Paths are examples, not bundled inputs. The destination must be new, outside
the checkout and strictly below the supplied approved root. The default build
is release/native/offline/locked. Reuse of an existing EXE requires a hash-bound
`-BuildReceipt` with `-SkipBuild`; an arbitrary EXE is not accepted as a native
build. No installed OCR service is stopped or replaced.

The accepted baseline tree must contain `text-table` and `raster` outputs from
the two committed synthetic PDFs. The semantic comparator ignores timings and
volatile Office metadata, not text, cell values, confidence or geometry.
Python is a **maintainer comparison tool**, not an end-user requirement.

## Smoke and package verification

The smoke script copies the candidate into a new Cyrillic/spaced temporary
location, uses an unrelated working directory and a process-local PATH limited
to Windows system paths, clears OCR overrides for that child, and runs both
native-text and actual raster OCR examples. It checks tables and all four
outputs, missing runtime/data cases, and baseline compatibility. Each native
child has bounded execution. Its evidence is retained, not silently discarded.

This controlled-PATH check is **not** a clean Windows VM. The developer machine
may have system-installed runtimes. A real clean Windows x64 receipt remains a
separate publication prerequisite.

The generated `verify.ps1` checks payload hashes and unexpected files. It is
not a signature or a license approval. Applications need Windows 10/11 x64
system DLLs/UCRT and the app-local redistributables identified by the PE closure;
they do not need Cargo, Rust, vcpkg, Visual Studio or global PATH edits.

## ZIP/public release gate

`-CreateZip` requires verified global and per-component rights, a semantic
baseline, and an independently recorded `-CleanEnvironmentReceipt` binding the
exact EXE, input inventory and baseline hashes. The script extracts and smokes
the ZIP again. Without these conditions it must fail closed. The manual
`.github/workflows/release-windows.yml` operates only on a dedicated,
pre-provisioned `ocr-portable-rc` self-hosted runner and uploads no binaries.
It is not dispatched automatically and creates no public GitHub Release.

Before publication, review the exact PDFium build and bundled third-party
notices, all native/static dependencies, LGPL corresponding-source/relink
requirements, traineddata terms and MSVC redistribution rights. MIT on this
repository is not a blanket license for the executable.

## Licensing references

- [PDFium license](https://pdfium.googlesource.com/pdfium/+/refs/heads/main/LICENSE).
- [Tesseract 5.5.2](https://github.com/tesseract-ocr/tesseract/blob/5.5.2/LICENSE),
  [Leptonica 1.87.0](https://github.com/DanBloomberg/leptonica/blob/1.87.0/leptonica-license.txt).
- [libheif 1.23.1 COPYING](https://github.com/strukturag/libheif/blob/v1.23.1/COPYING),
  [libde265 1.1.1 COPYING](https://github.com/strukturag/libde265/blob/v1.1.1/COPYING).
- [Pinned tessdata_fast license](https://github.com/tesseract-ocr/tessdata_fast/blob/87416418657359cb625c412a48b6e1d6d41c29bd/LICENSE).
- [Microsoft runtime redistribution](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170).
