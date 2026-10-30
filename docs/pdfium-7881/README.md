# PDFium 7881 — notice supplements and compliance review (DRAFT)

**Decision: PDFIUM LICENSE CLOSURE — INCOMPLETE. Redistribution is NOT approved.**

This is a documentation-only, low-disk review of the exact PDFium binary used by
the Windows portable release candidate in [PR #2](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/2).
It does **not** contain a DLL, .tgz, PDB, traineddata, PDF or runnable bundle.
The source-only scope of the repository's existing THIRD_PARTY_NOTICES.md remains unchanged.

## Binary identity and signed producer

| Property | Verified evidence |
| --- | --- |
| PDFium DLL | Windows x64, version `151.0.7881.0`, API `7881`, 7,211,520 bytes |
| DLL SHA-256 | `79d4676b656cfb1abcea88f9ade3b4b0826c5200382db5f4ec72a636c598c118` |
| Producer | [bblanchon/pdfium-binaries chromium/7881](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F7881) |
| Asset | `pdfium-win-x64.tgz`, SHA-256 `73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08` |
| Producer recipe | [`867b3ecc32163d715e84708c51fae5f031254985`](https://github.com/bblanchon/pdfium-binaries/tree/867b3ecc32163d715e84708c51fae5f031254985) |
| CI | [Run 27143496415](https://github.com/bblanchon/pdfium-binaries/actions/runs/27143496415), Windows x64 build successful |

On 2026-10-30, explicit verification of public attestation 30308810 against
the existing archive passed (exit 0) with pinned repository, signer workflow,
source/signer digest and GitHub-hosted runner policy. All 46 archive file
members also matched the existing bundle in local byte measurements.
See the [binary-specific draft](PDFIUM_THIRD_PARTY_NOTICES_DRAFT.md) and
[machine-readable evidence](LICENSE_EVIDENCE.json) for the reproducible policy
and public-claim versus local-measurement distinction. An attested producer workflow does **not** establish the exact
PDFium source checkout, retained linker objects, compiler/SDK versions or full
third-party license closure. The candidate current official `chromium/7881`
revision `91b9d569b34be4f38eed7b3c49b227356c3aadad` is **not** an executed
checkout receipt.

## Notice work performed

- [NOTICES.md](NOTICES.md) identifies supplementary original license texts,
  source pins, and binary-product credits; the source file notices are reproduced
  without modifying the original 16 archived notices.
- [COMPONENT_MATRIX.md](COMPONENT_MATRIX.md) records the 28 reviewed components
  and groups. Candidate graph presence and actual binary retention are separate
  facts.
- [RELEASE_GATE.md](RELEASE_GATE.md) records all unresolved checks and prohibits
  promotion of the release to `verified` based on these documents alone.

**Initial candidate omissions now supplemented:** HarfBuzz main/ms-use terms,
FreeType `fthash`, and additional OpenJPEG file-specific holders. The original
seven supplements were verified unchanged; one additional bounded OpenJPEG
notice collection was added. Full historical scope, generated-data provenance,
and runtime/license mapping for libc++, Clang builtins and Microsoft CRT
remain unresolved. The original 16 PDFium/archive notices were
reviewed; they remain preserved in the local candidate, not duplicated or
removed in this repository. The new text supplements are conservative review
material, **not** proof of the old DLL's exact retained-object graph.

## Review artifacts

- [PDFIUM_THIRD_PARTY_NOTICES_DRAFT.md](PDFIUM_THIRD_PARTY_NOTICES_DRAFT.md):
  original 16-notice index, eight supplements, credits and unresolved scope.
- [LICENSE_EVIDENCE.json](LICENSE_EVIDENCE.json): hashes, source pins and evidence levels.
- [Closure addendum](../../reports/PDFIUM_7881_LICENSE_CLOSURE_ADDENDUM.md):
  provenance, notice checks and actual CI retry evidence.
- Independent [HEIF LGPL options](../compliance/HEIF_LGPL_RELEASE_OPTIONS.md)
  and [MSVC review](../compliance/MSVC_REDISTRIBUTION_REVIEW.md).

**PDFIUM PROVENANCE — PUBLIC ARTIFACT VERIFIED.** This does not upgrade
license closure or Windows release readiness.

## Publication boundary

- Do not copy unreviewed files into a public release, mark any distribution
  status `verified`, enable a ZIP publishing gate, or merge PR #2 because of
  this documentation.
- PDFium's binary license review is separate from the Rust executable's
  statically linked **libheif/libde265 LGPL-3.0**, application-local **MSVC DLL**
  redistribution, full static dependency closure, and clean Windows VM test.
- No PDFium checkout, GN/Ninja build, runtime replacement or production OCR
  operation is required to review these license texts.
- Final scope/rights assessment is for the release owner and qualified
  legal/compliance reviewer. This packet is **not a legal opinion**.
