# MSVC and CRT: no-cost Windows release decision

**Decision: HOLD for the existing candidate.** A qualifying individual can use
Visual Studio Community 2026 without a paid subscription. That is a practical
license route, not evidence that the existing build or its proposed distribution
is already covered. No purchase, installation, rebuild, runtime change, or binary
publication is requested by this decision.

Scope: free public GitHub distribution by an individual of the exact Windows x64
OCR executable SHA-256
`c6cddc7516d5e44b225cde2fabb35e534016e05a7c900774b087019e538dfc23`,
source commit `3185a5f5f1c79a5036b2a0c6d63e6334c4f66f26`, and existing
manifest SHA-256 `7f2381924d4521feef2b4bebc0b69d9d95d99fb03e0efbed254c64fd390d6801`.
Tesseract, EG79, stamp/table recognition, pure-Rust HEIC, PDFium, NativeText and
JSON/DOCX/XLSX stay in scope. This is an engineering review of the published
terms, not a license grant or a determination of the owner's entitlement.

## 1. Which Microsoft grant applies?

The documents below were obtained from the [official license directory](https://visualstudio.microsoft.com/license-terms/)
and read in memory. All three English agreements identify October 1, 2025 as
their last update. The installer's actual edition, channel, terms and any
separate grant still control the owner's situation.

| Official terms | Relevant provisions and result |
| --- | --- |
| [Visual Studio Community 2026 agreement](https://visualstudio.microsoft.com/wp-content/uploads/2025/10/Visual_Studio_2026-License-Community_ENU.docx), `VS2026_Comm_ENU.1033` | **§2(a)–(b)** permits an individual to develop and test their own applications, including for sale; a free public application is within that described use. **§7(c), Build Devices and Visual Studio Build Tools** covers its stated device/use conditions. **§8(a)(i)** is the affirmative grant for listed object code; **§8(b)–(c)** impose downstream terms, significant primary functionality, indemnification (with the stated exception) and distribution restrictions. This is not the organizational license under §2(c). [Microsoft's Community page](https://visualstudio.microsoft.com/vs/community/) identifies the edition as free. |
| [Diagnostic and Build Tools 2026 agreement](https://visualstudio.microsoft.com/wp-content/uploads/2025/10/Visual_Studio_2026-License-Diagnostic_and_Build_Tools_ENU.docx), `VS2026_Tools_ENU.1033` | **§1(a)–(c)** treats it as a supplement to a valid Community, Professional or Enterprise license; check the actual Visual Studio or VS Code development workflow. **§1(d)–(e)** without that license covers compiling/building reasonably required, OSI-licensed **third-party C++ dependencies**, with only narrowly permitted compile-enabling modifications. It does not cover linking the author's own Rust application merely because it is open source. **§2, Build Devices and Visual Studio Build Tools** limits permitted devices/use. **§4, Scope of License** restricts distribution of the Microsoft software and its code. No separate affirmative Distributable Code provision was found in this agreement. [Microsoft's licensing guidance, Example 3](https://www.microsoft.com/licensing/guidance/Visual-Studio#visual-studio-build-tools) distinguishes the dependency from other portions of the application. |
| [Standalone v14 Redistributable and Runtime agreement](https://visualstudio.microsoft.com/wp-content/uploads/2025/10/Visual-C-V14-License-Redistributable_and_Runtime_ENU.docx), `Cpp_v14_ENU.1033` | **§1** grants installation and use to the recipient; **§4** limits redistribution of the Microsoft software/code. User installation from Microsoft and publisher redistribution are distinct. This is not a license for the compiler or an affirmative grant to rehost the installer. |

The Community document was 69,066 bytes, SHA-256
`02ac07637f4a513a807cd3721c7620433fc483bc049d25784b0c10c54b05e1fb`.
The other two agreement hashes match the [previous exact documents](../MSVC_REDISTRIBUTION_REVIEW.md#microsoft-terms-reviewed).

The [2026 REDIST list, Distributable Code and Visual C++ Runtime Files](https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution#visual-c-runtime-files)
covers the retail `VC/redist` subtree subject to a valid applicable license;
it excludes debug/preview payloads and requires the files to stay unmodified.
The [exact three-file evidence](REDIST_EVIDENCE.md) verifies correspondence
to that subtree. The list's introduction names Build Tools alongside
Community, but its permission is conditional on the product terms. Do not
use the list to fill the missing affirmative grant in the separately
retrieved Build Tools terms. Read it together with Community §8 or another
applicable affirmative grant covering the exact files and publisher.
[Microsoft's deployment guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170#redistributable-files-and-licensing)
also conditions app-local redistribution on the applicable license.

For app-local distribution, verify the approved way external users and
distributors will agree to Microsoft-code protections under §8(b). A public
GitHub ZIP plus an MIT project license is not evidence of that agreement.
Keep third-party conditions scoped to the Microsoft code, not an implied
change to the project's MIT license. Review compliance with the agreements'
trade-law provisions (Community §14) and actual destination, end-user and end-use restrictions
before approving an unrestricted download. Free price or availability on
GitHub is not proof of worldwide permission. See [Microsoft Exporting](https://www.microsoft.com/en-us/exporting).

## 2. Can this existing EXE use a system-installed CRT?

The current EXE rehash matched the stated SHA-256. Its retained build receipt
records `x86_64-pc-windows-msvc` and the stable VS Build Tools 2026 MSVC
14.51.36231 directory, with `link.exe` file version **14.51.36248.0**.
That is not the REDIST/DLL version. PE inspection found the following:

| Runtime | Direct imports or relevant dependency |
| --- | --- |
| `ocr-pipeline.exe` | `msvcp140.dll`, `vcruntime140.dll`, `vcruntime140_1.dll`; 11 UCRT API-set DLL names, plus ordinary Windows system imports. All three CRT DLLs are direct imports; no delay-load import descriptors were found. |
| App-local CRT | All three x64 DLLs are **14.51.36247.0** and byte-identical to the same-named system-installed DLLs on the review host. Its x64 runtime registration reports **v14.51.36247.00**, installed. |
| `pdfium.dll` | Loaded dynamically by the program. Its direct PE imports are `advapi32.dll`, `gdi32.dll`, `kernel32.dll` and `user32.dll`, with no delay-load descriptors. This does not establish absence of static runtime code. |

An independent `llvm-readobj` import/export comparison on the review host
found all **142** EXE import names in the corresponding System32 CRT exports:
120 in `msvcp140.dll`, 21 in `vcruntime140.dll`, one in
`vcruntime140_1.dll`. This checks named exports, not actual Windows loader
behavior or forwarded destinations on a clean host.

[Microsoft's v14 compatibility rule](https://learn.microsoft.com/en-us/cpp/porting/binary-compat-2015-2017?view=msvc-170#restrictions-on-binary-compatibility)
requires a Redistributable at least as recent as the newest tools used by the
components. For this candidate, use the
[latest supported official x64 v14 Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170#latest-supported-redistributable-version)
that covers the VS 2026 v145 toolset used for this build. **14.51.36247.0 is
the verified reference for the known working DLL set, not the proven lowest
compatible version.** Do not infer a minimum redistributable package revision
from the numerical file version of `link.exe`; older packages were not tested.

The EXE imports the libraries by name, not an app-local absolute path. With
the correct centrally installed x64 runtime and supported Windows, keeping
the exact EXE is technically plausible without relinking. The same-byte
system comparison is static evidence, not a no-app-local startup or clean-host
smoke result. No installer was fetched and no DLL was copied or removed.

**Static Microsoft code does not go away.** The exact EXE's saved link map
(SHA-256 `a55fd0a2c9a3d10810b1dd2013ebc39dc8a4468560033da04730f905a17eff15`)
maps `mainCRTStartup` to `msvcrt:exe_main.obj`,
`__security_cookie` to `msvcrt:gs_cookie.obj`, and
`__guard_memcpy_fptr` to `vcruntime:softmemtag.obj`. [Microsoft documents](https://learn.microsoft.com/en-us/cpp/c-runtime-library/crt-library-features?view=msvc-170#c-runtime-lib-files)
static CRT startup even with a dynamic UCRT. These are concrete
mapped contributions, not just library names on the link command. Confirm
their applicable distribution terms and remaining SDK/static runtime scope
separately from the three DLLs. User installation does not resolve the
historical Build Tools license question either.

## 3. Choice and practical action

| Route | Rights needed | Package and validation impact |
| --- | --- | --- |
| **Free Community + app-local CRT** | Document qualifying Community and covered tool use; record an affirmative §8/REDIST grant for the exact files and approved downstream assent, indemnity, and scope. | No technical rebuild or DLL replacement. Keep the byte-identified retail files; add approved terms/notices and servicing owner; generate a new manifest/approval record. Confirm final rights and package gates on the exact payload. |
| **User installs official x64 v14 runtime** | Still document lawful build/tool use and rights for retained Microsoft/SDK code and all other components. No publisher copy of the three app-local DLLs or installer in this profile. Users obtain and accept Microsoft's installer directly. | New package profile and manifest, unchanged EXE. Requires external-runtime PE checks and targeted smoke before release; not a self-contained portable install. |

**Recommended next action:** preserve the EXE. Privately record whether the
publisher actually held an applicable free Community (or equivalent) license
when the retained build was made, under which edition/channel and terms, on
what eligible build device, and whether the Visual Studio/VS Code and Build
Tools workflow was covered. Evidence of installation or no separate purchase
does not answer that question. Record the separately applicable grant to
publish this exact EXE, including retained CRT/SDK contributions. Use the
[publisher confirmation](PUBLISHER_CONFIRMATION.md) and keep personal records,
agreements and signatures private.

- If that coverage and the §8 conditions for the app-local DLLs are documented
  and implementable on the intended GitHub channel, keep the existing app-local
  layout. No reviewed clause was found that requires a rebuild merely to
  document already-applicable rights.
- If the EXE's rights are documented but the app-local grant/assent or servicing
  arrangement is not, prefer the **user-installed CRT** profile for GitHub.
  Do not rehost Microsoft's installer. This avoids publishing these three
  DLL copies; it does not make the EXE Microsoft-code-free.
- If historical coverage or authority for this EXE cannot be substantiated,
  ask Microsoft or a qualified reviewer to resolve the exact artifact and
  Build Tools/Community fact pattern. Accepting Community now establishes
  only the rights it actually grants; no retroactive cure was identified.
  Hold the existing artifact until resolved. For a later build, the owner
  can acquire and accept the no-cost Community terms and document a covered
  workflow **before** building. A new build, if genuinely required, needs
  separate authorization, a new hash and validation; none was started here.

### External-runtime profile, only if separately authorized

The current [PE closure checker](../../../scripts/portable-pe.ps1) deliberately
requires the three VC DLLs app-locally. Do not remove files from the verified
candidate, relabel these dependencies as standard Windows DLLs, or weaken
the existing gate. Use a separate staging directory and explicit external CRT
profile. A new manifest should classify these exact import names as an
external prerequisite (Microsoft, x64, supported v14 toolset coverage,
official source), not as shipped files or a generic system DLL allowlist.
Exclude the three DLLs and any installer from that new payload only; preserve
the EXE, model, PDFium and OCR assets and verify their unchanged hashes.

Before any release of that profile:

1. Check direct/delay and transitive PE resolution, signature, architecture,
   supported runtime version and actual loaded DLL paths on an authorized
   validation host; fail closed on missing/wrong-architecture or incompatible
   prerequisite. Confirm no developer PATH or stray app-local DLL can mask it.
2. Verify actionable missing-runtime instructions, then run startup and
   targeted reviewed synthetic cases for PDFium/NativeText, Tesseract and EG79,
   technical stamp/table output, pure HEIC and JSON/DOCX/XLSX with the official
   runtime installed. Preserve the semantic comparison and final manifest
   binding. Do not substitute static inspection for these checks.
3. Verify the final package composition, notices, hashes and required receipts.
   Do not bypass the existing ZIP gate.

If both routes stay blocked, `x86_64-pc-windows-gnu` is a **future research
option only**. [Rust's target documentation](https://doc.rust-lang.org/rustc/platform-support/windows-gnu.html)
describes the GNU toolchain and its distinct runtime baseline. The MSVC
`x64-windows-static-md` native archives cannot be assumed reusable across
toolchains; investigate rebuilding the native Tesseract/vcpkg graph with
compatible archives, checking the PDFium C ABI, GNU runtime redistribution and
all OCR and packaging regressions. A port would not preserve this EXE's hash
or resolve EG79, PDFium or HEVC rights. No port or build was attempted.

**Windows Sandbox: NOT RUN — DEFERRED BY OWNER. Separate clean Windows x64:
NOT RUN — DEFERRED BY OWNER.** The existing candidate, RC1 and live runtime
remain unchanged. **WINDOWS PORTABLE RELEASE — BLOCKED: LICENSING.**
