# Public fixture provenance and test scope

The documents in `examples/documents/` are new synthetic documents made for
this source release by `examples/generate_sample.py`. Their text, table values,
layout and images are original synthetic material. They contain no customer
documents, personal information, corpus-derived crops or commercial scans.

The text/table PDF uses standard PDF Helvetica without embedding a font file.
The image-only PDF is a PDFium-rendered image of that same synthetic page.
The generator uses external ReportLab, Pillow and pypdfium2 development tools;
none of these tools, their binaries or their fonts are vendored here.

Regenerate with an environment that already supplies these optional development
packages:

```text
python examples/generate_sample.py
```

OCR output examples are recorded from actual CLI runs. They demonstrate the
schema and execution path, not recognition accuracy on real-world documents.
Runtime-dependent timing fields may differ between runs.

The versions used for generation are pinned in `examples/requirements-generate.txt`.
The two PDFs were visually inspected after rendering. Standard Helvetica is
referenced, but no font program is embedded in either distributed PDF.

## External test data excluded from this snapshot

The original local baseline was validated separately. Its private control
benchmark, real training crops, unverified multi-input images and PDFs with
unverified embedded-font redistribution rights are not distributed here.

The following eight fixture-dependent integration test files are intentionally
excluded rather than silently reported as passing public tests:

- `multi_input_pipeline.rs` and `multi_input_sources.rs`;
- `native_raster_chinese.rs`, `native_raster_eng.rs`, `native_raster_rus.rs`;
- `region_raster_ocr.rs`, `semantic_tables_pdf.rs`;
- `v1_2_control_regression.rs`.

The public suite retains source-only tests and tests that construct their own
synthetic PDFs. The library's existing unit tests are preserved. The local
test-runtime preparation script copies the new synthetic PDF to the historical
`runtime/fixtures/w3c-dummy.pdf` filename expected by render tests; it does not
copy an external W3C document. `runtime/` is ignored and never published.

No research corpus, model weights, native DLL or traineddata file is included.
