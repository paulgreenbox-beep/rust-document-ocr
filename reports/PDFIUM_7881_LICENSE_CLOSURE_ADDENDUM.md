# PDFium 7881 license closure addendum

Date: 2026-10-30. Documentation-only continuation of
[PR #3](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/3), branch
`docs/pdfium-7881-notice-packet-2026-10`.

**PDFIUM PROVENANCE — PUBLIC ARTIFACT VERIFIED.**
**PDFIUM LICENSE CLOSURE — INCOMPLETE.**
**WINDOWS PORTABLE RELEASE — BLOCKED: LICENSING.**

No redistribution approval, portable ZIP or runtime promotion follows from
this documentation. The release owner must resolve rights/scope with qualified
legal/compliance review. Working OCR, PDFium, EXE and models remain unchanged.

## Repository state and change boundary

Before this continuation, live GitHub checks showed:

| Ref | Observed state |
| --- | --- |
| `main` | `9a99b37102c7bb21c9f358bafd69f95f19a626f2` |
| PR #2 | OPEN, not merged, `e031efd17fa7fe8dcfd2240535692ad5ac7d9efd` |
| PR #3 | OPEN, not merged, `1d7611195d4cd1e6dc54103b5d562cbbe672fdcc` |

Changes continue the existing PR #3, not a duplicate PR. The resulting commit
is the commit containing this addendum; its exact SHA and fresh push/PR CI
receipts are recorded in the PR conversation/final handoff, not invented as
a self-referential file hash. [PR commits](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/3/commits)
and [checks](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/3/checks)
are the public live references. A baseline PASS must not be inherited by a new
HEAD before its checks complete.

Updated existing documents: PDFium README, COMPONENT_MATRIX, NOTICES and
RELEASE_GATE. New documents: binary-specific notices draft, compact JSON
evidence, this addendum, HEIF LGPL options and MSVC review. One new text-only
OpenJPEG supplement is added; all seven pre-existing supplements are unchanged.
No `src/`, Cargo, build.rs, runtime, model, workflow or packager changes.

## Public artifact verification versus executed-source history

DLL: Windows AMD64, 151.0.7881.0 / API 7881, 7,211,520 bytes, SHA-256
`79d4676b656cfb1abcea88f9ade3b4b0826c5200382db5f4ec72a636c598c118`.
The existing TGZ is 3,733,154 bytes, SHA-256
`73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08`.
These match the [producer release](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F7881)
and [asset metadata](https://api.github.com/repos/bblanchon/pdfium-binaries/releases/tags/chromium%2F7881),
release 336103186 / asset 441936973. No new runtime archive was downloaded.

The exact [attestation 30308810](https://github.com/bblanchon/pdfium-binaries/attestations/30308810)
was obtained as public JSON and explicitly cryptographically verified against
the existing TGZ, exit **0**, with repository, signer-workflow, source/signer
digest and GitHub-hosted-runner constraints. It identifies:

- OIDC issuer `https://token.actions.githubusercontent.com`;
- signer `bblanchon/pdfium-binaries/.github/workflows/build-all.yml@refs/heads/master`;
- producer source/signer digest `867b3ecc32163d715e84708c51fae5f031254985`;
- [run 27143496415, attempt 1](https://github.com/bblanchon/pdfium-binaries/actions/runs/27143496415/attempts/1);
- matching subject `pdfium-win-x64.tgz` with the stated SHA-256.

The attestation's resolved dependencies identify the **producer recipe**, not
the actual PDFium checkout, DEPS or compiler. The claim that all 46 archive
regular files match the existing extracted bundle is a **local measurement**:
read members in memory, compare size/hash per relative filename; no extraction,
execution or public upload. It is not an assertion signed by upstream about
the local filesystem. The matching DLL is a copy of the public artifact, not
an assumed unique user build.

Known producer patches are pinned shared-library/export/resource changes,
public C++ include-path changes, and Windows resource-compiler changes. The
nine explicit archived GN args match the recipe. That does not prove all
resolved flags or the exact history of the executed source tree. Additional
user patches are not invented. Candidate PDFium
`91b9d569b34be4f38eed7b3c49b227356c3aadad` is not claimed as that receipt.

## Notice review now performed

The [draft](../docs/pdfium-7881/PDFIUM_THIRD_PARTY_NOTICES_DRAFT.md) preserves
the index, hashes and pinned text/extraction origins of all 16 producer
notices. Original bytes are not rewritten. They remain external archived
texts, not a DLL or TGZ added to the repository. The builder MIT file is not
an MIT-only engine license. Required FreeType and IJG product credits are
explicit in the draft.

All seven existing source supplements were verified: three whole source
license files are byte-identical; four are exact complete license-comment
extractions with boundaries recorded in [LICENSE_EVIDENCE.json](../docs/pdfium-7881/LICENSE_EVIDENCE.json).
This does not certify every source file or object in the historical DLL.

### Narrower candidate coverage gaps addressed

- FreeType: ftbase.c includes the separately MIT-licensed fthash.c. The
  complete existing fthash grant is preserved; FTL/product credit remains
  separate from the hash-code license.
- HarfBuzz: candidate production sources reach use.cc, generated use-table,
  and ms-use inputs. Thus the separate Microsoft MIT notice is supported by
  a derived-data chain, not merely source-tree presence. Main/ms-use COPYING,
  generated table and generator have equal Git blobs at candidate DEPS
  `d639197ed529b05c27f38ebaab365a621d5edad5` and wrapper README revision
  `e6741e2205309752839da60ff075b7fa2e7cddd3`. The discrepancy and actual
  historical retention remain unresolved.
- OpenJPEG: review extended to the leading notices of 24 selected files and
  27 local headers. An eighth supplement preserves additional full BSD-2
  comments and PDFium/Foxit notice pointers, covering further candidate
  holders not in the first four OpenJPEG supplements. Generated LUT provenance,
  optional branches, full-body attribution and historical retention remain open.

The evidence vocabulary is now explicit: CONFIRMED_IN_BINARY,
PRODUCER_RECIPE_EVIDENCE, CANDIDATE_SOURCE_EVIDENCE, NOTICE_ONLY, UNKNOWN,
NOT_INCLUDED_PROVEN. No third-party historical retained-object map is
invented; no exclusion receives NOT_INCLUDED_PROVEN from defaults or imports.

### Still not closed

ICU's full composite Unicode/data/build-script document, JPEG/IJG/API/SIMD
terms, Zlib file-level notices, AGG 2.3's actual grant, applicable Apache NOTICE,
vendored changes and bundled fonts/data remain scope-sensitive. GPL-labelled
ICU build-support material does not by itself make runtime GPL. Candidate
libc++/Clang builtins/static CRT mapping is not cleared by a file named
llvm-libc.txt. Candidate root .gn disables LLVM libatomic; Windows libc++ does
not automatically imply libc++abi/libunwind. No blanket copyleft-free claim
is made.

The missing historical checkout is not itself a licensing requirement to
rebuild. Adding correct notices needs no PDFium rebuild. If uncertainty cannot
be accepted, a new reproducible build is a separate future decision and would
not prove the old build's source.

## Push CI failure and bounded retry

At old HEAD `1d7611195d4cd1e6dc54103b5d562cbbe672fdcc`:

| Event | Run/job | Result |
| --- | --- | --- |
| PR | [41072348342](https://github.com/paulgreenbox-beep/rust-document-ocr/actions/runs/41072348342) / 124119419658 | PASS |
| Push attempt 1 | [41072156283](https://github.com/paulgreenbox-beep/rust-document-ocr/actions/runs/41072156283/attempts/1) / 124118767501 | FAIL launching linker |
| Push attempt 2 | [same run, attempt 2](https://github.com/paulgreenbox-beep/rust-document-ocr/actions/runs/41072156283/attempts/2) / 124212898829 | PASS |

The full public failed-job log was read. Formatting, fixture checksum and
`cargo check --offline --locked --all-targets -j 1` passed. The lib-test link
step could not **execute** `link.exe`; Windows reported error 433, a device
that does not exist. This is process creation, not a linker-reported unresolved
symbol or failing Rust assertion. The log names the hosted VS 2022/MSVC14.44
tool, distinct from local runtime redistribution evidence.

The authorized failed-job retry passed unchanged source/workflow and every
existing check, including 218 successful source tests and 3 ignored diagnostics,
then Clippy. Both attempts used windows-2022 image 20261025.346.1. This supports
a transient runner/environment launch failure, not a demonstrated source
regression. The underlying device/runner fault cannot be identified more
precisely from those logs; no cache corruption or exact hardware cause is
asserted. No tests were disabled, no workflow weakened, no cache deleted.
New-HEAD checks must still be observed separately after the documentation push.

## Independent EXE LGPL and Microsoft gates

[HEIF_LGPL_RELEASE_OPTIONS.md](../docs/compliance/HEIF_LGPL_RELEASE_OPTIONS.md)
records confirmed static archives/source hashes, actual core-only features,
static-library/dynamic-CRT distinction, and three unimplemented options:
usable static rebuild/relink package, replaceable dynamic HEIF, or a deliberately
HEIF-disabled native build. The last is not an existing feature switch.

A concrete manifest evidence defect was found: nested HEIF installed_status
fields say version 1.5.7 / same ABI, inconsistent with direct libheif1.23.1 and
libde2651.1.1 status/ABI records. It is documented, not silently corrected in
PR #2. A release inventory needs separately reviewed collector repair and
fresh evidence; it cannot be declared verified.

[MSVC_REDISTRIBUTION_REVIEW.md](../docs/compliance/MSVC_REDISTRIBUTION_REVIEW.md)
records the three planned app-local DLL hashes, valid signatures, VS2026
redist/installer correspondence, and conditional official REDIST terms. Full
Build Tools/runtime license text was examined in memory, not just a summary
page. The responsible publisher's applicable affirmative distribution grant
and entitlement remain unverified. Installation or matching System32 bytes
does not establish these rights. App-local DLLs and PDFium static CRT are
different boundaries.

## Minimal remaining blockers and owner actions

1. Release owner/reviewer accepts a documented PDFium scope, resolves material
   generated-data/file/runtime uncertainty, and approves a complete physical
   notices packet. Draft links are not the packet's required texts.
2. Choose and independently demonstrate an LGPL source/recombination or
   suitable shared-library route; alternatively explicitly authorize and test
   a HEIF-disabled variant. Do not auto-change features or copy DLLs.
3. License owner confirms exact Microsoft use/distribution agreement and
   publisher eligibility; release owner assigns servicing responsibility.
4. Correct the release inventory evidence mapping through separate review,
   finish complete EXE static closure, and run clean Windows x64 VM acceptance.
5. Only then consider a separate release-gate decision for PR #2. No merge,
   release, ZIP or distribution-status promotion is authorized by this work.

## Verification and preservation

Cheapest sufficient local path: pinned source/notice comparisons, SHA-256,
JSON/relative-link/privacy validation and documentation-only diff review.
Existing source CI remains required for every changed HEAD. No local Cargo,
PDFium or native build was run for a text-only change.

No PDFium rebuild/checkout, binary/model upload, production OCR invocation,
runtime replacement, model edit, global environment change or data removal
was performed. No raw CI/internal log or signed temporary download URL is
included in the commit. Original seven supplements and all protected source/
runtime payloads are preserved. PR #2, main and the manual ZIP gate remain
unchanged.
