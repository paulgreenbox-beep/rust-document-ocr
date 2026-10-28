//! CLI entry point for the OCR pipeline.

use std::process::ExitCode;

use clap::Parser;
use ocr_pipeline::config::Config;

#[cfg(any(feature = "native", test))]
use std::ffi::OsString;
#[cfg(any(feature = "native", test))]
use std::path::{Path, PathBuf};
#[cfg(any(feature = "native", test))]
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
#[cfg(any(feature = "native", test))]
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
};

#[derive(Debug, Parser)]
#[command(
    name = "ocr-pipeline",
    version,
    about = "Local CPU OCR: PDF/images/TIFF/HEIF/DjVu -> DOCX/XLSX/JSON"
)]
struct Cli {
    /// Input document. The decoder is selected from its byte signature.
    #[arg(long)]
    input: std::path::PathBuf,
    #[arg(long)]
    output_dir: std::path::PathBuf,
    /// Full path to PDFium 7881, used only when the input is a PDF.
    #[arg(long)]
    pdfium_dll: Option<std::path::PathBuf>,
    #[arg(long = "ocr-dpi", visible_alias = "dpi", default_value_t = 300)]
    ocr_dpi: u32,
    #[arg(long, default_value_t = 100)]
    grid_dpi: u32,
    /// Tesseract languages joined with '+', for example rus+eng.
    #[arg(long, visible_alias = "language", default_value = "rus+eng")]
    languages: String,
    /// Keep configured languages, use reliable native Chinese evidence, or explicitly select Chinese.
    #[arg(long, value_enum, default_value = "configured")]
    language_routing: ocr_pipeline::LanguageRouting,
    /// Explicit directory containing the requested *.traineddata files.
    #[arg(long)]
    tessdata_path: Option<std::path::PathBuf>,
    #[arg(long)]
    page_limit: Option<u32>,
    #[arg(
        long = "max-page-workers",
        visible_alias = "parallelism",
        default_value_t = 4
    )]
    max_page_workers: usize,
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    enable_cell_ocr_fallback: bool,
    /// Write additive, schema-v1 OCR progress events as JSON Lines. This file
    /// is created only when processing starts and is never written to stderr.
    #[arg(long)]
    progress_jsonl: Option<std::path::PathBuf>,
}

#[cfg_attr(
    not(feature = "native"),
    expect(
        unused_variables,
        reason = "Preserve the accepted OCR baseline during source-only publication."
    )
)]
#[expect(
    clippy::needless_return,
    reason = "Preserve accepted feature-gated CLI control flow during publication."
)]
fn main() -> ExitCode {
    let cli = Cli::parse();
    let progress_jsonl = cli.progress_jsonl.clone();
    let config = Config {
        input: cli.input,
        output_dir: cli.output_dir,
        pdfium_dll_path: cli.pdfium_dll,
        dpi: cli.ocr_dpi,
        grid_dpi: cli.grid_dpi,
        languages: cli.languages,
        language_routing: cli.language_routing,
        tessdata_path: cli.tessdata_path,
        page_limit: cli.page_limit,
        parallelism: cli.max_page_workers,
        enable_cell_ocr_fallback: cli.enable_cell_ocr_fallback,
    };
    if let Err(e) = config.validate() {
        eprintln!("Configuration error: {e}");
        return ExitCode::from(1);
    }

    #[cfg(not(feature = "native"))]
    {
        eprintln!("ERROR: This CLI requires the 'native' feature to process real documents.");
        eprintln!("Build with: cargo build --release --features native");
        eprintln!("Without 'native', only unit/integration tests can run (cargo test).");
        eprintln!();
        eprintln!("Notes for Windows:");
        eprintln!("  - leptonica-sys (used by leptess) and pdfium-render require MSVC toolchain.");
        eprintln!(
            "  - leptonica-sys 0.4.9 with the vcpkg build feature expects vcpkg to be installed"
        );
        eprintln!(
            "    and discoverable. Set the environment variable VCPKG_ROOT to the vcpkg install"
        );
        eprintln!("    directory (the folder that contains vcpkg.exe).");
        eprintln!("  - Use the documented x64-windows-static-md vcpkg triplet for Tesseract.");
        eprintln!("    See INTEGRATION.md; do not use global vcpkg integration for this project.");
        eprintln!("  - Without VCPKG_ROOT the build fails with `error: VcpkgNotFound` while");
        eprintln!("    configuring leptonica-sys; this is a build-time system dependency issue,");
        eprintln!("    NOT a runtime error.");
        eprintln!();
        eprintln!("Runtime requirements (separate from the build above):");
        eprintln!("  - Pdfium runtime: pass its full path with --pdfium-dll, or use the documented bundled runtime layout.");
        eprintln!(
            "  - Tesseract runtime: tesseract.dll on PATH (Windows) and a tessdata directory"
        );
        eprintln!("    reachable via TESSDATA_PREFIX or in the default search path.");
        eprintln!("No output files have been written and no output directory was created.");
        return ExitCode::from(1);
    }

    #[cfg(feature = "native")]
    {
        return run_native_main(&config, progress_jsonl.as_deref());
    }

    // Preserved only as an uncompiled reference while the stable library API
    // takes over native orchestration below. The production CLI now reaches
    // `process_document()` through `run_native_main()`.
    #[cfg(any())]
    {
        // Open the native dependencies first; only stage the output directory after
        // both Pdfium and Tesseract have been successfully initialized. This avoids
        // leaving any output directory (staging or final) behind when the system
        // libraries are missing. The final output-dir must NOT exist yet and its
        // parent must exist; the transaction then creates a sibling staging dir.
        let mut source =
            match ocr_pipeline::PdfiumPageSource::open(&config.input, config.dpi as f64) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to open PDF: {e}");
                    eprintln!("No output directory was created.");
                    return ExitCode::from(1);
                }
            };
        let ocr = match ocr_pipeline::TesseractOcrEngine::new(&config.language) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Failed to initialize OCR engine: {e}");
                eprintln!(
                    "Action: install the Tesseract runtime, ensure tesseract.dll is on PATH,"
                );
                eprintln!(
                    "        and set TESSDATA_PREFIX (or place tessdata/ next to the executable)"
                );
                eprintln!(
                    "        so the requested language (currently `{}`) is available.",
                    config.language
                );
                eprintln!("No output directory was created.");
                return ExitCode::from(1);
            }
        };
        let mut txn = match OutputTransaction::create(&config.output_dir) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Failed to stage output directory: {e}");
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    eprintln!(
                        "Action: remove or rename the existing directory, or pass a fresh --output-dir."
                    );
                } else if e.kind() == std::io::ErrorKind::NotFound {
                    eprintln!(
                        "Action: create the parent directory first; the parent of --output-dir must exist."
                    );
                }
                eprintln!("No output directory was created.");
                return ExitCode::from(1);
            }
        };
        let mut written: Vec<&'static str> = Vec::new();
        let exit = run_native(&config, &mut source, &ocr, &mut txn, &mut written);
        // If the pipeline and all exports succeeded, publish the staging
        // directory as the final output-dir. On any failure path the staging
        // directory is preserved for inspection.
        if exit == ExitCode::SUCCESS {
            if let Err(e) = txn.commit() {
                eprintln!(
                    "Failed to publish results into {}: {e}",
                    config.output_dir.display()
                );
                eprintln!(
                    "Staging directory was preserved for inspection: {}",
                    txn.staging_dir().display()
                );
                eprintln!(
                    "Note: the staging directory was NOT published as the final output directory."
                );
                return ExitCode::from(2);
            }
            // Report final paths to stdout only after a successful commit.
            for name in &written {
                println!("Wrote {}", config.output_dir.join(name).display());
            }
        } else {
            // Pipeline or export failed. The staging directory is intentionally
            // preserved for inspection. The final output-dir was never created.
            eprintln!(
                "Staging directory was preserved for inspection: {}",
                txn.staging_dir().display()
            );
            eprintln!(
                "Note: the staging directory was NOT published as the final output directory."
            );
        }
        exit
    }
}

#[cfg(feature = "native")]
fn run_native_main(config: &Config, progress_jsonl: Option<&Path>) -> ExitCode {
    use ocr_pipeline::{
        export_legacy_document_json, export_ocr_result_docx, export_ocr_result_json,
        export_ocr_result_xlsx, process_document_with_progress, OcrOptions, OcrProgressEvent,
        OcrProgressReporter, OcrProgressStage, TableMode,
    };

    // Validate without creating output or loading native libraries. Recheck
    // when staging and publishing, since another process can create the path.
    if let Err(error) = preflight_output_path(&config.output_dir) {
        eprintln!("Invalid output directory: {error}");
        return ExitCode::from(1);
    }

    let reporter = match progress_jsonl {
        Some(path) => match JsonlProgressReporter::create(path) {
            Ok(reporter) => Some(reporter),
            Err(error) => {
                eprintln!(
                    "Failed to create progress JSON Lines file {}: {error}",
                    path.display()
                );
                eprintln!("OCR warning [progress_write_failed]: progress telemetry is disabled.");
                None
            }
        },
        None => None,
    };
    let progress = reporter
        .as_ref()
        .map(|reporter| reporter as &dyn OcrProgressReporter);

    let options = OcrOptions {
        table_mode: TableMode::GridWithFallback,
        export_title_blocks: true,
        generate_overlays: false,
        #[allow(deprecated)]
        dpi: config.dpi,
        ocr_dpi: config.dpi,
        grid_dpi: config.grid_dpi,
        max_page_workers: config.parallelism,
        enable_cell_ocr_fallback: config.enable_cell_ocr_fallback,
        languages: config.languages.clone(),
        language_routing: config.language_routing,
        #[allow(deprecated)]
        language: "rus+eng".to_string(),
        tessdata_path: config.tessdata_path.clone(),
        page_limit: config.page_limit,
        pdfium_dll_path: config.pdfium_dll_path.clone(),
    };
    let result = match process_document_with_progress(&config.input, &options, progress) {
        Ok(result) => result,
        Err(error) => {
            if let Some(reporter) = progress {
                reporter.report(&OcrProgressEvent::failure(None, 0, error.code()));
            }
            eprintln!("Processing failed [{}]: {error}", error.code());
            eprintln!("No output directory was created.");
            return ExitCode::from(1);
        }
    };
    for warning in &result.warnings {
        eprintln!("OCR warning: {warning:?}");
    }

    // Native dependencies and the full in-process OCR pipeline have already
    // succeeded above. Only now create a sibling staging directory, so the
    // final output remains absent if input validation or initialization fails.
    let mut txn = match OutputTransaction::create(&config.output_dir) {
        Ok(transaction) => transaction,
        Err(error) => {
            eprintln!("Failed to stage output directory: {error}");
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                eprintln!(
                    "Action: pass a fresh --output-dir; existing output is never overwritten."
                );
            } else if error.kind() == std::io::ErrorKind::NotFound {
                eprintln!(
                    "Action: create the parent directory first; the parent of --output-dir must exist."
                );
            }
            eprintln!("No output directory was created.");
            return ExitCode::from(1);
        }
    };

    if let Some(reporter) = progress {
        reporter.report(&OcrProgressEvent::document(
            OcrProgressStage::Export,
            result.document.pages.len() as u32,
            0.0,
        ));
    }
    let mut written: Vec<&'static str> = Vec::new();
    let write_result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let json_path = txn.staging_path("result.json");
        export_legacy_document_json(&result, &json_path)?;
        written.push("result.json");

        let stable_json_path = txn.staging_path("result.v1.json");
        export_ocr_result_json(&result, &stable_json_path)?;
        written.push("result.v1.json");

        let docx_path = txn.staging_path("result.docx");
        export_ocr_result_docx(&result, &docx_path, options.export_title_blocks)?;
        written.push("result.docx");

        let xlsx_path = txn.staging_path("result.xlsx");
        export_ocr_result_xlsx(&result, &xlsx_path, options.export_title_blocks)?;
        written.push("result.xlsx");
        Ok(())
    })();

    if let Err(error) = write_result {
        if let Some(reporter) = progress {
            reporter.report(&OcrProgressEvent::failure(
                None,
                result.document.pages.len() as u32,
                "export_failed",
            ));
        }
        eprintln!("Failed to export results: {error}");
        eprintln!(
            "Staging directory was preserved for inspection: {}",
            txn.staging_dir().display()
        );
        eprintln!("Note: the staging directory was NOT published as the final output directory.");
        return ExitCode::from(2);
    }
    if let Err(error) = txn.commit() {
        if let Some(reporter) = progress {
            reporter.report(&OcrProgressEvent::failure(
                None,
                result.document.pages.len() as u32,
                "output_publish_failed",
            ));
        }
        eprintln!(
            "Failed to publish results into {}: {error}",
            config.output_dir.display()
        );
        eprintln!(
            "Staging directory was preserved for inspection: {}",
            txn.staging_dir().display()
        );
        return ExitCode::from(2);
    }
    for name in written {
        println!("Wrote {}", config.output_dir.join(name).display());
    }
    if let Some(reporter) = progress {
        reporter.report(&OcrProgressEvent::document(
            OcrProgressStage::Completed,
            result.document.pages.len() as u32,
            0.0,
        ));
    }
    ExitCode::SUCCESS
}

#[cfg(any(feature = "native", test))]
struct ProgressWriter<W> {
    writer: W,
    first_error: Option<std::io::Error>,
}

#[cfg(any(feature = "native", test))]
struct JsonlProgressReporter<W = BufWriter<File>> {
    state: Mutex<ProgressWriter<W>>,
}

#[cfg(any(feature = "native", test))]
impl JsonlProgressReporter<BufWriter<File>> {
    fn create(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(Self::from_writer(BufWriter::new(file)))
    }
}

#[cfg(any(feature = "native", test))]
impl<W: Write + Send> JsonlProgressReporter<W> {
    fn from_writer(writer: W) -> Self {
        Self {
            state: Mutex::new(ProgressWriter {
                writer,
                first_error: None,
            }),
        }
    }

    #[cfg(test)]
    fn failure_reason(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .first_error
            .as_ref()
            .map(ToString::to_string)
    }
}

#[cfg(any(feature = "native", test))]
impl<W: Write + Send> ocr_pipeline::OcrProgressReporter for JsonlProgressReporter<W> {
    fn report(&self, event: &ocr_pipeline::OcrProgressEvent) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.first_error.is_some() {
            return;
        }
        let result = (|| -> std::io::Result<()> {
            serde_json::to_writer(&mut state.writer, event).map_err(std::io::Error::other)?;
            state.writer.write_all(b"\n")?;
            state.writer.flush()
        })();
        if let Err(error) = result {
            // Keep the first cause and disable this optional sink. Reporting
            // directly to stderr once avoids recursive progress/log failures.
            let diagnostic = error.to_string();
            state.first_error = Some(error);
            let _ = writeln!(
                std::io::stderr().lock(),
                "OCR warning [progress_write_failed]: {diagnostic}; progress telemetry disabled."
            );
        }
    }
}

#[cfg(feature = "native")]
#[allow(dead_code)]
fn run_native(
    config: &Config,
    source: &mut ocr_pipeline::PdfiumPageSource,
    ocr: &ocr_pipeline::TesseractOcrEngine,
    txn: &mut OutputTransaction,
    written: &mut Vec<&'static str>,
) -> ExitCode {
    use ocr_pipeline::pipeline::Pipeline;
    use ocr_pipeline::preprocessing::DEFAULT_MAX_PIXELS;

    // main() has already verified that Pdfium and Tesseract can be initialized
    // before the staging directory was created, so the native adapters are
    // already valid here. We reuse the same instances to avoid a redundant
    // (and potentially expensive) re-initialization of Tesseract.
    let pipeline = Pipeline::new(source, ocr, DEFAULT_MAX_PIXELS, config.page_limit);

    match pipeline.run() {
        Ok(result) => {
            // Write into the staging directory only. The final output-dir
            // is published atomically by OutputTransaction::commit() in main()
            // after every export has succeeded. If any export below fails,
            // we return early and the staging directory is preserved on disk
            // for inspection, with its path printed to stderr.
            written.push("result.json");
            let json_path = txn.staging_path("result.json");
            if let Err(err) = ocr_pipeline::export_json(&result, &json_path) {
                eprintln!("Failed to export JSON: {err}");
                return ExitCode::from(2);
            }

            written.push("result.docx");
            let docx_path = txn.staging_path("result.docx");
            if let Err(err) = ocr_pipeline::export_docx(&result.pages, &docx_path) {
                eprintln!("Failed to export DOCX: {err}");
                return ExitCode::from(2);
            }

            let tables: Vec<(u32, &ocr_pipeline::TableMatrix)> = result
                .pages
                .iter()
                .flat_map(|p| p.tables.iter().map(move |t| (p.page_number, t)))
                .collect();
            if !tables.is_empty() {
                written.push("result.xlsx");
                let xlsx_path = txn.staging_path("result.xlsx");
                if let Err(err) = ocr_pipeline::export_xlsx(&tables, &xlsx_path) {
                    eprintln!("Failed to export XLSX: {err}");
                    return ExitCode::from(2);
                }
            }

            // All exports succeeded; the final paths are reported from main()
            // after commit, so the user sees the published location rather
            // than the staging directory.
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Pipeline failed: {err}");
            ExitCode::from(3)
        }
    }
}

// ---------------------------------------------------------------------------
// OutputTransaction: atomic publish of the native run's output directory.
//
// All successful artifacts are written into a sibling staging directory that
// lives next to the requested output-dir. Only after the entire pipeline and
// every export step have succeeded is the staging directory renamed to the
// final output-dir in a single directory-level filesystem operation. If any
// stage fails, the staging directory is intentionally preserved on disk for
// inspection and its path is printed to stderr; the final output-dir is
// never created.
//
// The CLI never overwrites or removes a pre-existing output directory: if the
// final path already exists, OutputTransaction::create returns
// io::ErrorKind::AlreadyExists. The parent of the final output-dir must
// already exist; OutputTransaction::create does NOT create it.
// ---------------------------------------------------------------------------

#[cfg(any(feature = "native", test))]
static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(any(feature = "native", test))]
struct OutputTransaction {
    final_dir: PathBuf,
    staging_dir: PathBuf,
    committed: bool,
}

#[cfg(any(feature = "native", test))]
impl OutputTransaction {
    /// Reserve a staging directory next to `final_dir` without creating
    /// `final_dir` itself. Returns an error if `final_dir` already exists
    /// (AlreadyExists) or if the parent of `final_dir` is missing
    /// (NotFound). On success the staging directory exists on disk and
    /// will be preserved on any failure path for inspection.
    #[expect(
        clippy::question_mark,
        reason = "Preserve accepted output transaction control flow during publication."
    )]
    fn create(final_dir: &Path) -> std::io::Result<Self> {
        preflight_output_path(final_dir)?;
        let parent = output_parent(final_dir)?;

        // 3) Compute a unique sibling staging path.
        let final_name = final_dir.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "output directory has no final name component: {}",
                    final_dir.display()
                ),
            )
        })?;
        let pid = std::process::id();
        let counter = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
        // ".staging-<pid>-<counter>" suffix keeps the staging dir hidden in
        // listings and trivially identifiable for crash forensics. The
        // leading dot plus an extra component ensures it can never collide
        // with a legitimate user-named directory.
        let mut staging_name = OsString::from(".");
        staging_name.push(final_name);
        staging_name.push(format!(".staging-{pid}-{counter}"));
        let staging_dir = parent.join(staging_name);

        // 4) Create the staging directory. create_dir (not create_dir_all)
        //    ensures we fail loudly if something raced us to that name.
        if let Err(e) = std::fs::create_dir(&staging_dir) {
            return Err(e);
        }

        Ok(Self {
            final_dir: final_dir.to_path_buf(),
            staging_dir,
            committed: false,
        })
    }

    /// Resolve a path inside the staging directory. The path may or may
    /// not exist; this is just path arithmetic.
    fn staging_path(&self, name: &str) -> PathBuf {
        self.staging_dir.join(name)
    }

    /// Expose the staging path for diagnostics on the failure path.
    fn staging_dir(&self) -> &Path {
        &self.staging_dir
    }

    /// Publish the staging directory as the final output directory.
    /// After commit returns Ok, the staging directory no longer exists
    /// (it has been renamed to `final_dir`).
    fn commit(&mut self) -> std::io::Result<()> {
        debug_assert!(!self.committed, "commit called twice on OutputTransaction");
        // A contender may have created final_dir after staging began. Windows
        // directory rename also refuses an existing destination, closing the
        // check/rename race on the validated Windows target. Do not pre-delete.
        preflight_output_path(&self.final_dir)?;
        std::fs::rename(&self.staging_dir, &self.final_dir)?;
        self.committed = true;
        Ok(())
    }
}

#[cfg(any(feature = "native", test))]
fn output_parent(final_dir: &Path) -> std::io::Result<&Path> {
    if final_dir.file_name().is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "output directory has no final name component",
        ));
    }
    Ok(final_dir
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new(".")))
}

/// Read-only early validation; staging and commit repeat the same checks.
#[cfg(any(feature = "native", test))]
fn preflight_output_path(final_dir: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(final_dir) {
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("output directory already exists: {}", final_dir.display()),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let parent = output_parent(final_dir)?;
    let metadata = std::fs::metadata(parent)?;
    if !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("output parent is not a directory: {}", parent.display()),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod audit_tests {
    use super::*;
    use ocr_pipeline::{OcrProgressEvent, OcrProgressReporter, OcrProgressStage};
    use std::sync::Arc;

    #[test]
    fn existing_output_dir_rejected_before_ocr() {
        let parent = tempfile::tempdir().unwrap();
        let output = parent.path().join("existing");
        std::fs::create_dir(&output).unwrap();
        std::fs::write(output.join("keep.txt"), b"existing result").unwrap();
        assert_eq!(
            preflight_output_path(&output).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            std::fs::read(output.join("keep.txt")).unwrap(),
            b"existing result"
        );
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 1);
    }

    #[test]
    fn missing_output_parent_rejected_before_ocr() {
        let parent = tempfile::tempdir().unwrap();
        let output = parent.path().join("missing/output");
        assert_eq!(
            preflight_output_path(&output).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);
    }

    #[test]
    fn valid_output_still_uses_atomic_staging() {
        let parent = tempfile::tempdir().unwrap();
        let output = parent.path().join("output");
        preflight_output_path(&output).unwrap();
        assert!(!output.exists());
        let mut transaction = OutputTransaction::create(&output).unwrap();
        let staging = transaction.staging_dir().to_path_buf();
        std::fs::write(transaction.staging_path("result.json"), b"{}").unwrap();
        assert!(!output.exists());
        transaction.commit().unwrap();
        assert!(!staging.exists());
        assert_eq!(std::fs::read(output.join("result.json")).unwrap(), b"{}");
    }

    #[test]
    fn concurrent_output_creation_does_not_overwrite_data() {
        for populated in [false, true] {
            let parent = tempfile::tempdir().unwrap();
            let output = parent.path().join("output");
            let mut first = OutputTransaction::create(&output).unwrap();
            let mut second = OutputTransaction::create(&output).unwrap();
            std::fs::write(first.staging_path("result.json"), b"first").unwrap();
            std::fs::write(second.staging_path("result.json"), b"second").unwrap();
            if populated {
                second.commit().unwrap();
            } else {
                std::fs::create_dir(&output).unwrap();
            }
            assert_eq!(
                first.commit().unwrap_err().kind(),
                std::io::ErrorKind::AlreadyExists
            );
            assert!(first.staging_dir().exists());
            if populated {
                assert_eq!(
                    std::fs::read(output.join("result.json")).unwrap(),
                    b"second"
                );
            } else {
                assert_eq!(std::fs::read_dir(&output).unwrap().count(), 0);
            }
        }
    }

    struct FailingWriter {
        attempts: Arc<AtomicU64>,
        fail_flush: bool,
    }

    impl Write for FailingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.attempts.fetch_add(1, Ordering::Relaxed);
            if self.fail_flush {
                Ok(bytes.len())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::StorageFull,
                    "synthetic write failure",
                ))
            }
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "synthetic flush failure",
            ))
        }
    }

    #[test]
    fn progress_write_failure_is_observable() {
        for fail_flush in [false, true] {
            let attempts = Arc::new(AtomicU64::new(0));
            let reporter = JsonlProgressReporter::from_writer(FailingWriter {
                attempts: attempts.clone(),
                fail_flush,
            });
            let event = OcrProgressEvent::document(OcrProgressStage::Completed, 2, 0.0);
            reporter.report(&event);
            let reason = reporter.failure_reason().unwrap();
            assert!(reason.contains(if fail_flush {
                "flush failure"
            } else {
                "write failure"
            }));
            let first_attempts = attempts.load(Ordering::Relaxed);
            assert!(first_attempts > 0);
            for _ in 0..10 {
                reporter.report(&event);
            }
            assert_eq!(attempts.load(Ordering::Relaxed), first_attempts);
            assert_eq!(reporter.failure_reason().as_deref(), Some(reason.as_str()));
        }
    }

    #[test]
    fn progress_jsonl_events_keep_the_existing_schema_and_order() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("progress.jsonl");
        let reporter = JsonlProgressReporter::create(&path).unwrap();
        let events = [
            OcrProgressEvent::document(OcrProgressStage::Export, 2, 0.0),
            OcrProgressEvent::document(OcrProgressStage::Completed, 2, 0.0),
        ];
        for event in &events {
            reporter.report(event);
        }
        assert!(reporter.failure_reason().is_none());
        let actual = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<serde_json::Value> = actual
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            parsed,
            events
                .iter()
                .map(|event| serde_json::to_value(event).unwrap())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            JsonlProgressReporter::create(&path).err().unwrap().kind(),
            std::io::ErrorKind::AlreadyExists
        );
    }
}
