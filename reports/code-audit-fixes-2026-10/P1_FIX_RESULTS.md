# P1 fix results

## Supplemental cell OCR isolation

`OcrEngine::recoverable_cell_error_reason` is an additive default method;
unknown/custom engine errors remain fatal unless explicitly classified.
Native Tesseract classifies only recognition-status and malformed-recognition
TSV failures as local. Required initialization, configuration, encoding,
preprocessing and resource failures are not converted to success.

The fallback keeps original cell snapshots. A failed primary, numeric,
manufacturer or generic technical request restores the original text,
confidence, spans and bbox. Rollback occurs before table-wide consensus and
again at the return boundary, so failed tentative text cannot influence other
cells. Other cells and subsequent pages continue. The separately optional
technical model retains its established absence/unavailable fallback.

Full-document paths return an additive warning:

```json
{"kind":"cell_ocr_fallback_failed","page_number":2,"table_index":0,"row":3,"column":1,"reason_code":"recognition_failed"}
```

Pages are one-based; table/row/column indices are zero-based. No recognition
value or raw recognition exception is included. Region processing retains its
existing warning shape and puts only location/code in the message; fatal
region OCR errors propagate as `ocr_failed`.

Regressions cover native-grid and raster-grid finish paths, original cell
fields, unchanged physical geometry, typed warnings, required initialization,
unknown failures, optional technical initialization, later technical failure,
numeric failure after tentative success, and a complete two-page pipeline.
All pass in source-only and native suites. Native classifier unit coverage
also passes.

## Incomplete-grid placeholders

Every matrix slot is initialized with its actual row/column before physical
cells are overlaid. Default spans remain one, text empty, confidence zero,
and legacy bbox absent. The existing stable API zero-size bbox sentinel is
retained; no physical rectangle is invented.

The 3x3 fixture omits physical cell (2,1). Tests verify all nine unique matrix
coordinates, serialized legacy/stable JSON, missing geometry, and all eight
physical cells' unchanged text, confidence, bbox and spans.

The two committed synthetic PDF comparisons show identical page text, cell
values, confidence, geometry, routing and exports. See the three regression
JSON reports for the measured control boundary.
