# Strict Clippy baseline

The accepted OCR implementation retains style debt under strict Clippy. This
publication stage acknowledges those findings with item-scoped `#[expect]`
attributes; it does not refactor the affected code or claim that the lints were
fixed. This lint-baseline pass changed no function body, expression, signature,
or algorithm.

Most annotations use this reason:

```rust
#[expect(clippy::<lint>, reason = "Preserve the accepted OCR baseline during source-only publication.")]
```

The two CLI control-flow expectations in `main.rs` retain their more specific
reasons for preserving feature-gated control flow and output-transaction
control flow. The default-feature-only `unused_variables` expectation is
conditional on `not(feature = "native")`.

## Findings

There are 23 expected Clippy findings across the source and test targets, plus
one conditional Rust compiler `unused_variables` expectation for the default
feature configuration.

| Lint | Count | Existing items |
| --- | ---: | --- |
| `clippy::manual_is_multiple_of` | 4 | `borderless_table::median`; `table_grid::odd_kernel`; `table_grid::median_f64`; test helper `native::pdfium_source::pack_gray_rows` |
| `clippy::io_other_error` | 3 | `export_docx::export_docx_with_options_legacy`; `export_docx::export_docx_with_options`; `export_xlsx::export_xlsx_with_options` |
| `clippy::too_many_arguments` | 6 | `PageSource::render_region_at_dpi`; `pipeline::prepare_page`; `pipeline::finish_prepared_ocr_page`; test helper `pipeline::process_one`; `pipeline::prepare_grid_cell_ocr_regions`; `native::pdfium_source::PdfiumPageSource::render_region_at_dpi_with_limit` |
| `clippy::needless_character_iteration` | 1 | `pipeline::recheck_suspicious_nonempty_cells` |
| `clippy::manual_c_str_literals` | 1 | `native::tesseract_engine::TesseractOcrEngine::initialize` |
| `clippy::useless_vec` | 2 | `table_semantics::classify_table`; test `table_grid::cell_text_does_not_join_short_mixed_or_cross_line_runs` |
| `clippy::cloned_ref_to_slice_refs` | 2 | Tests `export_docx::exports_all_data_tables_in_reading_order_and_omits_title_blocks_by_default`; `export_docx::title_block_can_be_included_after_data_tables` |
| `clippy::needless_borrow` | 1 | Test `table_grid::keeps_sparse_grid_away_from_drawing_corner_as_data_table` |
| `clippy::bool_comparison` | 1 | Integration test `pipeline_runs_one_page_and_writes_artifacts` |
| `clippy::needless_return` | 1 | `main::main` |
| `clippy::question_mark` | 1 | `OutputTransaction::create` |
| **Clippy total** | **23** | |

The conditional compiler lint is `unused_variables` on `main::main` when
`native` is disabled. The expectations remain scoped to their individual
functions, methods, or tests. This document does not claim that any of the
underlying style findings were fixed.

## Verification status

Both strict Clippy commands completed successfully in the public staging tree:

| Configuration | Result | Log |
| --- | --- | --- |
| Default features, all targets | PASS | `reports/ocr-release-validation-20261027/public-default-clippy-final.log` |
| Windows x64 native feature, all targets | PASS | `reports/ocr-release-validation-20261027/public-native-clippy-final.log` |

Commands used:

```text
cargo clippy --offline --locked --all-targets -j 1 -- -D warnings
cargo clippy --offline --locked --all-targets --features native -j 1 -- -D warnings
```

The default-feature source check is the CI-facing configuration. The native
feature check was run locally on Windows x64 with its native dependencies
provisioned. `--all-targets` checks test-target code but does not execute the
tests. No test suite or OCR job was run for this Clippy task.

`#[expect]` is checked: an expectation with no matching lint for a selected
configuration produces an unfulfilled-expectation warning, which the strict
commands promote to an error. The successful runs confirm that these
expectations matched in the configurations shown above.
