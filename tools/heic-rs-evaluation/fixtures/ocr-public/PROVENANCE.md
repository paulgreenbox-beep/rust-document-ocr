# Synthetic text HEIC inputs for pipeline A/B

Unmodified files from [Cloud2BR/OCR-IMG2PDF-Converter](https://github.com/Cloud2BR/OCR-IMG2PDF-Converter/tree/33db9cd8021e703d5100380538d3b8a8400cbe79/test-fixtures),
commit `33db9cd8021e703d5100380538d3b8a8400cbe79`. The pinned upstream
README identifies these as generic synthetic local-development fixtures with
no real user data. Its exact bytes are retained as UPSTREAM_README.md.

Repository terms are Apache-2.0; the original LICENSE bytes are retained as
UPSTREAM_LICENSE.txt. Source SVGs for form, note and receipt are included
unmodified alongside their HEIC variants. No fonts, codec tools or native
libraries are copied. The exact historic encoder configuration is not asserted
by this receipt: these are upstream-produced bytes, not locally re-encoded
images. Source/blob/size/SHA-256 identities are in SOURCE_MANIFEST.json.

SVG text elements provide a known synthetic transcription for token metrics;
they are not a human-reviewed engineering-document holdout. Normalization is
Unicode-word tokenization and case folding only: no D/O, 2/7, script or numeric
substitutions. This corpus supplements the tiny format-only fixtures; it does
not establish camera-photo, Russian/CAD-text or universal HEIF accuracy.
