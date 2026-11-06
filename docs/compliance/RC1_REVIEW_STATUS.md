# Practical RC1: current evidence and review limits

This follow-up applies to the existing local RC1 built from
`1af284f2cbf1201972b02060ace287b27150382b`, EXE SHA-256
`c305d60ec3861ebb5cf4e9532f346559be4ebc1b5989da31412c1c36f71f8cb4`.
It is not an approval to distribute. The EXE, manifest and historical receipts
are not changed by this documentation.

Windows Sandbox and separate clean Windows x64 are each
**NOT RUN — DEFERRED BY OWNER**. Existing developer-host smoke does not
qualify either. See [the owner decision](../PRACTICAL_RC1.md#current-owner-decision-for-rc1).

## Verified inputs; remaining decisions

| Area | Verified for the current candidate | Open before binary distribution |
| --- | --- | --- |
| PDFium 151.0.7881.0 | Exact DLL, 16 producer texts and eight supplements match the reviewed evidence. The staged draft also contains the FreeType and Independent JPEG Group credits. | Confirm retained-component/NOTICE coverage, including unresolved LLVM/static CRT/fonts, and approve the final recipient notice text. Correct the draft's relative notice links for the packaged layout and place applicable credits in recipient-facing documentation. |
| OpenJPEG generated tables | The two LUT headers in the candidate PDFium source match OpenJPEG 2.5.4 at the pinned commit below; the generation and inclusion paths are documented. | Source correspondence does not prove which objects were retained in the historical DLL or that the current notices are sufficient. |
| libheif 1.23.1 / libde265 1.1.1 | Staged copyright files include LGPLv3 and GPLv3 texts. Both exact upstream archives and all 11 captured port/patch files were rechecked. The RC1 map names members of both static libraries; a separate modified-libde265 probe linked with frozen HEIF passed. | Prominent recipient notice, delivery of corresponding source/application materials, a usable full OCR recombination route under applicable terms, and independent reproduction remain open. The standalone probe is not that proof. |
| MSVC app-local CRT | The three final EXE imports match the staged unmodified x64 REDIST files, version 14.51.36247.0, from the recorded Visual Studio Build Tools 2026 retail Release installation. | Identify the actual publisher's qualifying license and affirmative distribution grant and approve servicing. Technical file provenance and the conditional REDIST list alone do not establish those rights. |
| EG79 | The selected training run's English base matches the upstream `tessdata_best` 4.1.0 blob at the immutable commit below. The selected derived weights match the local RC1 weights. | Applicable Apache license/attribution and modification notice, authorization for the exact derived weights, and a documented rights review of real crop/label and font-rendered inputs. Do not publish the training corpus with RC1. |

### Source references and evidence boundaries

- PDFium: [pinned notice packet](../pdfium-7881/RELEASE_GATE.md) and
  [producer release](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F7881).
  The prior archive/attestation evidence is historical; no new binary rebuild
  or attestation is implied by checking the staged notice bytes.
- [Candidate PDFium OpenJPEG pin](https://github.com/chromium/pdfium/blob/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/libopenjpeg/README.pdfium):
  `6c4a29b00211eb0430fa0e5e890f1ce5c80f409f`. The classic `t1_luts.h` and HT
  `t1_ht_luts.h` Git blobs are `1a5e7844f761b602f0a61f1205ba9d96d0e0a94d` and
  `f39d5d05777a1628e2e4603b55be2541582b67fb`. See the pinned
  [LUT generator](https://github.com/uclouvain/openjpeg/blob/6c4a29b00211eb0430fa0e5e890f1ce5c80f409f/src/lib/openjp2/t1_generate_luts.c#L316-L320)
  and [HT generator](https://github.com/uclouvain/openjpeg/blob/6c4a29b00211eb0430fa0e5e890f1ce5c80f409f/src/lib/openjp2/t1_ht_generate_luts.c).
- HEIF and de265: [exact libheif COPYING](https://github.com/strukturag/libheif/blob/v1.23.1/COPYING),
  [exact libde265 COPYING](https://github.com/strukturag/libde265/blob/v1.1.1/COPYING),
  and [static-route options](HEIF_LGPL_RELEASE_OPTIONS.md). A static combined
  work needs the applicable LGPL conditions reviewed, not merely the license
  files or a library-only demonstration.
- MSVC: [2026 REDIST list](https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution#visual-c-runtime-files),
  [Build Tools terms](https://visualstudio.microsoft.com/license-terms/vs2026-ga-diagnostic-buildtools/),
  and [earlier technical review](MSVC_REDISTRIBUTION_REVIEW.md). The evidence
  does not establish publisher entitlement. An external Microsoft runtime
  prerequisite would be a separate package decision, not a silent substitution
  or permission to rehost the installer.
- EG79: [upstream English base](https://github.com/tesseract-ocr/tessdata_best/blob/e2aad9b983032bb1beff9133104a67cdbb87ca4d/eng.traineddata),
  blob `176dc3220de7db34d3b3aecbfa42043a6038348b`, SHA-256
  `8280aed0782fe27257a68ea10fe7ef324ca0f8d85bd2fd145d1c2b560bcb66ba`.
  [License at the same revision](https://github.com/tesseract-ocr/tessdata_best/blob/e2aad9b983032bb1beff9133104a67cdbb87ca4d/LICENSE).
  This identifies equivalent source bytes, not the original download URL or
  a grant for the modified weights. See [Apache-2.0 §4](https://www.apache.org/licenses/LICENSE-2.0)
  and [Microsoft's font FAQ](https://learn.microsoft.com/en-us/typography/fonts/font-faq);
  rendered-output guidance does not expressly settle model training or weights.

## Open static-review findings

The existing local comparison is retained at its original scope. Static review
does not replace it or create a new runtime result.

- `src/native/pdfium_source.rs` refines ownership for glyph grouping, but its
  projected `Word` does not carry the chosen owner into `src/table_grid.rs`.
  Rounded projected centers can land on the other side of the original raster
  separator. Review a synthetic end-to-end cell assignment regression before
  claiming refined ownership is preserved through export. Cross-grid seam
  duplicates are reported, not prevented by the metric.
- `scripts/build-windows-portable.ps1` records a baseline manifest hash, but
  `scripts/smoke-windows-portable.ps1` accepts the comparison directory separately
  without checking its exact contents against that manifest. Bind the approved
  baseline files before reopening ZIP publication.
- The ZIP branch writes the final archive name before the extracted-copy smoke.
  A later failure can leave that name behind. Use a provisional output and
  promote it only after verification in a future packaging change.

No release gate, source code, binary or payload file was changed by this review.
