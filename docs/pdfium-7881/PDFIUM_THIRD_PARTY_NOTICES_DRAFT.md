# PDFium 7881 third party notices draft

**DRAFT — NOT APPROVED FOR REDISTRIBUTION.** Review date: 2026-10-30.

This is the binary-specific **design and review index** for the existing
Windows AMD64 PDFium 151.0.7881.0 / API 7881 DLL, not an approved distribution
packet. It contains no binary and is not connected to the ZIP packager.
Retain all original producer texts and these supplements if later approved;
links or this index alone cannot replace the actual required license texts
in a future binary package.

## Exact artifact and separate evidence claims

- DLL: 7,211,520 bytes, SHA-256
  `79d4676b656cfb1abcea88f9ade3b4b0826c5200382db5f4ec72a636c598c118`.
- [Producer release](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F7881):
  `pdfium-win-x64.tgz`, 3,733,154 bytes, SHA-256
  `73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08`.
  Release ID 336103186; asset ID 441936973.
- Producer recipe: `867b3ecc32163d715e84708c51fae5f031254985`.
- [Attestation 30308810](https://github.com/bblanchon/pdfium-binaries/attestations/30308810)
  was explicitly verified against the existing archive on 2026-10-30,
  exit 0. OIDC issuer `https://token.actions.githubusercontent.com`;
  signer `bblanchon/pdfium-binaries/.github/workflows/build-all.yml@refs/heads/master`;
  source/signer digest equals the producer recipe; GitHub-hosted runner.
  Invocation: [run 27143496415, attempt 1](https://github.com/bblanchon/pdfium-binaries/actions/runs/27143496415/attempts/1).
- Public release metadata and the signed subject establish **archive-to-producer
  origin**. The DLL hash and all **46/46** member comparisons are local
  byte measurements, not an upstream claim about this user's bundle.
  The archive was read in memory without extraction, execution or new download.
- The executed PDFium checkout, resolved GN/DEPS/toolchain and retained-object
  inventory are **UNKNOWN**. Candidate `91b9d569b34be4f38eed7b3c49b227356c3aadad` is not the executed source
  receipt. Attestation resolves the builder repository only.

Reproducible policy example, using already obtained public text/archive files:

```powershell
gh attestation verify ./pdfium-win-x64.tgz --bundle ./30308810.bundle.json `
  --repo bblanchon/pdfium-binaries `
  --signer-workflow bblanchon/pdfium-binaries/.github/workflows/build-all.yml `
  --source-digest 867b3ecc32163d715e84708c51fae5f031254985 `
  --signer-digest 867b3ecc32163d715e84708c51fae5f031254985 --deny-self-hosted-runners
```

This command is verification, not a permission to acquire, replace or distribute
runtime files. Producer patches and the nine archived explicit GN arguments
are detailed in [LICENSE_EVIDENCE.json](LICENSE_EVIDENCE.json).
Their correspondence to the signed package is not a full historical execution
receipt. No extra user source patches are invented.

## Required product credits

This product includes PDFium and third-party software governed by their
respective licenses. Rust Document OCR's MIT license does not replace them.

This software is based in part on the work of the FreeType Team.

This software is based in part on the work of the Independent JPEG Group.

These are draft binary-product credits, not an assertion that a public
runtime package is already distributed. The IJG sentence also survives in
the original README.ijg text under its renamed archived filename.

## Preserve all 16 original notices

The table uses **original archive paths**, not paths newly added to this
repository. Source links identify the pinned text or extraction origin;
the exact original bytes are identified by SHA-256. Collector transformations
and CRLF/LF differences mean a linked source need not be byte-identical to
the archived notice. Do not rename/rewrite/drop originals to match these links.
The original builder MIT is package-level, not an MIT-only license for the DLL.

| Original file to preserve | Source or extraction origin | Original SHA-256 |
| --- | --- | --- |
| `licenses/abseil.txt` | [pinned source](https://chromium.googlesource.com/chromium/src/third_party/abseil-cpp/+/5e42a36a85a252d8cdee6c39661d2bfd9883fd5c/LICENSE) | `f54fff0b905df5b3464527c652a30e903b172d6dcab4d89b5e6f105d5e4a4603` |
| `licenses/agg23.txt` | [pinned source](https://pdfium.googlesource.com/pdfium/+/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/agg23/agg_array.h) | `c110d3ea2ad77467ce0dcff7d3337e6c8be8049a5103f4b9bd5fd911a77972e5` |
| `licenses/fast_float.txt` | [pinned source](https://chromium.googlesource.com/external/github.com/fastfloat/fast_float/+/05087a303dad9c98768b33c829d398223a649bc6/LICENSE-MIT) | `bf1b57355feca8fce77ee95f48002f8d4789fb71b30ec7599c06cda4901fbb2b` |
| `licenses/freetype.txt` | [pinned source](https://pdfium.googlesource.com/pdfium/+/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/freetype/FTL.TXT) | `f4b133e25df1f86ad3ffea453aa0e613f0474f34778dbbb3e437e7b2724937d8` |
| `licenses/icu.txt` | [pinned source](https://chromium.googlesource.com/chromium/deps/icu/+/3859e64eed5d34544b27fbcab0ac1685ce83df3c/LICENSE) | `93679f4389d53b6835d89843f251844fb9bc455b35bab036d3c8e7abe497a47a` |
| `licenses/lcms.txt` | [pinned source](https://pdfium.googlesource.com/pdfium/+/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/lcms/include/lcms2.h) | `7312b68c5b25e9bf2b828706fb4e29588f22705112f411fd42e1f7d84c3d139a` |
| `licenses/libjpeg_turbo.ijg` | [pinned source](https://chromium.googlesource.com/chromium/deps/libjpeg_turbo/+/640f254ad0fa03f6b1f29f89b7dd9366f2f6e533/LICENSE.md) | `db16a04128171879c60708d171b88d97345a2dd20f9bfc173680a4497c73f704` |
| `licenses/libjpeg_turbo.md` | [pinned source](https://chromium.googlesource.com/chromium/deps/libjpeg_turbo/+/640f254ad0fa03f6b1f29f89b7dd9366f2f6e533/LICENSE.md) | `be2b2b5ab168bce87bc3e31f2a5c5adba4b7f6e9e51d618e958d1d46972ebd95` |
| `licenses/libopenjpeg.txt` | [pinned source](https://pdfium.googlesource.com/pdfium/+/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/libopenjpeg/openjpeg.c) | `c5ab0890a737c2dfa7ba675036554f6d17741d98629b0c2a145354d00617e6b2` |
| `licenses/libpng.txt` | [pinned source](https://chromium.googlesource.com/chromium/src/third_party/libpng/+/8a5b4ace0c2a8a47d73037610b98096f55f3d18f/LICENSE) | `452390433ba0f88aa3e2b122c647741b72a0c117cd6ed7a329b49785aecb5511` |
| `licenses/libtiff.txt` | [pinned source](https://pdfium.googlesource.com/pdfium/+/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/libtiff/t4.h) | `92b72ba97e6c2749c2a94bc0ef646b47080217f1e772a482b33cf5a5f98a6506` |
| `LICENSE` | [pinned source](https://github.com/bblanchon/pdfium-binaries/blob/867b3ecc32163d715e84708c51fae5f031254985/LICENSE) | `8854f4388f1ca13b3ad9baa42e95f5546b4c0b17109c159256d3eca7be39b09b` |
| `licenses/llvm-libc.txt` | [pinned source](https://chromium.googlesource.com/external/github.com/llvm/llvm-project/libc.git/+/9309c117ebae84dd2f9df1ef99de4782162527d5/LICENSE.TXT) | `3b6226c32e168c83b891d8d6f0d3c29c2116dc3ef93dc93c307b54f279ecf383` |
| `licenses/pdfium.txt` | [pinned source](https://pdfium.googlesource.com/pdfium/+/91b9d569b34be4f38eed7b3c49b227356c3aadad/LICENSE) | `961eacd9633fff6d051db7208b755e9210e30efac7adec3e6a6d52798f0ccf0e` |
| `licenses/simdutf.txt` | [pinned source](https://chromium.googlesource.com/chromium/src/third_party/simdutf/+/f7356eed293f8208c40b3c1b344a50bd70971983/LICENSE) | `c172a0ba936ff31230febb5dad869e25cb7c1a07480c7a381be8cf011bb52719` |
| `licenses/zlib.txt` | [pinned source](https://chromium.googlesource.com/chromium/src/third_party/zlib/+/5c1dfd53066bf58d3d28197f715717dd88762443/zlib.h) | `33fd641c9f3b0e0be64bc78fea9e94807674cdd70c48477599226cb8956565fe` |

The complete collector is [pinned 08-licenses.sh](https://github.com/bblanchon/pdfium-binaries/blob/867b3ecc32163d715e84708c51fae5f031254985/steps/08-licenses.sh).
It scans all build.ninja third-party names, not final DLL retained inputs;
unknown names produce warnings, and some names including libc are ignored.
Header extraction is not automatically incomplete, but is not a file-level
copyright inventory.

The ICU text includes Unicode-3.0, historical ICU/data terms and build-script
exceptions: preserve it whole, not a single-label substitute. Its GPL-labelled
build-tool material does not prove runtime GPL inclusion. The JPEG roll-up is
not itself a license; retain the full IJG and BSD material and assess API/SIMD
scope. AGG 2.3's actual short grant is not silently replaced by canonical MIT
or a later AGG license. Apache NOTICE applicability and any vendored exceptions
remain scope-specific. Full terms remain in the original files, not in these
short review descriptions.

## Existing seven supplements and one bounded addition

The original seven PR #3 texts are unchanged. Whole-file comparisons or
complete license-comment extractions were verified against pinned sources.
Exact sources, boundaries and file hashes are machine-readable in
[LICENSE_EVIDENCE.json](LICENSE_EVIDENCE.json).

| Supplement | Bytes | SHA-256 |
| --- | --- | --- |
| [freetype-fthash-MIT.txt](notices/freetype-fthash-MIT.txt) | 1,232 | `5285d96f6aa3188193d5fbefd8710ed6a46af185e0a43817c17acee5f4b1ab3d` |
| [harfbuzz-COPYING.txt](notices/harfbuzz-COPYING.txt) | 1,971 | `ba8f810f2455c2f08e2d56bb49b72f37fcf68f1f4fade38977cfd7372050ad64` |
| [harfbuzz-ms-use-COPYING.txt](notices/harfbuzz-ms-use-COPYING.txt) | 1,141 | `c2cfccb812fe482101a8f04597dfc5a9991a6b2748266c47ac91b6a5aae15383` |
| [openjpeg-LICENSE.txt](notices/openjpeg-LICENSE.txt) | 2,112 | `a6af136f3e15038a666b61f376612a07d9a4e48cb7c01adbf3e33b3f14ab49b6` |
| [openjpeg-dwt.c-header.txt](notices/openjpeg-dwt.c-header.txt) | 2,144 | `c251cf39bf030146b6cc78a12f492a2b3d6272e1da9382535bd5aa8941143aa4` |
| [openjpeg-t1.c-header.txt](notices/openjpeg-t1.c-header.txt) | 2,123 | `aeda538d62ab8c138b7c4bdd3333224afa9f4eba41653ac3ed3f40664d9aba71` |
| [openjpeg-ht_dec.c-header.txt](notices/openjpeg-ht_dec.c-header.txt) | 1,703 | `148fe28ad876cd8a8d526b85522b2b8cb87768eb6bdba1c45d46c0f5b149a7be` |
| [openjpeg-additional-file-notices.txt](notices/openjpeg-additional-file-notices.txt) | 10,259 | `d4a587d834032b6c1a0e017ac6def40e33bb9018e961e6d1b88905e2f0b1f035` |

### FreeType

Candidate bundled target selects ftbase.c, which includes fthash.c. The
existing supplement preserves the complete separate MIT grant, also matching
fthash.h. Its filename masthead and a following origin comment are not part of
that extraction; do not describe it as the whole source file. The FTL credit
above is separate. The license-selection index's other BDF/PCF/gzip/autofit
exceptions are not automatically accepted as selected modules.

Sources: [target](https://pdfium.googlesource.com/pdfium/+/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/BUILD.gn),
[license index](https://github.com/freetype/freetype/blob/b08a2eb0dd37f4a6c886fa5b0ecf5b3e1d27aac7/LICENSE.TXT),
[ftbase.c](https://github.com/freetype/freetype/blob/b08a2eb0dd37f4a6c886fa5b0ecf5b3e1d27aac7/src/base/ftbase.c).

### HarfBuzz and ms-use

Candidate DEPS pins `d639197ed529b05c27f38ebaab365a621d5edad5`; wrapper README names
`e6741e2205309752839da60ff075b7fa2e7cddd3`. Preserve the discrepancy.
COPYING, ms-use/COPYING, the generated use-table and generator have identical
Git blobs at these two revisions; this narrows **text** uncertainty, not
historical revision uncertainty.

The candidate path is stronger than source presence:
BUILD selects use.cc/use-table.hh; use.cc includes the table and calls its
category function; the generated preamble and generator identify ms-use
Additional.txt inputs. The Microsoft MIT ms-use supplement is therefore
conservatively pertinent to that candidate derived-data path. It is not proof
of retained DLL objects. The use-machine header is a separate Mozilla/Google
Old MIT source, not itself the ms-use input.

Sources: [BUILD](https://pdfium.googlesource.com/pdfium/+/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/harfbuzz/BUILD.gn),
[use.cc](https://github.com/harfbuzz/harfbuzz/blob/d639197ed529b05c27f38ebaab365a621d5edad5/src/hb-ot-shaper-use.cc),
[table](https://github.com/harfbuzz/harfbuzz/blob/d639197ed529b05c27f38ebaab365a621d5edad5/src/hb-ot-shaper-use-table.hh),
[generator](https://github.com/harfbuzz/harfbuzz/blob/d639197ed529b05c27f38ebaab365a621d5edad5/src/gen-use-table.py).

### OpenJPEG

The existing LICENSE/dwt/t1/ht_dec additions do not cover every candidate
holder. The new eighth supplement adds source-exact j2k.c, thread.c,
sparse_array.c and PDFium/Foxit configuration/allocator notice blocks, including
Jerome Fimes, Parvatha Elangovan, Kaori Hagihara and Even Rouault. It preserves
full grants/disclaimers, not just a reconstructed author list.

The bounded review covers leading notices in all 24 explicitly listed
fx_libopenjpeg files and 27 local headers. It is not a full-body or object-map
audit. Generated t1_luts.h/t1_ht_luts.h provenance remains open, as does
historical scope; optional JPIP/JPWL are not inferred included from names.

## Remaining conditional obligations and gate

[COMPONENT_MATRIX.md](COMPONENT_MATRIX.md) distinguishes engine identity,
producer recipe, candidate source, notice-only and unknown evidence. No
third-party historical retention or exclusion is declared proven. Candidate
root .gn disables use_llvm_libatomic; do not infer atomic runtime from a
generic Chromium default. libc++/Clang builtins/static CRT and bundled font/data
scope still require applicable file-level terms and exception mapping.
An llvm-libc-named license does not prove that mapping even if wording overlaps.

**PDFIUM PROVENANCE — PUBLIC ARTIFACT VERIFIED.**
**PDFIUM LICENSE CLOSURE — INCOMPLETE.**
Neither status clears [Windows release gates](RELEASE_GATE.md).
Independent [HEIF LGPL](../compliance/HEIF_LGPL_RELEASE_OPTIONS.md),
[MSVC](../compliance/MSVC_REDISTRIBUTION_REVIEW.md), complete EXE static inventory
and clean Windows VM gates remain.

A release owner, with qualified legal/compliance review, must accept the
component scope and completed notices/rights package. This draft is not
packager input; `distribution_status=blocked` remains. No PDFium rebuild is
needed merely to add notices, and no new DLL is authorized here.
