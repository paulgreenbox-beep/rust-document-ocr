# MSVC runtime redistribution review

Review date: 2026-10-30. **BLOCKED: technical source/version correspondence is
verified; publisher redistribution rights are not established.** This is not
a license grant. Scope: three x64 app-local DLLs in the
[PR #2 candidate](https://github.com/paulgreenbox-beep/rust-document-ocr/blob/e031efd17fa7fe8dcfd2240535692ad5ac7d9efd/reports/WINDOWS_PORTABLE_CANDIDATE_MANIFEST.json).

| Candidate file | File/product version | Bytes | SHA-256 |
| --- | --- | --- | --- |
| `msvcp140.dll` | 14.51.36247.0 | 643,512 | `7c26614e1d733892c2deac7e245ce115504b1d80592dd0a01b08e3e5a55f89ca` |
| `vcruntime140.dll` | 14.51.36247.0 | 178,616 | `d1f4225df2cd877dbf130d5668a021dce3f94118455ff5ec952061c30afc9ce7` |
| `vcruntime140_1.dll` | 14.51.36247.0 | 50,112 | `a7146c08f89fe5b04541ab507cdb59ff7b44534d4ba3c668a426c6450a03434e` |

## Technical origin evidence

Read-only evidence identifies Visual Studio Build Tools **2026**, RTW 18.7.3 /
18.7.11925.98, not Visual Studio 2022. Installer records identify x64
`Microsoft.VisualCpp.Redist.14`, `.14.Latest`, and
`Microsoft.VC.14.51.CRT.Redist.X64.base`, version 14.51.36247.
Matching Microsoft-signed files exist in the installed product's relative
`VC/Redist/MSVC/14.51.36231/x64/Microsoft.VC145.CRT` directory. The candidate
hashes match those files. The directory and DLL version labels differ as shown;
this is not hidden or treated as a mismatch.

The existing `MSVC_REVIEW_PENDING.txt` identifies installed Build Tools redist
as the candidate source. Current System32 files happen to be byte-identical:
hashes alone do **not** identify the physical copy path. Attribution rests on
the redist location, installer receipt and recorded provenance, not uniqueness
of a fingerprint. Installation and signature validity do not establish rights.

The candidate EXE imports these app-local VC DLLs and their runtime chain.
PDFium directly imports only Windows system DLLs. Its possible static CRT and
Clang/LLVM code require a separate review; the PE import table cannot inventory
that code. Static-md vcpkg libraries also must not be called static CRT merely
because their library archives are static.

## Microsoft terms reviewed

The [2026 and later REDIST list](https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution)
expressly names Build Tools and permits listed unmodified files subject to
the applicable software license. The broad VC/redist subtree includes these
retail files; debug_nonredist and preview exclusions must be respected.
The list is **conditional**, not a stand-alone license grant to every holder
of an installed file.

[VC deployment guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170)
ties distribution to licensed Visual Studio users and applicable terms. It
also discusses app-local deployment and its servicing disadvantage compared
with the central redistributable. Choosing another deployment mechanism does
not automatically settle publisher rights.

The [official license directory](https://visualstudio.microsoft.com/license-terms/)
links the exact 2026 documents below. Their complete Word text was read in
memory; no Word file or native runtime was saved/downloaded to the project.

| Official document | Observed identity | SHA-256 of retrieved document bytes |
| --- | --- | --- |
| [Diagnostic and Build Tools 2026 terms](https://visualstudio.microsoft.com/wp-content/uploads/2025/10/Visual_Studio_2026-License-Diagnostic_and_Build_Tools_ENU.docx) | 44,201 bytes; updated October 1, 2025; EULAID `VS2026_Tools_ENU.1033` | `ff03a242ec131e8353643612cdf58ae3d6cc9b51d7dce929351d140ba24a8b83` |
| [C++ V14 Redistributable and Runtime terms](https://visualstudio.microsoft.com/wp-content/uploads/2025/10/Visual-C-V14-License-Redistributable_and_Runtime_ENU.docx) | 39,553 bytes; updated October 1, 2025; EULAID `Cpp_v14_ENU.1033` | `08651651a7602fc7c0e2763de0fde1ff9f868df2780597cd1775ee9d6441c783` |

The tools terms distinguish Visual Studio Product licensing from a limited
permission to build eligible open-source dependencies. That exception is not
a blanket grant for all development or redistribution. The runtime terms
permit installation/use. Both retrieved documents contain scope restrictions
on publishing/distributing software/code. Neither installation permission nor
the name Redistributable is, alone, an affirmative publisher distribution grant.
These restrictions concern the licensed Microsoft product, not an automatic
ban on distributing applications built with it. Any applicable specific
Distributable Code grant must be read alongside the general restrictions;
the stand-alone runtime terms are not assumed to override that grant.

The release owner's reviewer must identify the **applicable agreement granting
Distributable Code rights** (or another valid permission), its relationship to
the REDIST list and these terms, the acquisition channel and eligible publisher.
No contradiction is resolved by assuming installation proves entitlement. This
is an unresolved rights question, not a ruling that distribution is impossible.
Current organization/user entitlement and any specific agreement were not
inspected or requested from private stores.

Additional official context: [Visual Studio licensing guidance](https://www.microsoft.com/licensing/guidance/Visual-Studio)
and [supported v14 redistributable versions](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170).
Guidance does not replace the agreement applicable to this publisher.

## Required release-owner decision and exit evidence

1. Identify exact license/channel and applicable affirmative distribution
   grant; have the responsible license owner confirm user/organization and
   publishing-entity eligibility. Keep private entitlement documents private.
2. Record approved unmodified file identities, the redist source and relevant
   receipt; exclude debug/preview/unapproved files.
3. Assign app-local security-servicing ownership/cadence, or separately assess
   a central-redist prerequisite. Do not change deployment automatically.
4. Finish independent PDFium static-runtime, EXE static inventory and LGPL
   reviews; perform clean-machine validation only after the payload is approved.

No DLL, EXE, model, service, environment or packaging gate was changed. No ZIP
or public binary was created. This evidence does not clear Windows portable
redistribution.
