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

### Accepted synthetic baseline

Supply both the manifest and tree, or omit both for execution-only validation.
The tree is exactly two directories, `text-table` and `raster`, from the two
committed synthetic PDFs. Each must contain `result.json`, `result.v1.json`,
`result.docx` and `result.xlsx`. Keep the manifest and any logs **outside**
that tree. Extra files or directories (including hidden entries), symlinks,
junctions, missing files and modified bytes are rejected.

For ZIP qualification the separately approved UTF-8 JSON manifest has exactly
two fields: integer `schema_version: 1` and a `files` array of eight records.
Each record has exactly `path`, `size` (positive integer bytes) and `sha256`
(64 hex digits). For example, one of the eight records is for
`text-table/result.v1.json`. Use the literal fixture/output names above and
forward slashes; no aliases, traversal or duplicate paths are accepted.
The manifest is independent of the tree's absolute location; a different
directory with different files is not an accepted baseline.

Keep historical manifests unchanged. The known unversioned local RC1 shape
(scope, EXE/runtime hashes, and eight file path/hash records without sizes)
can still be checked for local comparison, but cannot qualify a ZIP. Prepare a
**new** v1 manifest from approved bytes with recorded sizes and have the
release owner approve it. A real clean-Windows receipt must bind its exact
manifest SHA-256, not the old receipt or the directory name.

The packager preflights the file set before building and passes that exact
manifest SHA-256 to both smoke runs. The smoke verifies and copies all eight
files, and only then invokes the comparator on the verified snapshot. The
semantic comparator ignores timings and volatile Office metadata, not text,
cell values, confidence or geometry. Python is a **maintainer comparison
tool**, not an end-user requirement.

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

For Practical RC1, both Windows Sandbox and separate clean Windows validation
are **NOT RUN — DEFERRED BY OWNER**. See the [current RC1 decision](PRACTICAL_RC1.md#current-owner-decision-for-rc1).
This limits the evidence to the existing local validation and does not change
the ZIP gate below.

The generated `verify.ps1` checks payload hashes and unexpected files. It is
not a signature or a license approval. Applications need Windows 10/11 x64
system DLLs/UCRT and the app-local redistributables identified by the PE closure;
they do not need Cargo, Rust, vcpkg, Visual Studio or global PATH edits.

## Pure-HEIC profile (owner-reviewed artifact reuse)

Legacy invocations keep the default `-BuildProfile legacy-native`. The explicit
`-BuildProfile pure-heic` selects this fixed build command, never legacy native:

```powershell
cargo build --release --offline --locked --no-default-features --features heic-rs-input --target x86_64-pc-windows-msvc --bin ocr-pipeline -j 1
```

The pure profile requires a **versioned, owner-reviewed authorization** and its
independently supplied trusted `-ExpectedBuildReceiptSha256`, both for reuse and
for a normal rebuild. It does not mint new static-link evidence automatically.
Use `-SkipBuild` to package the exact already reviewed EXE without recompiling:

```powershell
./scripts/build-windows-portable.ps1 `
  -BuildProfile pure-heic -SkipBuild `
  -BuildReceipt '<absolute-authorized-receipt.json>' `
  -ExpectedBuildReceiptSha256 '<trusted-64-hex-hash>' `
  -RuntimeManifest '<absolute-runtime-inventory.json>' `
  -ApprovedOutputRoot '<absolute-local-validation-root>' `
  -Destination '<fresh-child-directory>'
```

These placeholders are not executable paths or approval receipts. Supply the
accepted baseline pair and maintainer Python as above to run semantic comparison.
Do not derive the expected authorization hash from an arbitrary incoming receipt
and call that authentication. The binding is an unsigned owner attestation, not
a signature, legal grant or proof against a dishonest author of all inputs.

Authorization schema v1 (`receipt_kind=portable-pure-heic-authorization`) binds:

- `build_profile=pure-heic`, source commit/tree, lockfile hash, exactly
  `features=[heic-rs-input]`, no default features, Windows x64 MSVC, release,
  offline and locked;
- absolute `exe_path` and exact `exe_sha256`;
- `runtime_manifest_sha256`, plus the retained EG79 component and actual
  `tessdata/eng_technical_eg79_v1.traineddata` payload hash;
- `original_build_receipt`, `link_evidence`, and `link_map`, each with path/hash.

The existing original receipt is never rewritten. Its source, command, features,
target/profile and artifact hashes must agree. Link evidence must contain
nonempty responses/archives and successful hash-bound symbol scans, no old HEIF
inputs/symbols/map/verbose/import/literal findings, and the heic-rs input. The
actual saved map is hashed too. Nine unavailable temporary Rust objects in the
PR #8 evidence stay a stated limitation, not a complete SBOM claim.

The source commit must exist and be an ancestor. Current Cargo/build/Rust/vendor/
asset inputs must not differ. Only documented packaging/docs/verification paths
and the one standalone technical-capability integration test may change; the
test is not part of `--bin ocr-pipeline`. Unknown changed/untracked inputs fail
closed. JSON is strictly decoded and hash-checked from the same byte snapshot;
duplicate/case-ambiguous keys are rejected. A legacy receipt cannot be relabelled
pure just by adding its EXE hash.

Without `-SkipBuild`, the pure command above runs only after preflight. Its output
must still match the independently reviewed authorization. A fresh/different
binary requires new external linker qualification and a new trusted receipt;
the packager will not approve it merely because Cargo succeeded. This normal
rebuild path is command-contract tested; the current release work reuses the
unchanged qualified EXE rather than claiming a new normal-build experiment.

All previous runtime/hash/PE/baseline/transactional-archive and real clean-Windows
gates remain in force. EG79 is not silently omitted if rights remain unresolved.
Pure HEIC removes neither PDFium/MSVC/model/patent review nor the global
**BLOCKED: LICENSING** state. No public runtime archive is produced by this work.

## ZIP/public release gate

`-CreateZip` requires verified global and per-component rights, a semantic
baseline, and an independently recorded `-CleanEnvironmentReceipt` binding the
exact EXE, input inventory and versioned baseline manifest hashes. The
packager writes a uniquely named provisional archive beside the final ZIP
on the same volume. It checks the archive entries, extracts into a new
diagnostic directory, verifies the full staged file set, sizes and hashes
(including the finalized release manifest), and repeats smoke and semantic
comparison. Only then does it rename the verified archive to the final ZIP
without overwrite and report its SHA-256. Failures retain diagnostic evidence
and do not promote a provisional archive. An existing final ZIP is never
replaced. Without the required rights and receipts it must fail closed. The manual
`.github/workflows/release-windows.yml` operates only on a dedicated,
pre-provisioned `ocr-portable-rc` self-hosted runner and uploads no binaries.
It is not dispatched automatically and creates no public GitHub Release.

Before publication, review the exact PDFium build and bundled third-party
notices, all native/static dependencies, LGPL corresponding-source/relink
requirements, traineddata terms and MSVC redistribution rights. MIT on this
repository is not a blanket license for the executable.

Run the synthetic PowerShell checks with
`pwsh -NoProfile -File scripts/test-windows-portable.ps1`. They exercise the
baseline and ZIP transaction without OCR, a clean-Windows receipt, or a
distributable payload. They are not binary release qualification.

## Licensing references

- [PDFium license](https://pdfium.googlesource.com/pdfium/+/refs/heads/main/LICENSE).
- [Tesseract 5.5.2](https://github.com/tesseract-ocr/tesseract/blob/5.5.2/LICENSE),
  [Leptonica 1.87.0](https://github.com/DanBloomberg/leptonica/blob/1.87.0/leptonica-license.txt).
- [libheif 1.23.1 COPYING](https://github.com/strukturag/libheif/blob/v1.23.1/COPYING),
  [libde265 1.1.1 COPYING](https://github.com/strukturag/libde265/blob/v1.1.1/COPYING).
- [Pinned tessdata_fast license](https://github.com/tesseract-ocr/tessdata_fast/blob/87416418657359cb625c412a48b6e1d6d41c29bd/LICENSE).
- [Microsoft runtime redistribution](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170).
