# Microsoft CRT: exact Windows x64 candidate evidence

Review: 2026-11-09. **File/REDIST correspondence verified; redistribution
authority not confirmed.** This is not Microsoft license terms or permission
to publish.

Scope: the pure-HEIC local candidate with release-manifest SHA-256
`7f2381924d4521feef2b4bebc0b69d9d95d99fb03e0efbed254c64fd390d6801`.
The three DLLs were read and compared, not loaded by OCR, modified, replaced,
or uploaded.

| App-local file | Bytes | File and product version | SHA-256 |
| --- | ---: | --- | --- |
| `msvcp140.dll` | 643,512 | 14.51.36247.0 | `7c26614e1d733892c2deac7e245ce115504b1d80592dd0a01b08e3e5a55f89ca` |
| `vcruntime140.dll` | 178,616 | 14.51.36247.0 | `d1f4225df2cd877dbf130d5668a021dce3f94118455ff5ec952061c30afc9ce7` |
| `vcruntime140_1.dll` | 50,112 | 14.51.36247.0 | `a7146c08f89fe5b04541ab507cdb59ff7b44534d4ba3c668a426c6450a03434e` |

Each candidate file matches its manifest entry and the installed x64 file
under `<VisualStudioFolder>/VC/Redist/MSVC/14.51.36231/x64/Microsoft.VC145.CRT/`.
The directory label `14.51.36231` is not the file version. The installer
identifies `Microsoft.VisualStudio.Product.BuildTools`, version
`18.7.11925.98`, channel `VisualStudio.18.Release`, not prerelease. The
separate Visual Studio 2022 installation is not the reference used here.

Windows `Get-AuthenticodeSignature` returned `Valid` for all three candidate
files. The signer was **Microsoft Windows Software Compatibility Publisher**,
organization **Microsoft Corporation**, with certificate thumbprint
`2650247AC56048BC7928663C02733D25898A1D6E` and a Microsoft time-stamp
countersignature. This records the host's verification, not a trust or
eligibility assertion for another host. Identical hashes prove byte
correspondence, not which physical copy path was used or who has a license.

## Official terms and the remaining decision

- The [Visual Studio 2026 REDIST list, Visual C++ Runtime Files](https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution#visual-c-runtime-files)
  covers the installed `VC/redist` subtree subject to the applicable software
  terms and unmodified distribution with the program. These three files are
  in the retail x64 CRT subtree, not a `debug_nonredist` exclusion. List
  membership is not an affirmative publisher grant by itself.
- Microsoft's [Visual C++ deployment guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170#redistributable-files-and-licensing)
  conditions redistribution on the applicable Visual Studio license. It
  distinguishes app-local servicing from centrally installed redistributables.
  Switching deployment is not authorized here and would need separate review.
- The [official Visual Studio license directory](https://visualstudio.microsoft.com/license-terms/)
  lists [Diagnostic and Build Tools 2026](https://visualstudio.microsoft.com/license-terms/vs2026-ga-diagnostic-buildtools/)
  and [Visual C++ V14 Redistributable and Runtime 2026](https://visualstudio.microsoft.com/license-terms/vs2026-ga-visualcpp-v14-redist-runtime/)
  separately. The [retained terms review](../MSVC_REDISTRIBUTION_REVIEW.md)
  identifies the exact Word agreements previously examined. Microsoft's
  [Build Tools licensing guidance](https://www.microsoft.com/licensing/guidance/Visual-Studio)
  describes the limited open-source dependency build exception; do not treat
  that exception, runtime installation/use or signature validity as the
  publisher's permission to distribute the DLLs.

**Delivered:** exact three-file correspondence, conditional list mapping, and
the [unsigned publisher confirmation](PUBLISHER_CONFIRMATION.md). The
candidate's `licenses/MSVC_REVIEW_PENDING.txt` stays in place; it is not
Microsoft terms. The publisher must supply the actual agreement/channel,
affirmative clause, eligibility, scope and acceptance of release conditions.
This evidence is limited to the three app-local files; it does not establish
the composition or rights of any static Microsoft/LLVM runtime code.
