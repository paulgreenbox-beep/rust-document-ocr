# HEIC-RS experiment and remaining Windows release decisions

Status: **EXPERIMENT ONLY — NOT APPROVED FOR REDISTRIBUTION**.
No replacement OCR EXE or RC2 portable package was created by this evaluation.
The existing RC1 remains **WINDOWS PORTABLE RELEASE — BLOCKED: LICENSING**.

## Decoder decision

### Latest consumer adapter experiment

Root `heic-rs-input` is now an explicit opt-in full-pipeline adapter; default
`native` remains libheif. The small synthetic A/B and local source/native tests
are recorded in the [pipeline report](../../tools/heic-rs-evaluation/PIPELINE_HEIC_RS_RESEARCH.md).
No production promotion or release executable was made. The native dependency
graph remains selected; **full-app LGPL removal is not established**. Current
release rights/notice, patent and deferred-host gates remain unchanged.
The earlier decoder-only decision below retains its original limited scope.

### Current derivative addendum (2026-11-07)

The isolated local0.1.4-ocr.1 derivative now has tested complete raw-SPS
primary/grid/alpha accounting before dispatch and HEVC image buffers, with
per-codec checks. This advances the initial preallocation gap at that stated
algorithmic scope. It is not an exact heap-byte cap; metadata/input costs,
thread policy and explicit None opt-out remain separate. The registry cache,
root OCR Cargo/features/backend and RC1 runtime were not modified.

Genuine Main10/alpha/nonzero rotations/both mirrors now have frozen synthetic
fixtures and independent libheif comparisons. A reversed imir axis defect was
reproduced and corrected only in the local derivative. Colour differences and
the unchanged native white16 security rejection are retained. See the
[new bounded report and receipts](../../tools/heic-rs-evaluation/BOUNDED_HEIC_RESEARCH.md).
These named cases do not settle general HEIF/OCR equivalence, hostile-input
resource safety or patents. Original MIT/Apache-2.0/NOTICE material and dated
modification notices accompany the vendored source.

No actual full-app linker/removal evidence or new EXE is produced. The existing
RC1's libheif/libde265 obligations remain applicable. Production-adapter/RC2
promotion stays held pending separate source, format, OCR and rights review.

### Initial unmodified-crate decision (historical)

Published heic-rs 0.1.4 has MIT OR Apache-2.0 licensing, registry SHA-256
`8883c3bfa6b4ff19a40e52a6dc051c5612cbe21bf408fc6d4875e8dfd777cb66`
and packaged source commit `999470f514c8e9a1a7c37ea44ba70cc32b1308fe`.
These identities were verified, not inferred from a moving tag. The isolated
probe disables default features and enables only std; tile parallelism is off.
The existing OCR feature configuration and four-page-worker default are unchanged.

Six synthetic 8-bit HEVC fixtures, including a grid image, decode. This is not
coverage of 10-bit, alpha, true orientation metadata, fine-font OCR, sequences
or every supported HEIF form. See the [bounded experiment evidence](../../tools/heic-rs-evaluation/README.md).

The published decoder's max_pixels option does not establish the application's
required preallocation boundary. A small fixture with container dimensions
16x16 and HEVC dimensions 64x64 passes the container check at a 256-pixel limit,
but subsequently returns a 4096-pixel limit error. The exact published source
allocates SPS-sized HEVC planes before the later colour conversion limit.
This is a safe small runtime observation plus source tracing, not an OOM or
peak-memory measurement. Nor does it by itself prove an OCR-quality or
allocation regression relative to libheif.

Do not enable a production adapter or create a purported RC2 from these results.
A next candidate needs a reviewed limit passed into raw SPS/plane allocation
(including conformance cropping and aggregate grid tiles), additional format
fixtures and comparative OCR. Avoid an unreviewed custom HEVC parser or hidden
native fallback. No full OCR linker/removal claim is made for a standalone probe.

The source licensing does not automatically settle third-party HEVC patents.
The upstream [NOTICE](https://github.com/tbraun96/heic-rs/blob/999470f514c8e9a1a7c37ea44ba70cc32b1308fe/NOTICE)
states that the licensors do not license third-party HEVC patents. Release
owner/legal review must assess the actual jurisdictions, use and distribution.
This is a question for that reviewer, not an engineering legal clearance.

## Existing license gates remain applicable

| Area | Evidence preserved/rechecked | Still required |
| --- | --- | --- |
| PDFium 7881 | DLL identity; producer's 16 notice texts and eight supplements; source-evidence distinctions in the existing packet | Retained-component/NOTICE closure, applicable font/LLVM/static runtime material and approval of recipient credits. Do not turn a candidate source revision into the historical build receipt. |
| libheif / libde265 | They remain in the current OCR configuration and RC1; the library-only relink proof remains at its recorded scope | Applicable LGPL combined-work notices, exact corresponding-source/application materials and a practically usable full-app recombination route. The pure decoder probe does not remove this gate from RC1. |
| Microsoft CRT | App-local files below retain their recorded version/hash; technical REDIST provenance was established previously | Publisher's applicable valid license and affirmative redistribution grant; servicing decision. Installed file identity alone is not permission. |
| EG79 | Exact pinned Apache-2.0 base bytes and selected modified weights were identified previously | Applicable attribution/modification notice plus authorization for weights and review of real crop/label and font-rendered training input rights. No private corpus or weights are published here. |

All three current RC1 CRT files are version 14.51.36247.0:

| File | SHA-256 |
| --- | --- |
| msvcp140.dll | 7c26614e1d733892c2deac7e245ce115504b1d80592dd0a01b08e3e5a55f89ca |
| vcruntime140.dll | d1f4225df2cd877dbf130d5668a021dce3f94118455ff5ec952061c30afc9ce7 |
| vcruntime140_1.dll | a7146c08f89fe5b04541ab507cdb59ff7b44534d4ba3c668a426c6450a03434e |

References: [existing PDFium packet](../pdfium-7881/RELEASE_GATE.md),
[existing HEIF LGPL options](HEIF_LGPL_RELEASE_OPTIONS.md),
[Microsoft REDIST conditions](https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution#visual-c-runtime-files),
[Build Tools terms](https://visualstudio.microsoft.com/license-terms/vs2026-ga-diagnostic-buildtools/),
and [EG79 base license at its pinned revision](https://github.com/tesseract-ocr/tessdata_best/blob/e2aad9b983032bb1beff9133104a67cdbb87ca4d/LICENSE).
A release owner or qualified compliance reviewer must resolve the rights
questions; this document is not a legal certificate.

## Environment and publication

- Windows Sandbox: **NOT RUN — DEFERRED BY OWNER**.
- Separate clean Windows x64: **NOT RUN — DEFERRED BY OWNER**.
- A controlled-PATH local test is not either environment.
- No new clean receipt, public ZIP, tag, release or main merge.
- No working OCR, service, DB or model change.
- The fail-closed packager retains its actual clean-Windows receipt and rights
  requirements; a future payload needs its own evidence.
