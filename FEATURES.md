# OCR capabilities and verified scope

Rust Document OCR combines third-party text recognition with project-specific
Rust page analysis, glyph/word geometry, grid reconstruction, cell assignment,
technical-cell selection and export. It is not only a Tesseract UI, and it is
not a universal drawing, stamp, or table recognizer.

Statuses below distinguish implemented/tested structures from universal OCR
accuracy. **Supported** means the named path and bounded tests exist;
**partial** means important layouts remain outside that path; **experimental**
is opt-in research, not a promoted production replacement. Source tests and a
local runtime smoke do not constitute clean-Windows or release-rights approval.

## Page text, regions and responsibilities

| Capability | Scope | Implementation and controls |
|---|---|---|
| Preserve usable PDF NativeText | Supported; table extraction does not replace the complete page text with cell text | [page analysis](src/page_analysis.rs), [pipeline](src/pipeline.rs), `tests/native_text_selection.rs`, `tests/region_table.rs` |
| Raster/scanned-page OCR | Supported through Tesseract; default languages `rus+eng`, external matching traineddata required | [engine](src/native/tesseract_engine.rs), [preprocessing](src/preprocessing.rs), reviewed synthetic raster PDF |
| Native text plus tables | Supported as `native_text+grid`; native words can populate cells without full-page Tesseract | [pipeline](src/pipeline.rs), `tests/native_grid_dispatch.rs` |
| Selected PDF region | Supported library API: normalized top-left rectangle; explicit text/table mode, clipping/error diagnostics | [region API](src/region.rs), `tests/region_native_text.rs`, `tests/region_table.rs`, rotation/crop tests |
| OCR page orientation | Existing bounded route; searchable pages preserve PDF rotation rather than unconditional OCR auto-rotation | [page analysis](src/page_analysis.rs), [pipeline](src/pipeline.rs) |

PDFium opens/extracts the PDF and supplies the bitmap through the existing
direct-C render module. Tesseract/Leptonica recognize raster text and provide
word boxes/confidence. Rust decides routing, prepares crops, detects/reconstructs
grids, preserves NativeText, assigns words, selects bounded technical rechecks,
classifies structures/semantics, and builds exports. `image`/`imageproc` and
other upstream libraries supply foundational image operations. These dependencies
are not represented as original project inventions.

## Technical document and stamp recognition

The implemented stamp scope is a **table-like drawing title-block region**,
not arbitrary stamp images, handwritten approval seals or a certified GOST form.

1. A ruled grid is detected from the rendered page. Its own bbox and cells are
   retained; the full page is not automatically one table.
2. `table_grid::classify_grid` recognizes a compact lower-right drawing-frame
   geometry. Sparse content is evidence only inside that geometry. Dense frames
   also need supporting label groups such as sheet/лист, scale/масштаб,
   drawn/разраб, checked/пров and date/дата in separate cells.
3. Text comes from positioned native glyphs or the ordinary raster OCR path.
   The output is the actual cell text, bounds, confidence and structural
   `kind=title_block`; semantic classification can add `drawing_title_block`.
4. There is **no dedicated normalized stamp record** guaranteeing project number,
   author, signature, date, stage or scale fields. A matching label is supporting
   classification evidence, not proof that its value is correct.

| Stamp structure | Scope | Existing controls and limits |
|---|---|---|
| Sparse compact lower-right ruled frame | Supported bounded heuristic | `classifies_compact_lower_right_grid_as_title_block`; no arbitrary corner/shape guarantee |
| Dense compact lower-right labelled grid | Partial | `compact_dense_grid_needs_drawing_stamp_labels`, `dense_title_stamp_is_excluded_from_docx_and_xlsx_exports`; supporting labels/geometry required |
| Sparse table elsewhere on page | Preserved as data, not discarded as a stamp | `keeps_sparse_grid_away_from_drawing_corner_as_data_table` |
| Frame touching a data table through incomplete edges | Partial | `partial_frame_bridge_preserves_separate_data_and_title_grids`, `distant_title_block_axes_do_not_erase_body_separator` |
| GOST-specific form/mandatory requisites, signatures or seals | Not supported as a dedicated contract | No standard-specific acceptance corpus or field extractor; no compliance claim |

Stamp source: [table_grid.rs](src/table_grid.rs),
[table_semantics.rs](src/table_semantics.rs), [pipeline.rs](src/pipeline.rs).
Named classifier controls are in the first module; export contract controls are
in [semantic_table_contract.rs](tests/semantic_table_contract.rs).
The generated searchable-PDF integration control in
[technical_document_capabilities.rs](tests/technical_document_capabilities.rs)
connects real PDFium NativeText, body/title grids, geometry/confidence and
JSON/DOCX/XLSX projections. It is not a scanned-stamp OCR or EG79 accuracy test.
Overall real-world stamp recognition remains
**IMPLEMENTED — VALIDATION INCOMPLETE**, not a universal PASS.

### EG79: retained technical component, not a universal stamp model

The engine discovers the versioned `eng_technical_eg79_v1.traineddata` when it is
present in the selected tessdata directory. It does not replace the configured
full-page `rus+eng` language selection. The library keeps this model optional
for older embeddings, while the proposed **full pure-HEIC portable profile
requires the retained EG79 payload**. Distribution rights remain a blocker;
the model is not silently dropped to simplify publication.

In the implemented nonempty-cell technical recheck, the pipeline first requires
recognized headers/roles and suspicious candidates in code/designation/
manufacturer/cable-mark columns, limits work, prepares a cell crop, then compares
generic English and specialized model output under bounded edit/confidence/
technical-grammar rules. ASCII technical codes must include letters and digits;
supported punctuation is not a promise to understand all engineering notation.
Disagreement is not freely replaced with a guessed dictionary value.

**Structural title blocks are deliberately excluded from that specialized
recheck.** Empty-cell fallback, numeric rechecks and full-page OCR are distinct
paths. EG79 is therefore important to technical-cell processing, but it is not
the detector/classifier for drawing stamps. Both models still use Tesseract;
cross-model agreement is not independent ground truth.

Controls: `versioned_technical_model_is_optional_and_does_not_change_page_languages`
in [tesseract_engine.rs](src/native/tesseract_engine.rs);
`technical_cell_selector_requires_exact_independent_consensus`,
`numeric_cell_grammar_rejects_identifier_homoglyphs_and_formulas`, and
`technical_recheck_never_modifies_a_structural_title_block` in
[pipeline.rs](src/pipeline.rs). Selector tests use controlled OCR results; they
are not a recognition-accuracy benchmark for EG79 weights.

## Table structure recognition

| Structure | Implementation | Test coverage | Limitations |
|---|---|---|---|
| Visible horizontal/vertical rules | Supported raster grid path | `detects_cells_from_a_ruled_grid`, 11×3 / 3×3 / 2×3 controls | Resolution, faint/skewed/curved lines and unrelated drawing rules can cause misses/false grids |
| Incomplete dividers | Partial: observed closed cells/shared-edge connectivity | `partial_shared_edges_connect_but_corner_contacts_do_not`, `detects_single_body_row_below_full_width_separator` | Does not invent arbitrary absent boundaries |
| Sparse rows / physically missing slots | Supported bounded reconstruction | `matrix_keeps_coordinates_for_missing_physical_cell`, public sparse projection test | Placeholder geometry does not become a fabricated physical box/value |
| Merged cells | Partial: spans only from observed closed bounds | `records_spans_only_for_observed_closed_cell_bounds` | No general complex merge recovery; DOCX/XLSX do not reproduce physical spans as native merged cells |
| NativeText inside grids | Supported source-owned glyph/word assignment | `production_source_wrapper_forwards_grid_boundaries_before_word_union`, source ownership unit regressions | Damaged Unicode/text mapping or unavailable geometry can remain problematic |
| Raster table with OCR | Supported shared grid/word/cell route | pipeline controlled-engine tests and reviewed raster CLI control | Correct grid does not imply correct OCR; numeric/cable codes still require review |
| Table-like title block | Partial geometry/label classification | Stamp controls above | Not a standard-specific stamp parser |
| Borderless aligned table | Partial conservative fallback after no ruled grid | `reconstructs_borderless_specification_with_real_geometry`, `missing_cell_does_not_shift_following_columns`, prose rejection controls | Irregular columns, multiline prose and weak alignment may be rejected; no ML model |
| Source vector lines | Optional bounded refinement of detected raster edges | `source_edge_separates_two_subpixel_neighbor_font_centers`, competing-edge and curve rejection tests | Not a standalone vector table detector; nested Form paths are not traversed; unavailable evidence retains raster bounds |
| Domain-specific table type | Local probabilistic classification | material specification/equipment schedule/cable journal/work estimate/commercial offer/noisy header tests | Semantics do not correct values or prove every table of that type is supported |

The ruled path uses adaptive threshold/inversion, directional erosion+dilation,
segment merging, stable axes, observed closed cells and line intersections.
It does not use a neural table model. Borderless reconstruction and the older
word-alignment fallback are separate and must not be confused with verified
line-grid geometry. Source precision is in
[grid_geometry.rs](src/native/pdfium_source/grid_geometry.rs), not a second
unbounded raster render.

Implementation: [table_grid.rs](src/table_grid.rs),
[borderless_table.rs](src/borderless_table.rs),
[pdfium_source.rs](src/native/pdfium_source.rs), [pipeline.rs](src/pipeline.rs).
PR #5's glyph ownership, neighbor-cell, bounded-coordinate and custom-PageSource
compatibility behavior is retained. The HEIC feature separation does not edit
these algorithms.

## JSON, DOCX and XLSX

- JSON schema remains `1`. `kind` is structural (`data_table`, `title_block`,
  `unknown`); `semantic_type` is a separate uncertain domain classification.
  Cell text, geometry, row/column/spans, confidence and assignment diagnostics
  remain available where supported. Missing geometry is not invented.
- Full page text is preserved independently of table cells. Legacy `result.json`
  and additive `result.v1.json` keep their established contracts.
- XLSX routes data tables to `Page_*` and title blocks to separate `TitleBlock_*`
  sheets, or omits title blocks by explicit option without deleting JSON data.
- DOCX exports multiple data tables in reading order. Included title blocks are
  placed after data tables; the low-level exporter default omits them, while
  the public options determine the requested projection.
- These are content/table projections, not facsimile drawing layout. Geometry
  and confidence belong primarily to JSON/API; complex physical merged-cell
  fidelity, fonts and drawing layout are not promised by Word/Excel export.

Controls: [semantic_table_contract.rs](tests/semantic_table_contract.rs),
[export_docx.rs](src/export_docx.rs), [export_xlsx.rs](src/export_xlsx.rs),
[JSON compatibility](docs/JSON_V1_COMPATIBILITY.md).

## Inputs, operational limits and release status

The common native pipeline supports PDF, JPEG/PNG/BMP, single/multipage TIFF,
WebP and single/multipage DjVu with format/size/page/pixel limits. HEIF behavior
depends on the selected backend. Pure `heic-rs-input` is **experimental**: primary
HEVC image, bounded grid/Main10/alpha/transform cases are tested, but arbitrary
HEIF sequences, premultiplied alpha and universal codec/color equivalence are
not supported or qualified. Extra images may be ignored with a warning.

The four-page-worker CPU default is unchanged; PDFium access is serialized.
DPI is an adjustable input, not a universal quality optimum. No cloud OCR,
ABBYY fallback, GPU OCR, automatic human verification or production overlay
generation is provided by these paths.

Limited synthetic and accepted private-corpus checks are described in
[the exact-artifact report](HEIC_PURE_NATIVE_LINK_REPORT.md), not evidence of
universal stamp/table accuracy. See the separate
[technical code/model rights review](docs/compliance/TECHNICAL_OCR_AND_EG79_RIGHTS.md).
Source MIT declarations do not license every runtime/model/training input.
**WINDOWS PORTABLE RELEASE — BLOCKED: LICENSING.** Sandbox and separate clean
Windows x64 remain **NOT RUN — DEFERRED BY OWNER**.
