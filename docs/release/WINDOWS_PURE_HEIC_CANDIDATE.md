# Windows pure-HEIC candidate — feature and release notes

Status: **LOCAL REVIEW CANDIDATE — NOT A PUBLIC BINARY RELEASE**.
This is a new current-scope summary; older RC1 and HEIC research receipts retain
their original scope and are not rewritten as evidence for a new artifact.

## Technical document and stamp recognition

The application has its own Rust page/region, geometry, cell assignment and
bounded technical-recheck logic in addition to Tesseract recognition. Drawing
title-block support is limited to table-like ruled regions satisfying compact
lower-right geometry and sparse-content or supporting-label rules. Outputs
preserve actual text/cells and `kind=title_block`; no universal GOST form,
signature, seal or normalized stamp-requisite extraction is claimed.

EG79 is retained as the versioned technical-cell component of the proposed full
package. Its automatic use is restricted to selected suspicious nonempty data
cells under existing evidence rules; structural title blocks are excluded from
that recheck. It does not replace full-page rus+eng or the stamp detector.
Tesseract and its base model are upstream work, not wholly developed here.

## Table structure recognition

Ruled, approximately axis-aligned grids provide independent table bounds,
observed cells/intersections, rows/columns and unique word assignment. Native
glyph ownership is preserved before word union. Some incomplete separators,
sparse cells and observed rectangular spans are handled; conservative borderless
alignment is a separate fallback. Native vector paths can refine existing
raster edges but do not replace the grid detector. These boundaries and their
tests are listed in [FEATURES](../../FEATURES.md).

Table discovery, cell geometry, word placement, OCR accuracy and export fidelity
are distinct stages. A false grid on a checkerboard is not a successful table.
No arbitrary merged-cell, multiline-layout or universal engineering-table
accuracy claim is made.

## Native PDF text preservation

Usable NativeText remains complete page text even when tables are added.
Processing modes remain `native_text`, `native_text+grid`, `ocr`, `ocr+grid`.
Searchable pages preserve PDF rotation rather than unconditionally applying
OCR auto-orientation. The HEIC profile does not change routing or these rules.

## Supported input formats

PDF; JPEG/PNG/BMP; single/multipage TIFF; WebP; single/multipage DjVu through the
common stack. The pure feature selects local `heic-rs 0.1.4-ocr.1` instead of
libheif. Primary HEVC, named Main10/grid/alpha/transform fixtures are tested;
premultiplied alpha is explicitly rejected. This is not a guarantee for every
HEIF sequence, auxiliary image, metadata or color configuration. No hidden
legacy HEIF, ABBYY, cloud or manually installed-codec fallback is added.

## Output and export formats

`result.json`, schema-v1 `result.v1.json`, DOCX and XLSX keep their contracts.
Structural `kind` and semantic classification remain separate. Title blocks
stay in JSON; export options omit or separate them. XLSX uses `TitleBlock_*`
sheets, DOCX includes requested title blocks after the data tables. These are
content/table projections, not facsimile drawings or full physical merge export.

## Known limitations

- Damaged native Unicode, small/skewed/faint glyphs, noisy scans and complex CAD
  layouts can still produce wrong text. Confidence is a review signal.
- Stamp heuristics do not certify standard compliance or field correctness.
- Weak/missing grid lines, irregular borderless layouts and complex merges
  remain partial. Grid success is not OCR accuracy evidence.
- Four page workers remain the default. Grid/OCR DPI must be evaluated for the
  input document; there is no document-independent optimum.
- No new broad corpus or accuracy result is inferred from documentation changes.
  The exact retained pure EXE's prior 11-input and 40-PDF results remain at their
  recorded scope; focused synthetic checks supplement them.
- Sandbox and separate clean Windows x64 are each
  **NOT RUN — DEFERRED BY OWNER**. A controlled local PATH smoke is not either.

## Model and third-party licensing

The source package declares MIT for project code. Tesseract, PDFium, image
libraries, model bases/weights, training inputs, Microsoft runtimes and the
modified HEIC decoder have separate origins/conditions. Git attribution and a
passing test do not establish legal ownership of training materials or release
authority. EG79 is not removed to evade unresolved rights and is not uploaded.

For the exact pure EXE qualified in PR #8, available graph/link/PE/source evidence
excludes the old libheif/libde265 chain; legacy native/RC1 and mixed builds are
not covered by that exclusion. PDFium notice closure, Microsoft redistribution
eligibility, EG79/inputs, HEVC patent and modified heic-rs notice review and the
release owner's publication decision remain open.

**WINDOWS PORTABLE RELEASE — BLOCKED: LICENSING.** No public runtime ZIP, GitHub
Release, model publication, installation or main merge is authorized here.
See [rights review](../compliance/TECHNICAL_OCR_AND_EG79_RIGHTS.md) and the
[exact-artifact link report](../../HEIC_PURE_NATIVE_LINK_REPORT.md).
