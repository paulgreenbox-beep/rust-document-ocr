# Third-Party Notices

## Project license and scope

The project license is stated in LICENSE as MIT, with the copyright line exactly: Copyright (c) 2026 Киселюк Павел Геннадьевич. This identifies the project copyright holder supplied for this release; it does not replace the terms of third-party components or grant rights to external data, models, fonts, SDKs, or services.

The Rust registry dependency inventory, including exact locked versions and license expressions, is in docs/CRATE_LICENSES.md. This release is source-only. No PDFium, Tesseract, Leptonica, HEIF/DjVu native runtime binaries, SDK DLLs, model checkpoints, traineddata, or developer-tool packages are redistributed here.

## Optional native OCR and document runtimes

The values marked validated below are external runtime compatibility requirements recorded in local runtime documentation. They are not claims that a corresponding binary is packaged, nor do they identify an exact redistributable binary build.

| Component | Version or evidence | Upstream license terms | Source-release decision |
| --- | --- | --- | --- |
| PDFium | External optional runtime; local documentation validates API revision 7881, but no exact binary build receipt is recorded here. | The upstream LICENSE begins with a BSD 3-clause-style grant and also contains Apache-2.0 text and third-party notices. The license closure of a built PDFium distribution depends on its exact build. | No PDFium binary is included. Before distributing one, identify the exact build and carry its PDFium and third-party notices from that build. |
| Tesseract | External optional runtime; local documentation validates 5.5.2. | Apache-2.0 in the official 5.5.2 LICENSE. | No engine binary is included. A future binary package needs its own binary and dependency inventory. |
| Leptonica | External optional runtime; local documentation validates 1.87.0. | The official license file contains a two-condition BSD-style notice; retain its full copyright, conditions, and disclaimer with any redistributed binary. | No library binary is included. |
| libheif | External optional HEIF runtime; the exact installed native library build is not pinned by a receipt here. The Cargo binding version is separately listed in docs/CRATE_LICENSES.md. | The libheif library is GNU LGPL version 3 in upstream COPYING; sample applications and wrappers have separate MIT terms. | No native library binary is included. Its LGPL terms must be handled if a particular binary build is later redistributed. |
| libde265 | Possible HEIF codec/runtime dependency; exact presence depends on the selected libheif build. | The libde265 library is GNU LGPL version 3 in upstream COPYING; sample applications have separate MIT terms. | No native library binary is included. Resolve exact presence and notices from a build-specific dependency receipt before any binary distribution. |
| Tesseract traineddata | External data files selected separately by language/repository/revision. | The official tessdata, tessdata_fast, and tessdata_best repositories declare Apache-2.0. These model files have their own repository license boundary, separate from the Tesseract engine. | No .traineddata file or model weights are included. Pin and review each exact model repository and revision before redistributing data. |
| DjVu Rust implementation | djvu-rs 0.27.0 is in the Rust crate inventory. No native DjVu binary requirement is asserted here. | The crate's declared license is listed in docs/CRATE_LICENSES.md. | Rust source dependency only; no DjVu native runtime binary is included. |

Authoritative native references:

- [PDFium upstream LICENSE](https://pdfium.googlesource.com/pdfium/+/refs/heads/main/LICENSE); pypdfium2 upstream also explains that PDFium binaries have their own license and dependency notices in [its licensing notes](https://github.com/pypdfium2-team/pypdfium2/blob/main/README.md).
- [Tesseract 5.5.2 LICENSE](https://github.com/tesseract-ocr/tesseract/blob/5.5.2/LICENSE).
- [Leptonica 1.87.0 license file](https://github.com/DanBloomberg/leptonica/blob/1.87.0/leptonica-license.txt).
- [libheif COPYING](https://github.com/strukturag/libheif/blob/master/COPYING) and [libde265 COPYING](https://github.com/strukturag/libde265/blob/master/COPYING).
- [tessdata LICENSE](https://github.com/tesseract-ocr/tessdata/blob/main/LICENSE), [tessdata_fast LICENSE](https://github.com/tesseract-ocr/tessdata_fast/blob/main/LICENSE), and [tessdata_best LICENSE](https://github.com/tesseract-ocr/tessdata_best/blob/main/LICENSE).

Cargo metadata does not resolve the native DLL/import closure. In particular, a future binary distribution needs exact receipts for PDFium and its bundled transitive components, plus the actual HEIF codec configuration. The source-only snapshot avoids representing those external binaries as packaged facts. No dependency-license blocker is visible for this source-only scope from the recorded inventory; this is limited to the components inventoried here and does not replace file-specific fixture rights review. A native binary release is not cleared: it still needs the exact PDFium third-party closure and build receipt, plus compliance review for LGPLv3 libheif/libde265 when present.

## Optional synthetic-fixture developer tools

The public synthetic PDF generator uses external developer tools when a maintainer chooses to regenerate the examples. These packages are not Rust dependencies; their generation versions are pinned in `examples/requirements-generate.txt`. None of their package sources, binaries, PDFium wheels, or font files are vendored.

| Tool | Upstream license statement | Release treatment |
| --- | --- | --- |
| ReportLab | ReportLab's official developer FAQ says its open-source toolkit is under a BSD license; it does not identify a specific SPDX subtype there. | External optional generator dependency only; no ReportLab package is included. |
| Pillow | MIT-CMU License. | External optional generator dependency only; no Pillow package or binary is included. |
| pypdfium2 | Apache-2.0 OR BSD-3-Clause for pypdfium2 itself; its documentation/examples are separately CC-BY-4.0. Its PDFium binary builds carry separate PDFium and dependency notices. | External optional generator dependency only; no pypdfium2 package, PDFium wheel, or font file is included. |

The source script and synthetic documents are described in docs/FIXTURE_PROVENANCE.md. That provenance record says the text/table example is original synthetic content, uses standard unembedded Helvetica, and the image-only example is rendered from it. Fixture inclusion remains a file-specific rights question; this notice does not blanket-exclude all tests or fixtures.

Primary developer-tool references: [ReportLab licensing FAQ](https://docs.reportlab.com/developerfaqs/), [Pillow LICENSE](https://github.com/python-pillow/Pillow/blob/main/LICENSE), and [pypdfium2 licensing notes](https://github.com/pypdfium2-team/pypdfium2/blob/main/README.md).

The external English and Russian language files used in local validation were
matched by size and Git-blob SHA to the official `tessdata_fast` repository at
commit `87416418657359cb625c412a48b6e1d6d41c29bd`. The exact hashes and upstream
links are recorded in [INSTALL_WINDOWS.md](docs/INSTALL_WINDOWS.md). No language
files are distributed; the upstream Apache-2.0 model terms remain separate.

## Research-only OCR systems and model artifacts

Surya, GOT-OCR, and ABBYY are not dependencies or assets in this source release. The project's MIT license grants no rights in their code, services, checkpoints, or model weights.

| System or artifact | Primary-source terms reviewed | Source-release decision |
| --- | --- | --- |
| Surya | The current upstream repository distinguishes Apache-2.0 code from model weights under a modified AI Pubs Open Rail-M license with commercial-use restrictions; see its README and MODEL_LICENSE. A separately published Surya detection checkpoint, vikp/surya_det3, identifies its weights as CC-BY-NC-SA-4.0, showing why the exact model ID and revision matter. | No Surya code or weights are included. Do not treat the project MIT file as a license for any research copy; pin its exact code and model revisions and review their own terms before use or redistribution. |
| GOT-OCR 2.0 | The upstream StepFun model repository stepfun-ai/GOT-OCR2_0 declares Apache-2.0 on its model page. Other forks, conversions, or base-model components require their own review. | No GOT code or weights are included. The model page is not a blanket license for different copies or derivatives. |
| ABBYY | ABBYY Cloud OCR SDK terms describe ABBYY technology as proprietary and govern it through the applicable service or customer agreement. Exact product and agreement are not identified here. | No ABBYY SDK, service client package, or model is included. No redistribution right is asserted. |

Primary references: [Surya upstream README](https://github.com/datalab-to/surya/blob/master/README.md), [Surya MODEL_LICENSE](https://github.com/datalab-to/surya/blob/master/MODEL_LICENSE), [Surya detection model card](https://huggingface.co/vikp/surya_det3), [StepFun GOT-OCR2_0 model page](https://huggingface.co/stepfun-ai/GOT-OCR2_0), and [ABBYY Cloud Terms of Service, Cloud OCR SDK exhibit](https://www.abbyy.com/legal/cloud-terms-of-service-18-01-2024/).

## Release composition

For a source-only export, keep the project source, manifests and lockfile, cleared documentation, and individually rights-cleared synthetic examples. Exclude generated build outputs, executables and native DLLs, SDK/runtime bundles, all traineddata and model checkpoints, training corpora and caches, and research environments or model copies. Keep any public test or fixture only when its provenance and redistribution rights have been checked for that specific file. Do not infer binary or model redistribution rights from the project's MIT license.
