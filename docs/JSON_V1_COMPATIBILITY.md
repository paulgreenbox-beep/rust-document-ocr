# JSON v1 diagnostic compatibility

`schema_version` remains 1. The `warnings` array retains exactly the eight
warning kinds from baseline `6699a10dbfeff2471471b5eb2688e4d5394fe671`.
Putting `cell_ocr_fallback_failed` inside that array breaks standard Serde
deserialization of the baseline `OcrWarning`; a field default does not allow
unknown enum variants.

Cell failures instead appear in an optional additive `diagnostics` array:

```json
{"kind":"cell_ocr_fallback_failed","page_number":1,"table_index":0,"row":1,"column":2,"reason_code":"recognition_failed"}
```

`page_number` is one-based; table, row and column indices are zero-based.
Diagnostics carry a safe reason code and coordinates, not OCR text or the
underlying error message. The typed Rust API is `api::OcrDiagnostic`.
Missing diagnostics deserialize as an empty vector; empty vectors are omitted
from JSON. Pipeline failures are mapped into this field, not discarded.

Compatibility boundaries:

- Baseline Serde `OcrResult` readers and JSON clients that ignore unknown fields
  continue to read `result.v1.json`, including results with cell failures.
  They cannot display diagnostics they do not know about.
- Strict clients rejecting unknown fields are not guaranteed compatible with
  additive fields; they must explicitly admit `diagnostics`.
- New readers accept baseline envelopes without `diagnostics` and preserve
  typed diagnostics when present. The short-lived pre-fix warning kind inside
  `warnings` is not part of the retained baseline contract.
- Legacy `result.json` still serializes only `result.document`; diagnostics do
  not change its shape, recognized content, or DOCX/XLSX exports.
- The baseline public `OcrWarning` variants are restored, so exhaustive matches
  over that enum need no new arm. Code using the pre-fix cell warning variant
  must migrate to `OcrDiagnostic`.
- Adding public `OcrResult::diagnostics` is a **Rust source compatibility break**
  for downstream struct literals and exhaustive struct destructuring. Such
  callers must initialize `diagnostics: Vec::new()` or use
  `OcrResult::from_document`, and use `..` in patterns where appropriate.
  JSON compatibility does not imply full Rust source compatibility.
- The earlier audit patch also added `PipelineWarning::CellOcrFallbackFailed`
  to the separately exposed low-level pipeline API. Exhaustive matches on
  `PipelineWarning` still need that arm; restoring `OcrWarning` does not make
  every Rust enum in the crate source-compatible with the baseline.

`tests/support/baseline_v1.rs` freezes the actual baseline `OcrResult` and
`OcrWarning` definitions. Supporting document/table/page-processing types are
shared unchanged DTOs, not substitutes for these frozen envelope/enum types.
`tests/v1_warning_compat.rs` retains the pre-fix rejection as a synthetic witness
and tests both reader directions, legacy export, and exhaustive enum matching.
