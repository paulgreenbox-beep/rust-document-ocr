# Positioned PDF text and table recovery

The canonical native page text stays separate from geometry-aware text assigned to table cells. The following corrections apply to supported documents by geometry, not by file or page identity.

| Correction | Synthetic coverage |
| --- | --- |
| Split disjoint positioned glyph runs without joining units, quantities or neighboring rows | Generated glyph positions, rotations, gaps and missing geometry |
| Use confirmed cell boundaries before forming positioned words; refine only locally supported vector edges | Generated PDF dispatch, subpixel boundary tests and ambiguous-edge fallback |
| Keep real separators and recover a sparse closed body row without fusing adjacent frames | Generated ruled images, bounded axis clusters, partial-edge and frame tests |
| Preserve visible spaces, punctuation, line order and text beside thin rules | Generated positioned text and raster cell crops |
| Report ownership without counting a word in multiple cells | Synthetic adjacent-grid assignment tests |

`PageSource::native_words_for_grid_cells` is additive. Its default forwards to `native_words_for_rendered_page`, so existing custom sources need not implement it. The PDF source uses the supplied cell boundaries for its positioned words. Table values and geometry can change where ownership or reading order was previously wrong; the CLI, JSON schema and DOCX/XLSX serializers are not changed by these fixes.

`PageSource::native_words_with_grid_owners` additionally carries non-serialized source ownership alongside the unchanged words. Its default delegates to the existing grid hook without supplying owners, preserving geometric assignment for custom sources. PDF source owners are bound to actual grid/cell indices before assignment, so rounding an ink box cannot move a word across a refined source seam. Word confidence and bounding boxes, canonical page text, and exported JSON coordinates remain unchanged. Malformed owner counts/indices use the existing geometry-unavailable diagnostic path. The separately run synthetic PDF test `source_grid_owner_survives_word_projection_and_matrix_json` covers source refinement through final cell JSON.

The optional technical-cell model is discovered in the chosen tessdata directory when present. It does not replace the configured page languages. The source change does not train, replace or grant redistribution rights for any model. Synthetic checks establish specific behavior, not a general accuracy rate.
