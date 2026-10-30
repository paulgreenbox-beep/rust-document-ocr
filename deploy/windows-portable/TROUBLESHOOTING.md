# Portable troubleshooting

- Run `verify.ps1` before processing. A mismatch means the payload is incomplete
  or changed. Obtain the complete verified package; do not download random DLLs.
- Keep `pdfium.dll` and all supplied dependencies next to `ocr-pipeline.exe`.
  A Windows loader error can occur before the application can print diagnostics.
- Keep `eng.traineddata` and `rus.traineddata` in the adjacent `tessdata` folder.
  Explicit `--tessdata-path` overrides `TESSDATA_PREFIX`, which overrides the
  adjacent directory. Invalid explicit paths intentionally fail.
- Use Windows x64. DLL architecture and PDFium API (7881) must match the build.
- Use a fresh output directory with an existing writable parent.
- A local validation candidate is not approved for redistribution. Review
  `release-manifest.json`, component notices and release gate evidence.
- The smoke runner deliberately clears inherited development PATH entries and
  runtime overrides only in its child process environment, then restores them.
