# PDFium 7881 component evidence matrix

**DRAFT — NOT APPROVED FOR REDISTRIBUTION. License closure INCOMPLETE.**

This records 28 components/groups reviewed, not 28 confirmed DLL components.
Exact sources, revisions, hashes and qualifications are in
[LICENSE_EVIDENCE.json](LICENSE_EVIDENCE.json).

| Level | Meaning |
| --- | --- |
| `CONFIRMED_IN_BINARY` | Direct engine/API identity; not exact source or member map |
| `PRODUCER_RECIPE_EVIDENCE` | Pinned producer instructions or matching archived explicit config; not all resolved execution values |
| `CANDIDATE_SOURCE_EVIDENCE` | Reachability/configuration in candidate source; historical retention remains UNKNOWN |
| `NOTICE_ONLY` | Notice present, actual linkage unresolved |
| `UNKNOWN` | Evidence unavailable or insufficient |
| `NOT_INCLUDED_PROVEN` | Strong proof of exclusion required; **not used here** |

The old combined `RECIPE_OR_SOURCE_DEFAULT` label is split above rather than
silently promoted. No actual historical object/link receipt is claimed.

| Group | Available evidence | Current qualification |
| --- | --- | --- |
| 01 Package builder | `PRODUCER_RECIPE_EVIDENCE` | Package-level MIT, not DLL-wide; producer staging known. |
| 02 PDFium | `CONFIRMED_IN_BINARY` | Engine identity confirmed; actual checkout and static members UNKNOWN. |
| 03 FreeType | `CANDIDATE_SOURCE_EVIDENCE` | Full fthash MIT supplied; FTL credit in draft; retained scope open. |
| 04 ICU and bundled Unicode/data notices | `CANDIDATE_SOURCE_EVIDENCE` | Full composite original retained; actual units/data scope open. |
| 05 JPEG / libjpeg-turbo / IJG | `CANDIDATE_SOURCE_EVIDENCE` | Original terms retained; explicit IJG product credit; SIMD/API scope open. |
| 06 OpenJPEG | `CANDIDATE_SOURCE_EVIDENCE` | Existing four supplements plus additional file notices; LUT/full-body/historical scope open. |
| 07 zlib | `CANDIDATE_SOURCE_EVIDENCE` | Review any distinct notices in selected SIMD/source files; no generic claim that header extraction is defective. |
| 08 LittleCMS | `CANDIDATE_SOURCE_EVIDENCE` | File-specific coverage of vendored modified component. |
| 09 AGG 2.3 | `CANDIDATE_SOURCE_EVIDENCE` | Candidate metadata says MIT, but text is noncanonical; exact SPDX matching remains explicit. Actual retained AGG source scope; source includes PDFium downstream patches. |
| 10 Abseil | `CANDIDATE_SOURCE_EVIDENCE` | Actual header/object scope and applicable attribution/NOTICE inventory; no specific missing NOTICE proved. Appendix placeholders are not real author attribution. |
| 11 fast_float | `CANDIDATE_SOURCE_EVIDENCE` | Exact historical source/header scope; other optional licenses need not all be elected. |
| 12 HarfBuzz | `CANDIDATE_SOURCE_EVIDENCE` | COPYING and ms-use supplied; candidate derived-data chain traced; historical retention UNKNOWN. |
| 13 PNG / libpng | `NOTICE_ONLY` | Historical DLL linkage not proven absent; do not remove notice. Scope of separately licensed contrib/configure code if included/distributed. |
| 14 TIFF / libtiff | `NOTICE_ONLY` | Any additional file-specific holders if TIFF actually linked. Do not call this MIT or declare entire TIFF coverage complete. |
| 15 simdutf | `NOTICE_ONLY` | Historical inclusion not proven; keep existing notice. |
| 16 LLVM libc | `CANDIDATE_SOURCE_EVIDENCE` | Actual retained headers/objects and any separately licensed portions. Coverage of libc++/compiler-rt cannot be inferred from this filename. |
| 17 LLVM libc++ | `CANDIDATE_SOURCE_EVIDENCE` | Candidate static libc++; applicable LLVM/legacy/runtime mapping open. |
| 18 Clang compiler-rt builtins | `CANDIDATE_SOURCE_EVIDENCE` | Candidate builtins path; not compiler-rt atomic; actual archive/version unknown. |
| 19 Microsoft static CRT | `CANDIDATE_SOURCE_EVIDENCE` | Candidate /MT path, distinct from app-local MSVC DLLs; rights unresolved. |
| 20 Skia | `CANDIDATE_SOURCE_EVIDENCE` | Candidate nonselection is not proof of absence from historical DLL. No affirmative license expression invented for an unreviewed inclusion. |
| 21 Brotli | `CANDIDATE_SOURCE_EVIDENCE` | Candidate nonselection is not proof of absence from historical DLL. No affirmative license expression invented for an unreviewed inclusion. |
| 22 Fontations | `CANDIDATE_SOURCE_EVIDENCE` | Candidate nonselection is not proof of absence from historical DLL. No affirmative license expression invented for an unreviewed inclusion. |
| 23 Rust PNG | `CANDIDATE_SOURCE_EVIDENCE` | Candidate nonselection is not proof of absence from historical DLL. No affirmative license expression invented for an unreviewed inclusion. |
| 24 V8 | `PRODUCER_RECIPE_EVIDENCE` | Candidate nonselection is not proof of absence from historical DLL. No affirmative license expression invented for an unreviewed inclusion. |
| 25 XFA | `PRODUCER_RECIPE_EVIDENCE` | Candidate nonselection is not proof of absence from historical DLL. No affirmative license expression invented for an unreviewed inclusion. |
| 26 PartitionAlloc | `PRODUCER_RECIPE_EVIDENCE` | Candidate nonselection is not proof of absence from historical DLL. No affirmative license expression invented for an unreviewed inclusion. |
| 27 libc++abi / libunwind / compiler-rt atomic | `CANDIDATE_SOURCE_EVIDENCE` | Candidate nonselection is not proof of absence from historical DLL. No affirmative license expression invented for an unreviewed inclusion. |
| 28 PDFium bundled Foxit font data | `CANDIDATE_SOURCE_EVIDENCE` | Complete retained font-data/source notice scope not independently enumerated. |

Candidate root .gn overrides `use_llvm_libatomic=false`; generic Chromium
defaults must not create a fictitious atomic dependency. Windows candidate
libc++ does not automatically imply libc++abi/libunwind. No absence of a DLL
import proves absence of static code. Disabled candidate defaults are not
`NOT_INCLUDED_PROVEN`.

Sources and missing evidence: [draft](PDFIUM_THIRD_PARTY_NOTICES_DRAFT.md).
The original 16 texts stay untouched; the eight supplemental files are review
material. See [RELEASE_GATE.md](RELEASE_GATE.md) before any release decision.
