# Windows release licensing closure packet

Review: 2026-11-09. **WINDOWS PORTABLE RELEASE — BLOCKED: LICENSING.** This packet repairs and verifies notice materials. It does not clear rights, stage a new runtime, or authorize redistribution. The exact candidate remains local and not for distribution.

## Exact scope

| Identifier | Value |
| --- | --- |
| Windows x64 pure-HEIC candidate EXE SHA-256 | `c6cddc7516d5e44b225cde2fabb35e534016e05a7c900774b087019e538dfc23` |
| Candidate manifest SHA-256 | `7f2381924d4521feef2b4bebc0b69d9d95d99fb03e0efbed254c64fd390d6801` |
| Compiled source commit | `3185a5f5f1c79a5036b2a0c6d63e6334c4f66f26` |
| Closure PR base | `b09311eca214d6027e6d90cc41a2f52d82b18669` (`fix/pure-heic-packaging-profile`) |

The new work concerns notice files, component identity, and publisher decisions. It does not repeat the accepted OCR, runtime, or packaging validation. RC1, binaries, models, working services, and previous receipts are unchanged. Source CI for the documentation PR is recorded on that PR, not treated as binary qualification.

## 1. PDFium 151.0.7881.0

- **Proved:** The DLL is the 7,211,520-byte AMD64 artifact, SHA-256 `79d4676b656cfb1abcea88f9ade3b4b0826c5200382db5f4ec72a636c598c118`, identified by the matching [public producer asset](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F7881) and the retained [archive and attestation evidence](../pdfium-7881/LICENSE_EVIDENCE.json). The 16 producer texts match the corresponding archive members, including the original encoding.
- **Fixed:** Corrected the upstream IJG link and restored the exact sibling name `README.ijg` referenced by the producer's JPEG notice without changing its bytes. Retained the eight existing supplements, added three pinned supplemental files (OpenJPEG generated-table source headers and the libc++ and compiler-rt licenses/exceptions), and added the FreeType and IJG product credits. The physical tree has 28 inventoried files, including its recipient index; all hashes, sizes and 29 internal links were verified.
- **Ready:** [Recipient NOTICE and files](../pdfium-7881/recipient/NOTICE.md), [per-file inventory](../pdfium-7881/RECIPIENT_FILES.json) (SHA-256 `ba8f4eb035d9d8211a9836f6237ab8d286d09d4fb70baf8c8f41468ec7c4a097`), [source/collector comparison and staging map](../pdfium-7881/RECIPIENT_PACKET.md).
- **Still required:** Historical PDFium source/DEPS resolutions, actual GN flags and patches, toolchain/CRT input and retained-object or equivalent evidence for bundled code, fonts and generated tables. These facts are not recoverable from a version, exported symbols, producer recipe, notice presence, or archive attestation alone. Reviewer must record a supported closure decision for this exact artifact or request separately proven replacement provenance. **Public artifact verified; PDFium license closure incomplete.**

## 2. EG79 derived model

- **Proved:** The candidate retains the unchanged 15,400,601-byte `eng_technical_eg79_v1.traineddata`, SHA-256 `7ceee3f56f290c3b90c1670d8e0c7dd86e344759ff03915407c53369f1c8437c`. The selected training record identifies fine-tuning from the pinned [tessdata_best English base](https://github.com/tesseract-ocr/tessdata_best/tree/e2aad9b983032bb1beff9133104a67cdbb87ca4d), with its [Apache-2.0 license](https://github.com/tesseract-ocr/tessdata_best/blob/e2aad9b983032bb1beff9133104a67cdbb87ca4d/LICENSE). The separately packaged tessdata_fast license was not bound to EG79.
- **Fixed:** Supplied a derivative modification/attribution notice and the exact base license; the component's original empty notice list is recorded and proposed bindings are explicit. No model bytes, training inputs, or private annotations were added to this repository.
- **Reported by the owner, not independently verified:** EG79 was fine-tuned on the owner's own scanned documents and independently prepared materials, with real raster text from scans identified as the most useful material. The early synthetic Arial PDF is recorded separately as a test artifact; its use in fine-tuning is unconfirmed. No font files are intended for distribution. See the [origin statement](eg79/OWNER_CONFIRMATION.md#training-and-test-data-origin-reported-by-the-project-owner); permissions for document content and redistribution of the weights remain open.
- **Ready:** [Model evidence and placement](eg79/README.md), [recipient NOTICE](eg79/NOTICE.txt), [base license](eg79/LICENSE.tessdata_best.txt), [unsigned owner confirmation](eg79/OWNER_CONFIRMATION.md).
- **Still required:** A rights holder or authorized publisher must confirm authority to distribute these exact derived weights and the right to use the relevant training materials for this purpose, including any output restrictions and required credits. Retain substantiation privately. Apache-2.0 for the base does not confer rights to unrelated data or determine the publisher's rights in the fine-tune.

## 3. Microsoft CRT

- **Proved:** `msvcp140.dll`, `vcruntime140.dll`, and `vcruntime140_1.dll` each match their manifest and the official installed x64 `Microsoft.VC145.CRT` REDIST file byte-for-byte. Each is version 14.51.36247.0; all three signatures validated on the review host. The [official REDIST list](https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution#visual-c-runtime-files) identifies the covered installed subtree under the applicable terms; technical file correspondence is not an entitlement.
- **Fixed:** Replaced generic uncertainty about the three file identities with exact hashes, product and version evidence, the applicable REDIST reference, and a publisher-specific confirmation. The candidate's pending marker remains unchanged.
- **Ready:** [REDIST evidence matrix](crt/REDIST_EVIDENCE.md) and [unsigned publisher confirmation](crt/PUBLISHER_CONFIRMATION.md).
- **No-cost route reviewed:** [MSVC free-release decision](crt/MSVC_FREE_RELEASE_DECISION.md) identifies Community's affirmative grant and its conditions, the limits of the separate Build Tools and runtime agreements, and a user-installed CRT alternative for the unchanged EXE. Both routes require substantiating rights for the existing build and retained static code; neither is approved.
- **Still required:** Identify the entity that will distribute the unmodified files, its qualifying agreement/channel and applicable redistribution clause, downstream conditions, and servicing responsibility. Assess static MSVC/SDK inputs separately. Do not sign based on file presence or valid Authenticode alone.

## 4. heic-rs and HEVC

- **Proved:** The modified `heic-rs 0.1.4-ocr.1` has four existing candidate texts, all byte-matched to the pinned derivative and manifest. The MIT, Apache-2.0, and original NOTICE texts additionally match the [published 0.1.4 archive](https://static.crates.io/crates/heic-rs/heic-rs-0.1.4.crate). The fourth is the actual derivative change record. No missing original decoder notice was found.
- **Fixed:** Added a recipient index naming all four exact files and a separate HEVC patent worksheet. No decoder or feature changes.
- **Ready:** [Recipient index and exact hashes](heic-rs/RECIPIENT_NOTICES.md), [HEVC legal worksheet](heic-rs/HEVC_PATENT_REVIEW.md).
- **Still required:** Publisher records its applicable MIT/Apache-2.0 option, authority for local modifications, and final notice placement. Legal reviewer records scope, territories, distribution/use, applicable grants or patent obligations, and a decision for this HEVC decoder. The software licenses, pure Rust implementation, and absence of libheif/libde265 do not decide HEVC patent coverage.

## 5. Other dependencies and project code

- **Proved:** [Component matrix](windows-dependencies/COMPONENT_MATRIX.json) reconciles all 246 candidate rows with 223 locked registry archive matches and 445 existing notice file matches. It identifies 170 Rust and 16 vcpkg component rows with matching final linker inputs; 55 rows are selected-graph evidence only. Separately records 13 verified Rust sysroot archives, MSVC/SDK inputs, and temporary-object gaps. Link inputs are not proof of every retained member.
- **Fixed:** Added texts for ten missing registry notice bindings and two separately attributed pieces of code (XMLJSON-rs adaptation and libdeflate CRC tables). Restored native JPEG's missing version-specific IJG text, added liblzma's 0BSD text, Rust standard-library notices, and file-level notices for libarchive (42 groups) and native OpenJPEG (19 groups). Clarified Leptonica's two-condition notice without rewriting a historic license declaration. Traced the project MIT text and technical stamp/table source blobs to the compiled revision.
- **Ready:** [Recipient additions](windows-dependencies/NOTICE.md), [reconciliation, upstream sources and evidence limits](windows-dependencies/README.md), [project-code provenance](windows-dependencies/PROJECT_CODE_PROVENANCE.md).
- **Still required:** Confirm project/source contribution authority, record applicable alternative-license elections, resolve any other retained code not covered by available records (notably native file-specific and compiler/SDK inputs), and approve the final manifest's actual notice bindings. An inventory or a copyright line alone is not a chain-of-title determination.

## Final assembly and independent gates

After the [owner decisions](../../OWNER_RELEASE_DECISIONS.md) are resolved, a separately authorized packaging step must assemble and verify, without removing existing applicable texts:

| Future location | Source or action |
| --- | --- |
| `licenses/pdfium/` | Copy the [recipient tree](../pdfium-7881/recipient/NOTICE.md) according to the [placement map](../pdfium-7881/RECIPIENT_PACKET.md); verify actual package hashes and local links. |
| `licenses/eg79/` | Bind both EG79 texts from the [model evidence](eg79/README.md) to the unchanged model hash. |
| `licenses/heic-rs-NOTICES.md` | Copy the [index](heic-rs/RECIPIENT_NOTICES.md) alongside its four existing companion files. |
| `licenses/windows-dependencies/` | Copy the [NOTICE and entire notices subtree](windows-dependencies/NOTICE.md); bind relevant texts to the exact component entries and keep original notices. |
| CRT and project MIT | Keep the project MIT text and use the publisher-approved CRT terms and notices; unsigned confirmation forms are not recipient license terms. |

Record the final manifest SHA-256 and approval evidence for the exact bytes; a draft document or a file present in this repository does not satisfy a binary release gate. Keep the existing fail-closed distribution checks. No ZIP or GitHub Release is authorized by this packet.

- Windows Sandbox: **NOT RUN — DEFERRED BY OWNER**.
- Separate clean Windows x64: **NOT RUN — DEFERRED BY OWNER**.

These deferrals are not pass/fail outcomes, do not qualify portability, and do not replace a clean-environment receipt where the release gate requires one.
