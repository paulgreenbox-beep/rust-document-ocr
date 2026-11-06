# Positioned PDF text and table recovery

The canonical native page text stays separate from geometry-aware text assigned to table cells. The following corrections apply to supported documents by geometry, not by file or page identity.

| Correction | Synthetic coverage |
| --- | --- |
| Split disjoint positioned glyph runs without joining units, quantities or neighboring rows | Generated glyph positions, rotations, gaps and missing geometry |
| Use confirmed cell boundaries before forming positioned words; refine only locally supported vector edges | Generated PDF dispatch, subpixel boundary tests and ambiguous-edge fallback |
| Keep real separators and recover a sparse closed body row without fusing adjacent frames | Generated ruled images, bounded axis clusters, partial-edge and frame tests |
| Preserve visible spaces, punctuation, line order and text beside thin rules | Generated positioned text and raster cell crops |
| Choose one cell per word within a grid and report cross-grid seam duplicates | Synthetic adjacent-grid assignment and duplicate-metric tests |

`PageSource::native_words_for_grid_cells` is additive. Its default forwards to `native_words_for_rendered_page`, so existing custom sources need not implement it. The PDF source uses the supplied cell boundaries for its positioned words. Table values and geometry can change where ownership or reading order was previously wrong; the CLI, JSON schema and DOCX/XLSX serializers are not changed by these fixes.

Refined edges are used when splitting native glyph runs. Final assignment
uses the projected words and the detected grid cells; it does not carry a
stable refined cell identity through that projection. Do not interpret the
duplicate metrics as a guarantee of unique ownership across adjacent grids.

The optional technical-cell model is discovered in the chosen tessdata directory when present. It does not replace the configured page languages. The source change does not train, replace or grant redistribution rights for any model. Synthetic checks establish specific behavior, not a general accuracy rate.
