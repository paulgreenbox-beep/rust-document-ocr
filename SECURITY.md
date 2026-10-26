# Security policy

Rust Document OCR runs locally as a command-line program or an embedded Rust
library. It has no hosted OCR endpoint. Its native build and runtime depend on
locally supplied components such as PDFium, Tesseract, Leptonica, codec
libraries, and language data.

## Reporting a vulnerability

Please do not disclose exploit details in a public issue or pull request.

If private vulnerability reporting is enabled for the repository, submit the
report through GitHub's private vulnerability reporting flow. This policy does
not claim that the feature is currently enabled.

If private reporting is unavailable, ask the repository owner for a private
reporting channel without posting sensitive technical details publicly. Use
only a contact method published by the repository owner; no maintainer email
address is asserted here.

Include the affected package version, operating system, enabled Cargo
features, relevant native dependency versions, and a concise reproduction
description. Do not attach private PDFs, OCR output, traineddata, or credentials
unless the maintainer has provided an approved private transfer method.

## Handling document data

Keep source documents and OCR results local and access-controlled. When
reporting an issue, use a minimal redistributable fixture or a synthetic
reproduction. The optional `--progress-jsonl` channel contains pipeline
progress metadata rather than recognized document text.
