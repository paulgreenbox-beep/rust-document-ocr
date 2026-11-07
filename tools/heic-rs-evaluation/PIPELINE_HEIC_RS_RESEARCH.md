# Opt-in HEIC-RS full OCR pipeline A/B

**2026-11-07 — experimental adapter implemented; production promotion held.**
Implementation commit: `1905632ff1775dd727c9eac3bb768707b8036313`.

## Feature and unchanged contract

- `--features native`: existing libheif page source, unchanged default behavior.
- `--features heic-rs-input`: implies native, explicitly selects the bounded
  local `heic-rs0.1.4-ocr.1` source for HEIF signatures only. If both features
  are specified, this explicit experimental override wins. No native fallback.
- Decoder std only, serial; file/frame/display/raw-aggregate limits retained.
  Shared raw-site budget is not an exact heap-byte/RSS ceiling. A raw resource
  refusal becomes `heif_decode_failed`, stage `hevc_resource_limit`, with the
  actual required/limit units; it is not mislabeled as displayed width*height.
- The adapter returns cached GrayImage via the existing RenderedPage interface,
  using the same integer white-alpha compositing/luma weights. Transforms apply
  once in the decoder; primary-linked premultiplied alpha is explicitly rejected.
- PDF/native text, OCR/routing, GridDetector, assignment and export algorithms
  are unchanged. `schema_version:1`, structural kind, semantic fields and
  JSON/DOCX/XLSX contracts remain intact; default page workers remain4.

This first integration **retains the native Cargo dependency graph**, including
libheif/libde265. It does not prove their removal from an OCR executable or
clear LGPL obligations. Root default features remain empty. Cargo.lock added
one local package/one dependency edge, without updating existing versions.
The edited Cargo.toml was normalized from historical CRLF to LF so ordinary
diff whitespace validation passes; this is not an algorithm change.

## Reproducible comparison

Two fresh Windows x64 debug executables built from the same implementation
commit, offline/locked, with only the feature selection differing. No release
EXE was rebuilt or installed. Existing process-local MSVC/SDK/LLVM/vcpkg was
used; no codecs, models or tools were installed.

Each executable processed11 inputs independently (22 processes per run), with
120-second child timeout, bounded async stdout/stderr capture and termination
of only a handle-owned process tree. Settings: rus+eng, explicit identical
tessdata and PDFium7881 paths, ocr_dpi300, grid_dpi100, workers4, cell OCR
fallback enabled, OMP_THREAD_LIMIT1. These DPI values are controlled test settings,
not a universal optimum. Run repeated; text/grid outcomes reproduced.

The three text HEICs are pinned, unmodified Apache-2.0 synthetic upstream
[form/note/receipt fixtures](fixtures/ocr-public/PROVENANCE.md), with original
SVG text sources and retained license. No arbitrary-image HEVC encoder was
available locally, so no encoder was installed. A failed partial download from
another host was preserved outside the source tree and never used as input.

| Case | Native / HEIC-RS characters | Synthetic token F1 native / HEIC-RS | Grids / filled cells (both) | Final run milliseconds native / HEIC-RS |
| --- | ---: | ---: | ---: | ---: |
| Form HEIC | 269 /266 | 0.943396 /0.980392 | 0 /0 | 3288 /3211 |
| Note HEIC | 154 /154 | 1.000000 /1.000000 | 0 /0 | 1925 /2084 |
| Receipt HEIC | 331 /331 | 0.925000 /0.925000 | 1 /12 | 1675 /2146 |
| Native text/table PDF | 343 /343 | not a new accuracy measurement | 1 /9 | 509 /619 |
| Image-only table PDF | 627 /627 | not a new accuracy measurement | 1 /9 | 5376 /5256 |

Token F1 uses casefolded Unicode word multisets against source SVG text only,
without OCR-confusion substitutions. Form recall is1.0 for both: reducing words
28 to26 removes extra OCR tokens (source25), rather than losing expected words.
The note and receipt text are byte-identical across decoders. Their numeric
confidence values differ, and all such differences remain in the receipt.
Receipt grid geometry, cell text/filled count and selected DOCX/XLSX parts
match. No semantic confidence equality is invented.

Both PDF controls match full v1 JSON after removing only per-page timings,
including NativeText and grid routing, plus legacy JSON and compared exports.
The six further format-only inputs exercise Main10, alpha, irot90, imir0,
grid composition and white16 through the full pipeline. The first five match
non-timing results. All HEIC-RS cases exit0 and produce all4 artifacts. Native
white16 exits1 with its previously documented security-limit error and publishes
no output; candidate white16 exits0. There are **no timeouts/incomplete captures**.

Flat/pattern fixtures are not OCR-quality evidence. Existing pipeline false
tables/values on patterns (including98 filled checker-grid cells despite zero
page text) occur identically in both variants and are explicitly not accepted
as useful table recognition. No GridDetector/OCR change was made to hide them.

Comparison scope: full `result.v1.json` except page timings; legacy `result.json`
also compared. OOXML checks reuse the existing audit helpers: DOCX
`word/document.xml` and XLSX workbook/worksheets/sharedStrings XML. This is not
whole-ZIP/style/relationship or visual-layout equality. Raw exports are retained
locally. Input identity/lane/hash mismatches and duplicate variants are rejected
before comparison (five synthetic negative tests).

Timings above are illustrative debug observations, **not a speedup claim**.
This corpus is too small and not Russian/CAD/human-holdout evidence for a broad
quality/replacement decision. No original40-PDF candidate qualification is claimed.

## Verification

- cargo fmt --check: PASS.
- Candidate offline/locked check and both debug builds: PASS.
- Native suite:357 passed; candidate suite:369 passed (includes12 adapter tests).
- Five existing ignored tests per suite: two PDFium controls subsequently run
  separately per feature, bothPASS; three diagnostic-only tests NOT RUN.
- Candidate library Clippy, -Dwarnings: PASS.
- Signature/decoder selection integration test passes in both configurations.
- Five Python comparison-integrity negatives and PowerShell syntax: PASS.

Initial native-suite exit101 is retained: local PDFium fixture/runtime was
missing and default Temp caused XLSX access errors. The existing test-runtime
preparation script copied hash-verified inputs into the ignored experiment
runtime; process-local TMP/TEMP and serial tests resolved these. No production
assets or existing test assertions were changed. See
[machine-readable receipt](pipeline-evaluation.json) for source/runtime/log and
debug executable hashes and every per-case outcome. Historical bounded/initial
decoder receipts remain unchanged.

## Use without deployment

```powershell
cargo build --offline --locked --features native --bin ocr-pipeline --target-dir <research-target>
# Retain this baseline executable separately, then build the candidate:
cargo build --offline --locked --features heic-rs-input --bin ocr-pipeline --target-dir <research-target>
pwsh -NoProfile -File tools/heic-rs-evaluation/run-pipeline-ab.ps1 -BaselineExe <absolute-baseline-exe> -CandidateExe <absolute-candidate-exe> -Tessdata <absolute-tessdata> -Pdfium <absolute-pdfium.dll> -OutputRoot <fresh-research-directory>
python tools/heic-rs-evaluation/compare_pipeline_ab.py <research-directory> --output <new-report.json>
```

Primary image only; sequences/overlays and premultiplied alpha are not admitted
as supported candidate behavior. Conservative raw accounting can reject some
large/cropped/tiled inputs allowed by the old decoder. More malformed-file,
combined-transform, ICC/HDR and fine-font/real-document testing is required.

Working OCR, RC1, services, DB and global environment remain untouched. No ZIP,
runtime upload, tag, deployment or merge. Windows Sandbox and separate clean
Windows x64 remain **NOT RUN — DEFERRED BY OWNER**. Windows portable binary
release remains **BLOCKED: LICENSING**; patent, PDFium, LGPL, MSVC and model
rights decisions are not cleared by this experiment.
