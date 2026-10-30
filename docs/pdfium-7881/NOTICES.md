# PDFium 7881 — pinned notice supplements (review copy)

**DRAFT. NOT A DISTRIBUTABLE PACKAGE OR LICENSE APPROVAL.**

The existing producer archive has 16 original notices: root `LICENSE`
and `licenses/{abseil,agg23,fast_float,freetype,icu,lcms,libjpeg_turbo.ijg,
libjpeg_turbo.md,libopenjpeg,libpng,libtiff,llvm-libc,pdfium,simdutf,zlib}`
with their original filename extensions. Those original texts and their
hashes were inspected in the local candidate and are **not changed** here.
The original builder's MIT license does not override the PDFium engine and
dependency licenses. Keep all original notices in any future reviewed bundle.

## New source-pinned text files

| Supplement | Original source at pinned revision | Review status |
| --- | --- | --- |
| [HarfBuzz COPYING](notices/harfbuzz-COPYING.txt) | [HarfBuzz COPYING, d639197](https://github.com/harfbuzz/harfbuzz/blob/d639197ed529b05c27f38ebaab365a621d5edad5/COPYING) | Main Old MIT wording, copyright list and disclaimers retained |
| [HarfBuzz ms-use COPYING](notices/harfbuzz-ms-use-COPYING.txt) | [HarfBuzz src/ms-use/COPYING, d639197](https://github.com/harfbuzz/harfbuzz/blob/d639197ed529b05c27f38ebaab365a621d5edad5/src/ms-use/COPYING) | Separate Microsoft MIT terms; inclusion in this DLL **unproven** |
| [FreeType fthash MIT](notices/freetype-fthash-MIT.txt) | [fthash.c, b08a2eb](https://github.com/freetype/freetype/blob/b08a2eb0dd37f4a6c886fa5b0ecf5b3e1d27aac7/src/base/fthash.c), [fthash.h](https://github.com/freetype/freetype/blob/b08a2eb0dd37f4a6c886fa5b0ecf5b3e1d27aac7/include/freetype/internal/fthash.h) | Full matching MIT grant/comment; distinct from archived FreeType FTL |
| [OpenJPEG LICENSE](notices/openjpeg-LICENSE.txt) | [PDFium vendor LICENSE, 91b9d56](https://github.com/chromium/pdfium/blob/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/libopenjpeg/LICENSE) | Full component BSD-2-Clause license |
| [OpenJPEG dwt.c](notices/openjpeg-dwt.c-header.txt) | [Source header, 91b9d56](https://github.com/chromium/pdfium/blob/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/libopenjpeg/dwt.c) | Full leading copyright/license comment |
| [OpenJPEG t1.c](notices/openjpeg-t1.c-header.txt) | [Source header, 91b9d56](https://github.com/chromium/pdfium/blob/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/libopenjpeg/t1.c) | Full leading copyright/license comment |
| [OpenJPEG ht_dec.c](notices/openjpeg-ht_dec.c-header.txt) | [Source header, 91b9d56](https://github.com/chromium/pdfium/blob/91b9d569b34be4f38eed7b3c49b227356c3aadad/third_party/libopenjpeg/ht_dec.c) | Full leading copyright/license comment |

For these two source sets, the PDFium candidate revision is **not** a
historical executed checkout receipt. The HarfBuzz wrapper README reports
a different revision from candidate DEPS; the discrepancy stays open.
Even complete primary license texts do not establish full retained-object
coverage. Additional HarfBuzz subdirectory and file-specific terms must be
checked against the accepted scope.

## Proposed user-visible product credits

> This product includes PDFium and third-party software governed by their
> respective licenses. Rust Document OCR's MIT license does not replace them.
>
> This software is based in part on the work of the FreeType Team.
>
> This software is based in part on the work of the Independent JPEG Group.

The original IJG wording also survives inside the producer's archived
`libjpeg_turbo.ijg`. These credits are proposed for the future binary
distribution, **not** proof that the entire notice packet is complete.

## Future distribution integration (not done)

Review owner should retain each original notice byte-for-byte under
`licenses/pdfium/`, then add approved supplements under
`licenses/pdfium/supplements/` and an explicit navigation/credit page
`licenses/PDFIUM_THIRD_PARTY_NOTICES.md`. The documentation here is NOT
wired into the Windows ZIP packager and does not change the local runtime.
Do not copy or publish a public ZIP until [RELEASE_GATE.md](RELEASE_GATE.md)
is resolved and the release owner has signed off.
