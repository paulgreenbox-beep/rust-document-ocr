# HEIF LGPL release options

Review date: 2026-10-30. **Analysis only; Windows binary distribution remains
blocked.** This concerns libheif/libde265 in the Rust EXE, not the PDFium DLL.
No feature, dependency, native build, runtime, or release setting is changed.

## Confirmed input evidence and limits

The [PR #2 candidate manifest](https://github.com/paulgreenbox-beep/rust-document-ocr/blob/e031efd17fa7fe8dcfd2240535692ad5ac7d9efd/reports/WINDOWS_PORTABLE_CANDIDATE_MANIFEST.json)
and [release report](https://github.com/paulgreenbox-beep/rust-document-ocr/blob/e031efd17fa7fe8dcfd2240535692ad5ac7d9efd/reports/WINDOWS_PORTABLE_RELEASE_V1.md)
identify libheif 1.23.1 and libde265 1.1.1, static library inputs from
`x64-windows-static-md`. Read-only local checks confirmed:

| Input | SHA-256 |
| --- | --- |
| `heif.lib` | `041f516be7f146b687226af6aefa8de485a941aa995390afe2b87e8c0fa4ff0f` |
| `libde265.lib` | `31f871774382e4732dfcad572c43dec79543baad860615d76c455b5d26c5f14c` |
| `strukturag-libheif-v1.23.1.tar.gz` | `0b14d6bdf5680488e3aede354b1e11be1444b3fc4a30fcf2ae06bd6b601466be` |
| `strukturag-libde265-v1.1.1.tar.gz` | `5b4fac677018e6074196e8f9889f3e4a5310e46afbf22a893f620d4e24d3510e` |

The source archives exist, but a complete distributable corresponding-source
and recombination package has **not** been assembled or demonstrated. Library
inputs and the lack of DLL imports are not an inventory of every retained
static member.

### Existing manifest discrepancy

Both components' nested `installed_status` fields incorrectly contain version
`1.5.7` and the same ABI. They are inconsistent with the top-level component
versions and must not be relied on as correct HEIF installation receipts.
Direct status inspection instead records:

- libheif 1.23.1 ABI `2cfc5e1f0eb3a4e94ba47712f40087698ac5dea9ebf9cfabae85c3e1e4efbb4d`;
- libde265 1.1.1 ABI `e252a7dcfe273fb4d4bc1517bbf134d26f7b53f0f5132bfa8d6252133d8f7e69`.

This is a documentation finding, not a replacement manifest. PR #2 and its
blocked status remain untouched. Repairing the collector/evidence mapping
requires a separately reviewed change before any final release inventory.

Static-md means **static libraries with dynamic CRT**, not static CRT.
ABI receipts record core-only features. The observed libheif CMake configuration
has `BUILD_SHARED_LIBS=OFF`, `WITH_LIBDE265=ON`, libde265 plugin OFF,
X265/X264/AOM OFF, and plugin loading ON. Current port defaults must not be
substituted for this observed configuration: they can add x265/HEVC encoding.
No x265 inclusion is inferred from today's default feature list.

## Terms that the release packet must address

The pinned [libheif COPYING](https://github.com/strukturag/libheif/blob/v1.23.1/COPYING)
and [libde265 COPYING](https://github.com/strukturag/libde265/blob/v1.1.1/COPYING)
contain LGPLv3/GPLv3 terms and distinguish the libraries from separately
licensed examples/wrappers. MIT Rust bindings do not relicense these libraries.

Under LGPLv3 sections 0, 3 and 4, review the Combined Work, header-derived
code, notices/license texts, and terms permitting library modification and
reverse engineering for debugging those modifications. Section 4(d) requires
either usable Corresponding Application Code and Minimal Corresponding Source
for recombination, or a suitable replaceable shared-library mechanism.
Small-header exceptions do not erase linked-library obligations.

GPLv3 sections 1 and 6 inform source scope and delivery. Preserve controlling
build/install scripts and applicable modifications; assess permitted exclusions
rather than assuming an upstream source URL alone suffices. For network
distribution, document the actual source-access route. Corresponding Application
Code can be source and/or object code as defined by LGPLv3, not necessarily
application source in every arrangement. LGPLv3 section 4(e) requires Installation
Information only when GPLv3 section 6 would require it, and only as needed for
the modified Combined Work. Assess the User Product/transaction conditions;
neither universal applicability nor universal exemption is asserted here.

These are compliance work items, not a legal determination for this publisher.
The responsible release owner and qualified reviewer must approve the chosen
route and its complete terms.

## Option A retain static HEIF with a usable rebuild or relink route

**Cost: high evidence/maintenance effort. Intended behavior risk: low if inputs
and features are preserved; actual reproducibility is untested.**

Supply exact library source and applied patches, portfiles, auxiliary files,
triplet, explicit features and build settings. Observed libheif recipe files
include `cxx-linkage-pkgconfig.diff`, `find-modules.diff`, `gdk-pixbuf.patch`,
`symbol-exports.diff`, and `cmake-project-include.cmake`; libde265 uses
`fix-linkage.patch` and `fix-api-visibility.patch`. Unpatched upstream tarballs
alone do not capture these inputs. Pin and hash the actual recipe files before
publishing a source/relink package.

Provide suitable application code/objects and other inputs, or complete
rebuildable application source under terms permitting recombination. Record
the source commit, Cargo.lock, required dependency sources/generated inputs,
build scripts, link recipe and necessary non-system archives. Assess LTO/object
compatibility. Document legitimate acquisition of tools; do not automatically
redistribute proprietary SDK/tool binaries. Observed baseline versions include
Rust/Cargo 1.97.1, libclang 22.1.6, CMake 4.3.3, MSVC 14.51.36231; those labels
alone are not a complete reproducible toolchain receipt.

**Acceptance:** an independent recipient rebuilds both libraries, makes an
interface-compatible library modification, recombines/rebuilds the application,
and runs it without private infrastructure. Retain notices and source-access
evidence. Identical EXE bytes are not the test. This has **NOT RUN**.

## Option B replaceable shared HEIF libraries

**Cost: medium-to-high build/deployment work. Regression risk: medium-to-high;
plausible route, not validated.**

The locked libheif-sys build uses vcpkg discovery on Windows/MSVC; the locally
available vcpkg-rs supports `VCPKGRS_DYNAMIC=1` with
`VCPKGRS_TRIPLET=x64-windows`. Shared HEIF/de265 files exist locally, but their
existence does not qualify them for this release. A global triplet switch may
also change Tesseract/Leptonica and their dependency closure. Selectively changing
HEIF requires a separate design and verification. Merely copying DLLs beside
the existing static EXE cannot change its linkage.

**Acceptance:** import/link evidence proves the intended DLL boundary, an
interface-compatible modified library can replace the distributed one and work,
loader behavior permits that replacement, and codec/plugin/transitive closure
is inventoried. Run HEIF controls, PDF/image regressions, missing-library tests
and a clean-machine test. Distributed library binaries still need their own
source/notices review. No implementation or test was performed here.

## Option C HEIF-disabled PDF-focused build

**Cost: medium scoped source/test work. Regression risk: medium at feature and
input-dispatch boundaries. Not an existing working build option.**

At the reviewed source baseline, `native` enables `heif-input`.
`--no-default-features --features native` therefore still includes HEIF.
Removing native also removes the production native PDF/OCR path. Although
native/mod.rs conditionally declares heif_source, native/input_source.rs uses
HEIF imports/variants/branches/error mappings within native without equivalent
gating. No existing PDF-only release is claimed.

A future change must consistently gate imports, dispatch, variants, forwarding,
errors and tests, and review the packaging script's native feature selection.
**Acceptance:** native PDF/OCR builds without HEIF native inputs; link evidence
confirms absence; HEIF input fails explicitly; accepted native-text/raster/table
PDF results remain compatible; inventory/notices reflect the new closure.
Other PDFium/MSVC/model/static-component gates remain.

## Decision

Choose a route before implementation. No option is complete or approved here.
Keep current features, runtime and `distribution_status=blocked`. No rebuild,
source-package upload, binary publication or OCR behavior change was performed.

Implementation anchors: [reviewed Cargo.toml](https://github.com/paulgreenbox-beep/rust-document-ocr/blob/e031efd17fa7fe8dcfd2240535692ad5ac7d9efd/Cargo.toml),
[input dispatch](https://github.com/paulgreenbox-beep/rust-document-ocr/blob/e031efd17fa7fe8dcfd2240535692ad5ac7d9efd/src/native/input_source.rs),
[vcpkg-rs dynamic documentation](https://github.com/mcgoo/vcpkg-rs#dynamic-linking).
