# Windows portable release troubleshooting

- **Missing PDFium:** keep the API-7881 x64 DLL beside `ocr-pipeline.exe`.
  Portable-marker mode intentionally does not load a developer's fallback DLL.
  Do not copy DLLs to System32 or change global PATH.
- **Missing traineddata:** retain both `tessdata/eng.traineddata` and
  `tessdata/rus.traineddata`. An existing `TESSDATA_PREFIX` overrides the local
  directory; clear/fix it for the current process only. An explicit invalid
  `--tessdata-path` is an error even when local data exists.
- **Cyrillic paths / TessInitError:** Windows narrow-character native model
  loading uses an API-owned ASCII temporary mirror. It does not change models.
  A non-ASCII TEMP without an ASCII Windows short alias needs an explicitly
  writable ASCII TEMP for that process. Do not install another OCR runtime.
- **Missing native import:** use the PE closure report to identify the exact
  dependency. Supply only the version/hash/notice-reviewed app-local file.
  Do not add DLLs speculatively or copy from System32 into a distributable ZIP.
- **Output already exists:** choose a new output directory whose parent exists.
  Staging/publish semantics remain unchanged; existing results are not overwritten.
- **Baseline difference:** stop release preparation and inspect the semantic
  comparison report. Timings and volatile Office metadata are excluded, but
  text, geometry, confidence, routing and document XML are not waived.
- **Blocked ZIP/license review:** this is intentional. Notices alone may not
  establish the exact binary origin or satisfy corresponding-source/relink
  obligations. Resolve the named component review rather than flipping a flag.
- **Clean-environment gate missing:** a controlled PATH on a developer machine
  is not an unprovisioned Windows VM. Record the separate test as NOT RUN.
- **PowerShell restrictions:** use your organization's approved script policy;
  the package does not change execution policy, elevation or system settings.
- **Timeout:** smoke retains stdout/stderr and terminates only its own child
  process tree. Retain the evidence and diagnose the last reported stage;
  never kill unrelated OCR/service processes.

The `verify.ps1` file checks integrity, not publisher identity. A public release
requires reviewed provenance and separate artifact authentication/distribution
decisions. See [PORTABLE_WINDOWS.md](PORTABLE_WINDOWS.md).
