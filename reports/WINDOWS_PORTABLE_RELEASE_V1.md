# WINDOWS PORTABLE RELEASE — BLOCKED: LICENSING

Date: 2026-10-28. PR: <https://github.com/paulgreenbox-beep/rust-document-ocr/pull/2>.
Repository: `paulgreenbox-beep/rust-document-ocr`.
Branch: `release/windows-portable-v1`; no automatic merge or public Release.

## Result and source identity

Working Windows x64 release/native EXE and executable candidate infrastructure
were prepared and tested. This is **not a cleared redistributable binary**.
No ZIP was created, and no DLL, EXE, traineddata, private PDF or generated OCR
document was uploaded to GitHub.

- Starting main/merged PR #1: `9a99b37102c7bb21c9f358bafd69f95f19a626f2`.
- Runtime discovery commit: `3a2e1b5`.
- Packaging code commit: `f5a35a86899db35911fb94f63380cb7ff1d17746`.
- Inventory/test correction: `e4d28545e70b351eb4b02ff6ae0839ee4951433d`.
- Reviewed-fixture checksum gate: `921c5777855df1b1ea91d2bdf0872e5ace0dfdd5`.
- A documentation/manifest commit follows these source commits; the final PR
  tip is available in PR #2. No changes to main are made by this task.
- Cargo/CLI version: **0.1.0**. No arbitrary 1.0.0 version was invented.
- Final EXE: **20,121,088 bytes**, SHA-256
  `72c1b7880595c04503f25692d76eed2b57cce5727c8fe811b15dac5abf763665`.

The optimized EXE was rebuilt through the packaging script at a clean source
commit. The final complete-inventory layout reuses that exact EXE with a
hash-bound build receipt; its packaging-time source snapshot has clean HEAD
`921c577...`. The intervening inventory and packaging safety fixes change
maintainer tooling, not any Rust source used to build that EXE.

## Deliverables

- `scripts/build-windows-portable.ps1`: offline locked native build, pinned
  input verification, safe new destination, component/notice review, PE closure,
  manifest, smoke, and fail-closed ZIP gate. Only the two individually
  checksum-pinned public fixtures are copied; arbitrary PDF globs are not used.
- `scripts/smoke-windows-portable.ps1`: bounded native child processes, separate
  unrelated CWD, Cyrillic/spaced EXE/runtime/input/output paths, controlled PATH,
  actual OCR/table/export checks and semantic baseline comparison.
- `scripts/portable-pe.ps1`: Windows x64 normal and delay import inspection.
- `scripts/test-windows-portable.ps1`: runtime-free synthetic/source checks.
- `scripts/collect-portable-inventory.py`, `scripts/test_portable_inventory.py`:
  offline local evidence collection and three tests for lockfile completeness
  and standard dual-license notice filenames. Maintainer Python 3.11+ only.
- `.github/workflows/release-windows.yml`: manual, pre-provisioned runner gate;
  no downloads of runtimes, binary uploads, or automatic Release creation.
- `deploy/windows-portable/`: `verify.ps1`, QUICK_START and TROUBLESHOOTING.
- `docs/PORTABLE_WINDOWS.md`, `docs/RELEASE_TROUBLESHOOTING.md`, CHANGELOG and
  README reference.
- [Full sanitized candidate manifest](WINDOWS_PORTABLE_CANDIDATE_MANIFEST.json).

The public manifest lists **535 payload files**, exact size/SHA-256 per file,
the 55 PE dependency edges, component evidence and source/toolchain identity.
The local validation directory has **536 files including the manifest**, total
**39,708,763 bytes** (about 37.87 MiB). Its file list is not a ZIP contents list:
**there is no ZIP and therefore no ZIP SHA-256**.
Manifest SHA-256:
`8803f1eb4a5cd50e96707d19f78e93d43114cca373fa52f65bfc155fc265b4db`.
Local input manifests and raw logs remain ignored; absolute developer paths
are removed from the public candidate manifest.

### Follow-up: vcpkg installed-status evidence (2026-10-31)

The committed candidate manifest is an immutable record of the **earlier** local
validation run, not a newly regenerated or corrected runtime receipt.
Its nested `evidence.installed_status` for all 18 `vcpkg:` components
incorrectly repeats the last processed port's `Version: 1.5.7` and the same
`Abi`. Seventeen of those recorded versions contradict their own component
versions; the `zstd` version happens to agree. **Do not use any of these
nested status fields as per-port release or license evidence.**

The source-only collector now selects the installed-status stanza by the
actual component port rather than reusing a loop variable. A synthetic test
covers distinct port versions/ABIs, `Port-Version`, and absent stanzas without
borrowing another component's evidence. This corrects future collection only:
the published candidate manifest, its hashes, the existing EXE, and the
packaging/licensing gate were **not** changed or re-attested.

Before any release decision, rerun the collector on the actual reviewed vcpkg
installation into a new ignored output directory, compare each port's status
against installed lists and hashes, and produce fresh bound provenance. The
Windows portable release remains **BLOCKED: LICENSING**.

## Actual dependency closure

The optimized EXE's normal imports were independently checked with MSVC
`dumpbin /DEPENDENTS` and the script's PE parser. The parser also covers delay
imports and recursively checks every packaged root-level PE file. Non-system
app-local dependencies are:

| File | Version/evidence | SHA-256 |
|---|---|---|
| `pdfium.dll` | local DLL/bundle 151.0.7881.0, API 7881 | `79d4676b656cfb1abcea88f9ade3b4b0826c5200382db5f4ec72a636c598c118` |
| `msvcp140.dll` | file version 14.51.36247.0, installed VC145 redist | `7c26614e1d733892c2deac7e245ce115504b1d80592dd0a01b08e3e5a55f89ca` |
| `vcruntime140.dll` | file version 14.51.36247.0 | `d1f4225df2cd877dbf130d5668a021dce3f94118455ff5ec952061c30afc9ce7` |
| `vcruntime140_1.dll` | file version 14.51.36247.0 | `a7146c08f89fe5b04541ab507cdb59ff7b44534d4ba3c668a426c6450a03434e` |
| `tessdata/eng.traineddata` | official tessdata_fast pinned commit below | `7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2` |
| `tessdata/rus.traineddata` | same | `e16e5e036cce1d9ec2b00063cf8b54472625b9e14d893a169e2b0dedeb4df225` |

Windows system/API-set imports remain operating-system dependencies, not copied
System32 files. Windows 10/11 x64 with system UCRT is the intended deployment
scope. `tesseract.exe` is neither used nor included.

Tesseract **5.5.2**, Leptonica **1.87.0**, libheif **1.23.1**, libde265
**1.1.1**, and their native dependency inputs are static archives from
`x64-windows-static-md`, not the dynamic DLL set of the prior service bundle.
The EXE does not import tesseract/leptonica/heif/de265 DLLs. Do not copy the old
bundle's entire DLL directory based on names alone.

The manifest has 282 locked registry-crate records and 18 native-port input
records with per-archive hashes. These are a conservative dependency/link-input
inventory, **not proof that every optional, target-specific or build dependency
is retained in the executable**. Exact static linker-member closure, Rust
toolchain runtime notices and full redistribution review remain publication
work; a PE import table cannot prove them. Eleven registry sources are absent
from the local cache, largely non-Windows/optional entries. Missing license
evidence is recorded as NOASSERTION rather than invented.

## Licensing decision

All distribution flags stay blocked. Project MIT does not make a combined
native executable MIT-only.

1. **PDFium provenance/closure:** local VERSION, args and license texts exist,
   and the DLL renders both controls. They do not establish the authoritative
   source/build receipt or exact completeness of notices for this binary.
   Existing fingerprint alone is not redistribution approval.
2. **LGPL native code:** libheif/libde265 are statically linked. Corresponding
   source, application relink/rebuild instructions and applicable distribution
   terms have not been assembled and verified as a compliance packet. Local
   upstream source tarballs do exist; the blocker is not falsely described as
   unavailable source. SHA-256: libheif source
   `0b14d6bdf5680488e3aede354b1e11be1444b3fc4a30fcf2ae06bd6b601466be`;
   libde265 source
   `5b4fac677018e6074196e8f9889f3e4a5310e46afbf22a893f620d4e24d3510e`.
   Merely adding LGPL text does not complete this review.
3. **MSVC:** app-local files come from the installed Visual Studio redist, not
   System32. The relevant distributor eligibility and exact license/REDIST-list
   evidence are not established by installation, filename or hash alone.
4. **Models:** eng/rus match official tessdata_fast commit
   `87416418657359cb625c412a48b6e1d6d41c29bd`; Apache-2.0 notice is retained.
   No custom, Chinese, commercial or research models are added.
5. **Full static/notices review:** input archive hashes and collected notices
   are preserved, but unresolved licenses, actual retained components and the
   complete binary compliance packet must be resolved before publication.

Primary terms: [PDFium](https://pdfium.googlesource.com/pdfium/+/refs/heads/main/LICENSE),
[Tesseract 5.5.2](https://github.com/tesseract-ocr/tesseract/blob/5.5.2/LICENSE),
[Leptonica 1.87.0](https://github.com/DanBloomberg/leptonica/blob/1.87.0/leptonica-license.txt),
[libheif 1.23.1](https://github.com/strukturag/libheif/blob/v1.23.1/COPYING),
[libde265 1.1.1](https://github.com/strukturag/libde265/blob/v1.1.1/COPYING),
[pinned models](https://github.com/tesseract-ocr/tessdata_fast/blob/87416418657359cb625c412a48b6e1d6d41c29bd/LICENSE),
[Microsoft redistribution](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170).
This records operational release evidence and unresolved obligations; it does
not declare blanket legal clearance.

## Tests actually executed

| Gate | Actual result |
|---|---|
| Release/native build, offline and locked | PASS, then rebuilt through packaging script |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --offline --locked --all-targets -j 1` | PASS |
| `cargo test --offline --locked -j 1 -- --test-threads=1` | 218 PASS, 3 diagnostic tests ignored |
| `cargo clippy --offline --locked --all-targets -j 1 -- -D warnings` | PASS |
| `cargo check --offline --locked --features native -j 1` | PASS |
| `cargo test --offline --locked --features native -j 1 -- --test-threads=1` | 303 PASS, 4 ignored |
| Isolated exact PDFium text test with `--ignored --lib` | 1 PASS |
| Additional native library Clippy, `-D warnings` | PASS |
| PowerShell source-only PE/integrity/license-gate tests | PASS |
| Python inventory boundary/dual-license tests | 3 PASS |
| Actual runtime inventory with `-CreateZip` | correctly rejected before payload/ZIP creation |
| Real clean Windows VM / unprovisioned machine | **NOT RUN** |
| Extracted public ZIP smoke | **NOT RUN: no cleared ZIP** |
| Manual provisioned-runner release workflow | **NOT RUN** |

Toolchain: rustc 1.97.1, Cargo 1.97.1, existing LLVM 22.1.6 and MSVC/vcpkg.
Observed local OS: Windows NT 10.0.26300.0, x64 PE target. No installs,
global environment changes, service stops or production runtime replacement.

The real smoke copied the complete local validation tree to a new Cyrillic
path with spaces, set an unrelated child WorkingDirectory, cleared OCR runtime
overrides and restricted process-local PATH to Windows directories. Both inputs
also live in that copied path. No CLI PDFium/tessdata parameters were supplied.

| Synthetic control | Pages | Route | Page characters | Tables | Baseline |
|---|---:|---|---:|---:|---|
| native text/table | 1 | native_text+grid | 343 | 1 | equal |
| image-only raster PDF | 1 | ocr+grid | 627 | 1 | equal |

Each produces nonempty `result.json`, `result.v1.json`, DOCX and XLSX. The raster
case has actual recognized words, not an empty successful pipeline. Missing
PDFium, missing eng/rus, invalid explicit tessdata and invalid TESSDATA_PREFIX
all fail with relevant diagnostics and no published output. Each child has a
120-second timeout; stdout/stderr are drained and retained. Only owned child
trees may be terminated, never processes found by name.

Text, cell values, confidence, physical bbox/spans, routing, language, warnings,
legacy JSON and selected DOCX/XLSX XML match accepted PR #1 controls. Timings and
volatile Office metadata are intentionally excluded; no OCR difference is
waived. This proves bounded compatibility, not customer-document accuracy.

## Runtime discovery changes and remaining risks

Explicit tessdata > TESSDATA_PREFIX > EXE-adjacent tessdata > development.
Invalid overrides do not silently fall back. Windows non-ASCII paths use an
exclusive API-owned ASCII model copy; native handles die before owned files.
If a Unicode TEMP has no ASCII short alias, an actionable process-local TEMP
error remains a documented limitation. A marked portable package cannot borrow
missing PDFium/tessdata from the developer tree. Four new native unit tests
cover these rules. Render functions/ABI, native extraction, grid geometry,
orientation, OCR profiles, thresholds, routing, models and export contracts are
unchanged (only PDFium's loader-path resolver changed).

Controlled-PATH tests are **not** clean-machine proof: system-installed runtime
components could still mask deployment requirements. The clean-Windows receipt
is a distinct ZIP gate. Functional HEIF/TIFF/DjVu behavior was not newly
certified by these two PDF smokes; existing native unit coverage was rerun.

Byte-for-byte compiler reproducibility is not certified: two optimized EXEs
had identical `.text`, `.data`, `.pdata` and `.reloc`, but different `.rdata`
and overall hashes. The exact tested final EXE is pinned above. Packaging is
input/hash-gated, not claimed to reproduce identical compiler bytes everywhere.
The preserved development fallback also embeds a build-tree location in the
EXE; binary path-remapping/privacy review is not certified by the sanitized
JSON inventory. No such executable is publicly uploaded by this task.

## GitHub Actions observation

Both source checks passed at reviewed source/attribute HEAD
`2df60dfc07018e53067e4112ee72508238f6bb4d`:
[branch push PASS](https://github.com/paulgreenbox-beep/rust-document-ocr/actions/runs/40687589710/job/122813832633)
and [PR PASS](https://github.com/paulgreenbox-beep/rust-document-ocr/actions/runs/40687597602/job/122813862100).
GitHub reported MERGEABLE; no merge was performed. This report-only update
follows that checked code and is checked again before final handoff.

An intermediate attributes-only commit temporarily removed the pre-existing
PDF binary-attribute rule; CI rejected the checkout's synthetic fixture checksum.
The rule was restored, the exact manifest bytes were explicitly indexed, and
the two checks above passed. Accepted fixture files and their committed hashes
were not changed. Git and the validation payload now contain byte-identical
candidate manifests with the SHA-256 recorded above.

The source CI includes the new runtime-free PowerShell checks; it does not
replace actual local native smoke or the **NOT RUN** clean-machine and manual
release-workflow gates. No public Release or ZIP was created.
