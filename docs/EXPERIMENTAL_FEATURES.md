# Experimental and limited features

This page separates implemented options with narrower validation boundaries
from the normal CPU OCR path. No feature listed here changes the default
language from `rus+eng` or makes the system an accuracy-certified service.

## Opt-in language routing

The default `configured` route uses the languages supplied to Tesseract.
Explicit `simplified-chinese` routing selects `chi_sim+eng` for each page.
`auto-chinese` uses conservative signals from an existing PDF text layer; it
does not run a Chinese probe on scanned or raster-only pages. Chinese routing
requires separately supplied Chinese traineddata.

An optional technical-cell model can provide a bounded recheck for selected
cells. It is external to this source package and is not required for normal
`rus+eng` page OCR. No language data or model weights are included.

## Borderless table fallback

The default library mode, `GridWithFallback`, first uses ruled-grid detection
and may use repeated word alignment when no ruled grid is found. This fallback
is heuristic. Sparse layouts, irregular columns, multi-line cells, and complex
merged cells may be rejected or reconstructed incompletely. The `Grid` mode
is ruled-grid only; `Legacy` keeps a compatibility path; `Off` skips table
extraction. These are `OcrOptions` library settings, not CLI flags.

## Semantic table classification

Semantic table types and column roles are deterministic, probabilistic
classifications based on normalized header aliases, value patterns, geometry,
and OCR/detector signals. A specific type is emitted only when combined
evidence is sufficient; otherwise the result remains generic or unknown.
Classification evidence contains rule identifiers and scores, not copied OCR
text. It does not correct text, populate missing values, or replace structural
`kind`.

Confidence fields help downstream review. They do not prove that a cell or
document is correct.

## Diagnostics and unsupported outputs

The features `native-diagnostics`, `pdfium-c-probe`, and
`table-grid-review` enable diagnostic or review tools. They are not required
by the normal CLI or library path.

Overlay generation is not implemented. The library reports a structured
`overlays_unsupported` warning if overlay output is requested; it does not
silently promise an overlay file.

The engine is CPU-only. GPU processing and research alternatives such as
Surya, GOT, or ABBYY are not dependencies, supported backends, or required
parts of the build and test workflow.
