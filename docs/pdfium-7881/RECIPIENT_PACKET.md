# PDFium 7881 recipient packet and remaining approval

**Known notice set prepared and verified for owner review. PDFium license
closure remains INCOMPLETE. No redistribution approval.**

## What is proved

- The pure-HEIC local candidate contains PDFium 151.0.7881.0, AMD64, API 7881,
  7,211,520 bytes, SHA-256
  `79d4676b656cfb1abcea88f9ade3b4b0826c5200382db5f4ec72a636c598c118`.
  The [public producer release](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F7881)
  and the retained [archive/attestation evidence](LICENSE_EVIDENCE.json)
  identify the matching producer artifact. The old attestation, build and
  runtime acceptance checks were not rerun.
- The actual 16 producer notices match their archived sizes and hashes. Each
  was compared against its pinned source or collector transformation: six
  reproduce exact bytes, nine differ only in CRLF/LF, and the builder's
  `LICENSE` matches its source after the documented staging paragraph and
  newline normalization. FreeType's original notice includes a non-UTF-8
  copyright byte; comparison preserved it, not a replacement character.
  [Original source pins and raw hashes](PDFIUM_THIRD_PARTY_NOTICES_DRAFT.md),
  [collector](https://github.com/bblanchon/pdfium-binaries/blob/867b3ecc32163d715e84708c51fae5f031254985/steps/08-licenses.sh),
  [builder LICENSE staging](https://github.com/bblanchon/pdfium-binaries/blob/867b3ecc32163d715e84708c51fae5f031254985/steps/07-stage.sh).
- The eight actual previously reviewed supplements are present and their
  LF-normalized bytes match the recorded reviewed texts; the package uses
  CRLF. The new recipient copies retain the actual package bytes. The older
  evidence JSON's hashes for these eight remain the LF reference, not a
  claim about raw CRLF bytes.
- The two OpenJPEG LUT headers in the
  [candidate PDFium tree](https://github.com/chromium/pdfium/tree/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/libopenjpeg)
  match the [pinned upstream OpenJPEG 2.5.4 tree](https://github.com/uclouvain/openjpeg/tree/6c4a29b00211eb0430fa0e5e890f1ce5c80f409f/src/lib/openjp2)
  byte-for-byte. The classic `t1_luts.h` SHA-256 is
  `1c6cd64611fc56641e7f48ae0575808e0ed135b845cd4806f6e7f47d7bbf7c79`;
  `t1_ht_luts.h` is
  `5988bb6f3da45c4db3f233db782b24b18951f37e38b326c83beb350587e7928e`.
  These are candidate-source comparisons, not retained-object measurements.

## What was corrected

1. The old review index referred to original producer texts not carried in the
   repository. Its `notices/` links also did not resolve from the previously
   staged `licenses/pdfium/supplements/` directory. The [recipient NOTICE](recipient/NOTICE.md)
   now links to actual included texts in the same tree; it replaces that
   historical review index as the recipient entry point.
2. The producer collector renames `README.ijg` to `libjpeg_turbo.ijg`, while the
   unchanged libjpeg-turbo license document still links to `README.ijg`.
   A byte-identical local alias fixes navigation without rewriting either
   original text. The archival source reference for the IJG file now points to
   [README.ijg](https://chromium.googlesource.com/chromium/deps/libjpeg_turbo/+/640f254ad0fa03f6b1f29f89b7dd9366f2f6e533/README.ijg),
   not the separate license roll-up. The archive's `licenses/` directory
   layout is retained so its root LICENSE's relative reference also works.
3. The recipient NOTICE gives the FreeType and Independent JPEG Group product
   credits explicitly. No external URL is required to read the local terms.
4. Added the complete [libc++ license text](recipient/supplements/llvm-libcxx-LICENSE.TXT)
   at [candidate libc++ pin 5abc7f8](https://chromium.googlesource.com/external/github.com/llvm/llvm-project/libcxx.git/+/5abc7f839700f0f17338434e1c1c6a8c87c00c11/LICENSE.TXT)
   and [compiler-rt license text](recipient/supplements/llvm-compiler-rt-LICENSE.TXT)
   at [LLVM revision 20b6ec66967a](https://github.com/llvm/llvm-project/blob/20b6ec66967ac2a8f932863c1abf251e5b17a843/compiler-rt/LICENSE.TXT),
   both as conservative candidate supplements. The
   [candidate DEPS](https://github.com/chromium/pdfium/blob/91b9d569b34be4f38eed7b3c49b227356c3aadad/DEPS)
   identifies the Windows clang package `clang-llvmorg-23-init-10931-g20b6ec66-11.tar.xz`
   (SHA-256 `855e4a23cfd89e0fa9a6899ccb343a7c28fd3d26f045a89cc40cc4c29cb6a85c`).
   This explains the notice pin, not the actual historical toolchain receipt.
   The producer's `llvm-libc.txt` is a separate notice, not a substitute.
5. Added complete leading BSD notice blocks from the classic and HT
   [OpenJPEG generators](recipient/supplements/openjpeg-lut-generator-notices.txt)
   corresponding to the verified candidate LUT files. The notices do not
   assert that the generator programs were shipped or linked.

## Ready documents

Use [NOTICE.md](recipient/NOTICE.md) and all 28 texts listed in
[RECIPIENT_FILES.json](RECIPIENT_FILES.json) together. The inventory pins
the exact intended byte sizes and SHA-256 hashes, including both LLVM texts
and the IJG alias. Their relative paths map under a future release's
`licenses/pdfium/` directory; keep this directory structure intact.
The repository's `.gitattributes` only preserves raw third-party text bytes;
it is not a recipient payload file. The historical staged review draft is
not an alternative to this NOTICE.

This is a physical text packet for the known evidence, not a runtime package
or packager input. No DLL, package manifest or existing runtime was changed.
If approved later, update the package inventory, include the actual notice
tree, and validate the **new final payload** and links before publishing.

## Concrete confirmation still required

The publisher/reviewer must accept this exact DLL and notice set and obtain
or decide how to resolve the missing scope evidence below. Nothing here
establishes all required notices for the historical binary:

| Missing historical evidence | What would close the question |
| --- | --- |
| Actual PDFium source SHA, resolved DEPS, full GN args and applied patches | A producer build receipt for the exact archive, not current candidate defaults or the builder-repository attestation alone |
| Actual LLVM/MSVC/SDK versions, exact builtins and static CRT inputs | Producer toolchain inputs and link/object/archive receipts; assess file-level exceptions and static CRT terms independently of the three app-local CRT DLLs |
| Which static objects/headers, generated data and font payloads were retained | Linker/member inventory plus generated-data/font provenance; map HarfBuzz/FreeType/OpenJPEG/JPEG/SIMD terms and any Apache NOTICE obligations to those inputs |
| Completeness of final recipient notice texts | An accepted component-to-notice map, additional applicable texts if identified, and publisher/compliance sign-off tied to this DLL SHA and final package inventory |

No absence of a PE import proves absence of static content. Adding
conservative texts does not make unknown retention known. If the missing
historical receipts cannot be obtained, a separately authorized, receipted
build with known inputs is a different release candidate, not a workaround
that clears this one. [Release gate](RELEASE_GATE.md) stays blocked.
