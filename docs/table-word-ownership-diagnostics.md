# Table word ownership diagnostics

Pass `--table-ownership-diagnostics` to the native CLI to write
`table-ownership-diagnostics.json` alongside the regular exports. Rust hosts can
call `process_document_with_table_ownership_diagnostics` to receive the OCR
result and the report separately. The report has its own schema version.
A sidecar write failure emits a warning and the regular exports still publish.

Each entry identifies the one-based physical page and reports:

- `status` is `observed_conflict`, `observed_no_conflict`, or `not_observable`.
- `conflict_pairs` counts pairs of structural data tables with positive-area
  overlap and at least one shared source word.
- `unique_conflicting_words` counts each source word once.
- `additional_memberships` sums the number of distinct participating tables
  minus one for each conflicting word.
- `pairs` identifies tables by their zero-based positions in the page's emitted
  table list and includes the count of shared words in each pair.
- `evidence_complete` indicates whether every eligible pair could be ruled out
  by geometry or tested using source IDs. `reasons` contains stable codes.
  A confirmed pair can be reported even if evidence for other pairs is incomplete.

For ruled grids, the identity of a word is its position in the common page
word stream from PDF native text or raster OCR. The engine records that
position at the point where the word enters a cell. Equal text and rounded
coordinates play no role in identifying the source word. Title blocks and the
legacy table of kind `unknown` are outside the rule. Ordinary cell spans remain
within one table. The detector represents retained nested grids as distinct
tables, so containment is evaluated as overlap.

The borderless route retains table geometry. Its source word IDs are unavailable
after reconstruction, so an overlapping borderless pair gives
`not_observable`. Separate OCR calls on cell crops do not share the page's source
word stream. The report covers whole-document processing and leaves page quality
classification and the regular export formats unchanged. It contains no OCR
text, document names, or paths.
