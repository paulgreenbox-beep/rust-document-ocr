# PDFium 7881 — compact 28-component evidence matrix

**DRAFT. License closure INCOMPLETE; redistribution NOT approved.**
Adapted from the local 2026-10-30 review. This table records *candidate*
source selection, **not** proven historical retained DLL objects. No
third-party component has a confirmed historical executed build graph.

Classification: `CONFIRMED_IN_BINARY` = direct engine/API identity;
`RECIPE_OR_SOURCE_DEFAULT` = candidate source/recipe only;
`NOTICE_ONLY` = original notice exists but linking unverified.
Disabled defaults do not establish `NOT INCLUDED — PROVEN`.

| Group | Evidence | Pending review |
| --- | --- | --- |
| 01 Package builder | NOTICE_ONLY | MIT applies to package; not entire DLL |
| 02 PDFium | CONFIRMED_IN_BINARY | Root grant retained; dependencies unresolved |
| 03 FreeType | RECIPE_OR_SOURCE_DEFAULT | fthash MIT and product credit supplement |
| 04 ICU + Unicode data | RECIPE_OR_SOURCE_DEFAULT | Composite notice; source scope open |
| 05 libjpeg-turbo / IJG | RECIPE_OR_SOURCE_DEFAULT | Product credit / SIMD scope |
| 06 OpenJPEG | RECIPE_OR_SOURCE_DEFAULT | Component/file notices supplemental |
| 07 zlib | RECIPE_OR_SOURCE_DEFAULT | Header grant; source scope |
| 08 LittleCMS | RECIPE_OR_SOURCE_DEFAULT | MIT; object scope open |
| 09 AGG 2.3 | RECIPE_OR_SOURCE_DEFAULT | Original grant; object scope |
| 10 Abseil | RECIPE_OR_SOURCE_DEFAULT | Apache; applicable NOTICE check |
| 11 fast_float | RECIPE_OR_SOURCE_DEFAULT | MIT; object scope |
| 12 HarfBuzz | RECIPE_OR_SOURCE_DEFAULT | Missing COPYING / subdir terms |
| 13 PNG | NOTICE_ONLY | XFA scope unresolved |
| 14 TIFF | NOTICE_ONLY | Single-file grant; other copyright scope |
| 15 simdutf | NOTICE_ONLY | V8 checkout is not linking |
| 16 LLVM libc | RECIPE_OR_SOURCE_DEFAULT | Existing text; runtime map open |
| 17 LLVM libc++ | RECIPE_OR_SOURCE_DEFAULT | Runtime notices unresolved |
| 18 compiler-rt builtins | RECIPE_OR_SOURCE_DEFAULT | Link-input scope open |
| 19 Microsoft static CRT | RECIPE_OR_SOURCE_DEFAULT | License/REDIST rights open |
| 20 Skia | RECIPE_OR_SOURCE_DEFAULT | Candidate disabled; not proven absent |
| 21 Brotli | RECIPE_OR_SOURCE_DEFAULT | Candidate disabled; not proven absent |
| 22 Fontations | RECIPE_OR_SOURCE_DEFAULT | Candidate disabled; not proven absent |
| 23 Rust PNG | RECIPE_OR_SOURCE_DEFAULT | Candidate disabled; not proven absent |
| 24 V8 | RECIPE_OR_SOURCE_DEFAULT | Explicit=false; not proven absent |
| 25 XFA | RECIPE_OR_SOURCE_DEFAULT | Explicit=false; not proven absent |
| 26 PartitionAlloc | RECIPE_OR_SOURCE_DEFAULT | Explicit=false; not proven absent |
| 27 libc++abi/libunwind/atomic | RECIPE_OR_SOURCE_DEFAULT | Candidate exclusion only |
| 28 Bundled Foxit font data | RECIPE_OR_SOURCE_DEFAULT | Source attribution scope |

The original 16 notices are preserved in the local bblanchon bundle.
Full pinned sources and new supplements: [NOTICES.md](NOTICES.md).
The exact historical PDFium checkout, DEPS, linked objects and toolchain
were not recovered. Release blockers: [RELEASE_GATE.md](RELEASE_GATE.md).
