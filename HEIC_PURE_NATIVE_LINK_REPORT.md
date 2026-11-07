# Pure HEIC feature separation and Windows final-link review

Date: 2026-11-07. Research branch: `experiment/heic-rs-pure-native-link`.
Base: PR #7, `191943cdf3728ebf30856a6c90d9cffd801d07e9`.

**LIBHEIF/LIBDE265 REMOVAL — VERIFIED**, scoped to the exact pure CLI artifact
identified below and the available graph/source/link evidence. This is not a
whole-program SBOM, a statement about other feature builds or support-DLL
internals, an accuracy certification, or redistribution approval.

**WINDOWS PORTABLE RELEASE — BLOCKED: LICENSING.** No installation, public ZIP,
tag, release, merge, model replacement, DB operation or service operation was
performed. Existing RC1 and working OCR were not replaced.

## 1. Identity and scope

Both new release executables were built from clean committed Rust sources:

- Source: `3185a5f5f1c79a5036b2a0c6d63e6334c4f66f26`.
- Git tree: `3349ed2e8a8cee6a9b14f2d55649173dd6479052`.
- Cargo.lock SHA-256: `890b2dfb70b4049c3feb72892d231ddc28730896701e4d88d4136761de080376`;
  unchanged from the base. Legacy packages remain in the lockfile deliberately.
- Windows x64 MSVC; Rust/Cargo 1.97.1, Rust LLVM 22.1.6.
- Installed MSVC toolset directory 14.51.36231; actual linker FileVersion
  **14.51.36248.0**. These are different recorded version identifiers, not an
  inferred equality.
- Later commit `5e63d7e85bd16d793c25363b3aeb4922e218d0c5` changes only verification
  tools. Subsequent report/tool documentation does not change compiled OCR
  sources. The final PR HEAD is not misrepresented as the historical build SHA.

| Artifact | Features | Bytes | SHA-256 |
|---|---|---:|---|
| Pure research CLI | `heic-rs-input` only | 19,186,688 | `c6cddc7516d5e44b225cde2fabb35e534016e05a7c900774b087019e538dfc23` |
| Same-source legacy control | `native` only | 20,172,800 | `8afa2d6c153fd3ccf0e6dcd8cd34e408525f7ca302b58861af6dfbefb7adcad3` |

The pure artifact is 986,112 bytes smaller; size alone is not exclusion proof.
Raw response files, linker output/maps, PDB, source paths, receipts and customer
results remain private. Their sanitized measurements and checksums are in
[pure-native-link-evidence.json](tools/heic-rs-evaluation/pure-native-link-evidence.json),
including all 225 selected packages and features. No binaries or customer text
are included in this PR.

## 2. Feature architecture and compatibility

| Selection | Common OCR stack | HEIC dispatch | Legacy dependency selected |
|---|---|---|---|
| No native features | Source-only behavior | Unsupported | No |
| `native` | Yes | libheif | Yes |
| `heic-rs-input` | Yes | Patched local heic-rs | No |
| `native,heic-rs-input` | Yes | heic-rs, explicit priority | Yes; not a pure build |
| `--all-features` | Yes | heic-rs, explicit priority | Yes; not a pure build |
| `native-common` | Yes | Unsupported HEIF input | No |

`native-common` owns PDFium, Tesseract/Leptonica, raster/TIFF, WebP and DjVu.
`native` adds `heif-input`; `heic-rs-input` independently adds the Rust decoder.
Shared HEIF limits/errors/warnings moved to `heif_types`; legacy public paths
re-export them. Shared module gates use `native-common`. No decoder retries
another backend on error. Mixed-feature tests explicitly reject premultiplied
alpha instead of falling back. Diagnostic/example feature requirements were
adjusted only where they require the common stack.

No PDF rendering, native text, OCR, grid, orientation, assignment, geometry,
confidence, JSON/DOCX/XLSX algorithm or four-worker default was changed.

## 3. Graph and actual link evidence

The pure selected graph contains neither `libheif-rs`, `libheif-sys`, libheif nor
libde265. Registry presence, lockfile presence, installed vcpkg libraries and
`/LIBPATH` search directories are not treated as linked code.

| Observation | Pure | Legacy positive control |
|---|---:|---:|
| Captured final arguments | 258 | 262 |
| Resolved/scanned archives | 236 | 240 |
| llvm-nm scan failures | 0 | 0 |
| Symbol-output lines inspected | 276,219 | 356,145 |
| Old HEIF archive inputs | 0 | 4 |
| Native API definitions in old archives | 0 | 469 heif + 56 de265 |
| Map lines matching old archive names | 0 | 2,368 |
| Verbose-link lines matching old archives | 0 | 26 |
| Old HEIF DLL imports/literals | None found | None found |

Legacy inputs are the libheif-rs/sys rlibs, `heif.lib` and `libde265.lib`.
The positive control demonstrates why PE-import absence **alone is not proof**:
legacy HEIF code is statically linked and has no HEIF DLL import either.

The map and locale-independent symbol/input analysis supplement the selected
graph and cfg/source review. PDFium remains dynamically loaded; no HEIC-specific
LoadLibrary/FFI/fallback path was found in the selected Rust decoder. The exact
CLI still imports Microsoft runtime/system libraries. Package PE closure and
real controlled-PATH smoke were checked separately.

**Coverage limit:** nine temporary Rust linker objects (one `symbols.o`, eight
own-binary `rcgu.o`) were deleted by rustc after linking and were not independently
hashed/scanned. They remain explicitly `not_scanned_temporary_objects` in the
receipt, not silently reported as zero unresolved inputs. The final map remains
available. This is converging bounded evidence for exclusion of these named
libraries, not exhaustive identification of every retained machine instruction.

The initial response-file parser diagnostic is retained privately but is not
acceptance evidence: Windows CommandLineToArgvW does not split LF-separated
rustc MSVC response arguments. Corrected analysis validates one complete quoted
argument per line, refuses unsupported/empty input grammar, and was checked
against the legacy positive control. The reproducible analyzer is
[inspect-link-evidence.py](tools/heic-rs-evaluation/inspect-link-evidence.py).
Its raw output must remain local because it contains build paths.

## 4. Commands and verification

Run each build in its own evidence directory and use an explicit research target;
`<...>` below denotes caller-supplied absolute local paths, not a shell command.
The linker recorder is compiled separately with rustc. Its two environment
variables `HEIC_REAL_LINKER` and `HEIC_LINK_EVIDENCE_DIR` are process-local.

```powershell
cargo check --offline --locked --no-default-features --features heic-rs-input --all-targets --target-dir <research-check-target> -j 2
cargo test --offline --locked --features native --target-dir <research-check-target> -- --test-threads=1
cargo test --offline --locked --no-default-features --features heic-rs-input --target-dir <research-check-target> -- --test-threads=1
cargo rustc --release --offline --locked --no-default-features --features heic-rs-input --bin ocr-pipeline --target-dir <research-release-target> -j 1 -vv -- -C linker=<capture.exe> -C link-arg=/VERBOSE:LIB -C link-arg=/MAP:<private-map>
```

The legacy release command differs only by `--features native` and its own
capture/map destinations. Pure build elapsed 662,432 ms; cached same-source
legacy control 75,659 ms. These are not clean comparative build benchmarks.

| Check | Observed result |
|---|---|
| fmt and ordinary git diff --check | PASS |
| Six graph/all-target compilation configurations | PASS |
| Legacy full tests | 359 passed, 5 ignored |
| Pure full tests | 366 passed, 5 ignored |
| Two binding-sensitive PDFium tests in separate processes | 2 passed |
| Mixed/common-only/all-features dispatch tests | 4 / 2 / 4 passed |
| Vendor resource subset with std + parallel | 10 passed |
| A/B comparator fail-closed tests | 5 passed |
| Portable source-only synthetic gates | 50 PASS assertions, exit 0 |

Three diagnostic-only ignored tests were not promoted to mandatory runtime
acceptance. Initial local TEMP access and junction-permission failures were
retained; process-local TEMP and an approved synthetic junction-test rerun
resolved the environment constraints. No gate was disabled.

CI must be checked on this PR's final exact HEAD. Authoritative current run URLs
and conclusions are recorded in the PR, not substituted with old PR #7 passes.
Source/isolated-decoder CI is not Windows native release qualification.

## 5. Resource safeguards and negative CLI checks

The existing local `heic-rs 0.1.4-ocr.1` derivative is unchanged. It checks raw
SPS geometry, conservative pre-crop sizes, all requested grid/tile occurrences,
alpha and aggregate raw-luma sites with checked arithmetic before primary HEVC
picture buffers/parallel work. Root dispatch supplies a 512 MiB input limit,
50 million output pixels and existing frame limits; decoder threads remain one.
Input-file/canvas limits are separate from raw-picture limits. The latter are
**not an exact RSS/heap-byte cap**, nor proof against all hostile inputs.

The pure release CLI rejected both a premultiplied-alpha derivative and a valid
HEIF-signature/truncated-container fixture with `heif_decode_failed`, exit 1,
no timeout, and **no published output directory**. Premultiplication remains
explicitly unsupported; no confidence-based/native fallback is added.

## 6. New release OCR comparisons

Fresh release binaries, not old/debug receipts, were used with rus+eng,
OCR DPI 300, grid DPI 100, workers 4, cell fallback true and OMP_THREAD_LIMIT=1.

| Synthetic SVG-reference input | Legacy token F1 | Pure token F1 |
|---|---:|---:|
| Form | 94.34% | 98.04% |
| Note | 100% | 100% |
| Receipt | 92.5% | 92.5% |

11/11 pure inputs succeeded; legacy succeeded on 10/11 and rejected white-16
under its unchanged decoder limits. The receipt retains one grid/12 filled
cells. Both PDF controls match full non-timing JSON/legacy DTO and inspected
OOXML content. HEIC color/confidence/bbox/export differences are recorded, not
normalized away or called broad accuracy gains. Tiny synthetic format patterns
are format checks, not human-verified OCR holdout. A checkerboard's apparent
table is not evidence of useful table recognition.

The private accepted corpus then completed: **40 PDF / 345 pages / 4,071
positions / 36,633 raw fields / 36,639 reviewed fields**, with all six known raw
oracle errors explicitly retained. NativeText, table data, positions, typed-f32
geometry/confidence, complete non-timing v1/legacy JSON, and all unique DOCX/XLSX
ZIP members matched the accepted results. Embedded XML bytes were not rewritten;
only ZIP container metadata/order/compression and explicit page timings were
excluded. EG79 model-load and implicit/explicit language-default controls passed.
Total runner wall time: **107.188 seconds**, not a new historical performance
benchmark. Inputs and both runtime bundles were hash-checked unchanged.

Two initial private attempts stopped before the corpus because short successful
CLI executions finished before the observer obtained its required second ACTIVE
module snapshot. Those attempts remain FAIL_OR_INCOMPLETE. A separate private
observer variant requests a 5 ms follow-up **after one complete snapshot**, keeping
initial/error polling at 50 ms. It retains both identical ACTIVE snapshots,
PID/birth/image/module-path checks, 299 recovery limits, timeout, job ownership,
pipe draining and every semantic/export/oracle gate. Both baseline and candidate
used this variant. Frozen runner/observer source files were not edited. This
instrumentation change is recorded by source/effective-function/wrapper hashes.

## 7. Local package and preserved boundaries

The unchanged fail-closed packager prepared a fresh **local directory only**:
551 files, 54,226,081 bytes, distribution_status `blocked`.
Manifest SHA-256:
`f6c8208182d4f590a7f1c586e3892722e122afc1fbf7e3d93282092e75c30469`.
It used the new hash-bound build receipt, real new EXE, accepted baseline hashes,
PE closure, controlled PATH and Cyrillic/space-path smoke. Missing PDFium and
traineddata negatives remained effective. No real-runtime ZIP was created.

Its new inventory filters the selected Cargo graph, excludes the four old HEIF
static entries and adds local derivative licenses/NOTICE/patch provenance.
Graph-selected build/dev entries remain a conservative superset, not a retained
code SBOM. Extra legacy copyright texts are labeled notice evidence only.
Native Version/ABI fields were reread by exact Package+Architecture core stanza;
the known historical cross-stanza collector data were not copied as current
facts or rewritten in historical receipts. No rights were marked verified.

The original RC1 EXE remains
`c305d60ec3861ebb5cf4e9532f346559be4ebc1b5989da31412c1c36f71f8cb4`.
The original 546-file package and six protected receipts were rechecked, not
regenerated. No working ONLYOFFICE runtime, DB, services, model/tessdata or
PDFium was changed. No vcpkg/toolchain/cache cleanup was performed.

## 8. Release decision and remaining work

| Stage | Status |
|---|---|
| Feature separation | PASS |
| Legacy native compatibility | PASS |
| Pure heic-rs compilation | PASS |
| Cargo dependency exclusion | VERIFIED |
| HEVC resource safeguards | PASS, bounded scope |
| Windows release EXE | BUILT |
| Linker/PE removal evidence | VERIFIED, exact artifact and stated limits |
| 11-input A/B | PASS, limited synthetic/format scope |
| 40-PDF regression | PASS, private accepted corpus |
| Local packaging | PASS, local directory only |
| Licensing | OPEN |
| Clean Windows | NOT RUN — DEFERRED BY OWNER |

The old HEIF LGPL chain is excluded for **this pure artifact**, not for legacy
RC1/native/mixed builds. PDFium notices, Microsoft CRT redistribution rights,
EG79 rights, HEVC patent review and modified heic-rs notices remain independent
release-owner/compliance decisions. Sandbox and separate clean Windows x64
remain **NOT RUN — DEFERRED BY OWNER**. The real clean-Windows receipt and all
license/hash/baseline/transactional ZIP gates remain required for public ZIP.

Next safe action: review this isolated source PR and its exact-head checks;
review the local evidence/package separately. Do not install, merge into main,
publish RC2, or remove the global licensing block automatically.
