# PDFium 7881 — release-owner compliance checklist

**Current decision: PDFIUM LICENSE CLOSURE — INCOMPLETE.**
**Windows portable release: BLOCKED: LICENSING.**

This documentation-only packet does not grant redistribution rights. It
does not modify the [Windows portable candidate PR #2](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/2),
its `distribution_status=blocked`, or `scripts/build-windows-portable.ps1`.
It contains no software binary, license approval, or production runtime update.

## PDFium-specific review

- [x] Identify published artifact by SHA-256 and signed producer workflow;
  preserve source/builder attestation limitations.
- [x] Inspect all 16 original producer notices, without changing them.
- [x] Add text supplements for HarfBuzz COPYING, FreeType fthash MIT,
  OpenJPEG component license and selected file-level copyrights.
- [ ] Establish applicable file/subdirectory terms for the *actual*
  HarfBuzz/OpenJPEG/FreeType scope; the historical link graph is unavailable.
- [ ] Evaluate whether the conservative extra HarfBuzz ms-use notice applies
  to selected object sources. It is present for review, not asserted linked.
- [ ] Resolve or consciously document the discrepancy between HarfBuzz
  candidate DEPS and wrapper README revisions.
- [ ] Map exact libc++, Clang/compiler-rt and static CRT dependencies in the
  PDFium DLL to applicable copyright grants, exceptions and notices.
- [ ] Establish which component inclusions/exclusions are supported by
  producer evidence versus candidate source defaults; no invented compiler
  SHA, DEPS, resolved GN args or link map.
- [ ] Finish file-specific copyright and Apache NOTICE scope review,
  including vendored changes and font/data notices where applicable.
- [ ] Assemble a binary-specific `PDFIUM_THIRD_PARTY_NOTICES.md` incorporating
  required FreeType/IJG credits; keep source-only notices accurate.
- [ ] Obtain explicit release-owner/legal/compliance review of the entire
  proposed PDFium binary notice packet.

## Other independent Windows portable gates

- [ ] Static **libheif/libde265 LGPL-3.0** obligations in the Rust executable
  (corresponding source, replacement/relink route, applicable notices).
  These are **not** automatically part of the PDFium DLL.
- [ ] Application-local MSVC DLL redistribution eligibility and unmodified
  REDIST terms, distinct from any PDFium static CRT. See
  [Microsoft redistribution guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files)
  and the relevant Visual Studio edition's REDIST list.
- [ ] Full Rust/native static dependency notice and retained-object review.
- [ ] Clean Windows x64 VM smoke test of the *approved* ZIP with verified
  EXE/inputs and baseline results, including native-text and raster OCR.

## No-promotion rule

Do not change `distribution_status` to `verified`; do not build/upload a
public ZIP, create a GitHub Release, or report the license gate as PASS
because the seven text files exist. A SHA-256, signed attestation, stable
API, synthetic smoke test or absence of an old source checkout is not
itself proof of all license obligations.

No PDFium rebuild is needed merely to add notices. Any future rebuild must
be a separately approved operation, with its own provenance and regression
validation. Keep the current working OCR/DLL/EXE/models untouched.

## Handoff decision

**Packet status: DRAFT — ready for further technical notice review, NOT
ready for final release clearance.** The final decision belongs to the
release owner after suitable compliance/legal review.
