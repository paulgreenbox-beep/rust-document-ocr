//! Streaming OCR pipeline.
//!
//! Owns a [`PageSource`] and an [`OcrEngine`]. For each page:
//! 1. inspect native text -> 2. render/orient when OCR is needed ->
//! 3. preprocess -> 4. detect_table
//!    - if table: OCR words + reconstruct a [`TableMatrix`]
//!    - else:    OCR plain text
//! 4. collect a [`PageResult`], then drop the page's bitmaps before
//!    moving on to the next page so peak memory stays bounded.

use std::{
    any::Any,
    collections::HashMap,
    panic::AssertUnwindSafe,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Instant,
};

use image::{imageops, GrayImage};
use imageproc::contrast::otsu_level;
use serde::{Deserialize, Serialize};

use crate::borderless_table::detect_borderless_tables;
use crate::detector::{detect_table, DetectionResult};
use crate::dto::{BoundingBox, DocumentResult, PageKind, PageResult, TableMatrix, TableType};
use crate::language::{effective_languages, select_language_profile, LanguageRouting};
use crate::ocr::LanguageRoutedOcrEngine;
use crate::ocr::{OcrEngine, OcrLanguageProfile, OcrRequest};
use crate::orientation::{
    choose_orientation, orientation_thumbnail, rotate_gray_image, OrientationEvidence,
    ORIENTATION_ANGLES,
};
use crate::page_analysis::{
    analyze_native_text_for_languages, NativeTextLayer, NativeTextLayout, NativeTextQuality,
    PageOrientation, PageProcessingInfo, PageStageTimings, PageWordAssignmentMetrics,
    ProcessingMode, RenderSize,
};
use crate::page_source::{
    dimensions_within_pixel_limit, PageSource, RenderedPage, RenderedPageRegion,
};
use crate::preprocessing::preprocess;
use crate::progress::{OcrProgressEvent, OcrProgressReporter, OcrProgressStage};
use crate::quality::assess_page_quality;
use crate::reconstruction::reconstruct_table;
use crate::table_grid::{classify_grid, detect_table_grids, TableAssignmentMetrics, TableGrid};
use crate::table_semantics::{classify_table, ColumnRole};

/// Tolerances for the (currently stub) reconstruction routine.
const Y_TOLERANCE: f64 = 5.0;
const X_TOLERANCE: f64 = 10.0;
const DEFAULT_GRID_DPI: u32 = 100;
const DEFAULT_CELL_OCR_DPI: u32 = 300;
// Tesseract emits native diagnostics for very narrow fallback crops (for
// example 2x36 px). Besides being below a meaningful OCR size, repeated
// diagnostics can fill an embedding application's stderr pipe. Keep the
// fallback conservative: it supplements empty grid cells only.
const MIN_CELL_OCR_WIDTH: u32 = 8;
const MIN_CELL_OCR_HEIGHT: u32 = 8;
const CELL_OCR_TARGET_INK_HEIGHT: u32 = 30;
const CELL_OCR_MAX_SCALE: u32 = 4;
const CELL_OCR_MIN_CONFIDENCE: f32 = 0.05;
const CELL_OCR_GRID_NOISE_CONFIDENCE: f32 = 0.65;
const TECHNICAL_CELL_RECHECK_LIMIT: usize = 24;
const TECHNICAL_ROLE_MIN_CONFIDENCE: f32 = 0.70;
const TECHNICAL_MODEL_MIN_CONFIDENCE: f32 = 0.55;
/// Short searchable layers can be labels over a larger raster page. Only
/// these ambiguous layers pay for a bounded native-vs-OCR arbitration pass.
const NATIVE_TEXT_ARBITRATION_NON_WHITESPACE: usize = 40;

/// Table reconstruction policy used by both the library integration API and
/// the CLI. The default keeps the established grid-first, legacy-fallback
/// behavior.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TableMode {
    /// Skip all table reconstruction and return page OCR text only.
    Off,
    /// Use only the legacy OCR-word alignment heuristic.
    Legacy,
    /// Use only ruled-grid reconstruction; non-grid pages remain text pages.
    Grid,
    /// Prefer ruled-grid reconstruction, then use the legacy heuristic.
    #[default]
    GridWithFallback,
}

/// A non-fatal condition encountered while processing a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineWarning {
    /// Grid reconstruction was unavailable for one page; OCR continued using
    /// the configured fallback policy.
    GridDetectorFailed { page_number: u32, message: String },
    /// The source could not read a native text layer, so this page continued
    /// through raster OCR rather than failing the entire document.
    NativeTextUnavailable { page_number: u32, message: String },
    /// Native text was readable, but its optional geometry analysis failed.
    /// The raster grid check remains available, so this does not fail the page.
    NativeTextLayoutUnavailable { page_number: u32, message: String },
    /// Native text remains usable, but coordinate-bearing words were not
    /// available for grid assignment. Empty cells may use the configured
    /// low-resolution OCR fallback.
    NativeWordGeometryUnavailable { page_number: u32, message: String },
    /// A usable native-text page could not be rendered for its compact
    /// ruled-grid routing check. The page safely remains NativeText.
    TableRoutingPreflightFailed { page_number: u32, message: String },
    /// OCR orientation preflight was unavailable. The page was retained at
    /// its rendered orientation and standard OCR continued.
    OrientationDetectionFailed { page_number: u32, message: String },
    /// One optional cell recognition failed; its original content and physical
    /// geometry were retained. Indices are zero-based except page_number.
    CellOcrFallbackFailed {
        page_number: u32,
        table_index: usize,
        row: u32,
        column: u32,
        reason_code: String,
    },
}

/// The full pipeline result plus non-fatal diagnostics.
#[derive(Debug)]
pub struct PipelineOutcome {
    pub document: DocumentResult,
    pub warnings: Vec<PipelineWarning>,
    pub page_processing: Vec<PageProcessingInfo>,
}

/// Pipeline instance. The page source is borrowed mutably because each
/// page render may consume resources the source caches internally.
pub struct Pipeline<'a, S: PageSource, O: OcrEngine> {
    source: &'a mut S,
    ocr: &'a O,
    max_pixels: usize,
    page_limit: Option<u32>,
    table_mode: TableMode,
    languages: String,
    language_routing: LanguageRouting,
    grid_dpi: u32,
    cell_ocr_dpi: u32,
    enable_cell_ocr_fallback: bool,
    progress: Option<&'a dyn OcrProgressReporter>,
}

impl<'a, S, O> Pipeline<'a, S, O>
where
    S: PageSource,
    O: OcrEngine,
{
    pub fn new(source: &'a mut S, ocr: &'a O, max_pixels: usize, page_limit: Option<u32>) -> Self {
        Self {
            source,
            ocr,
            max_pixels,
            page_limit,
            table_mode: TableMode::default(),
            languages: "rus+eng".to_string(),
            language_routing: LanguageRouting::Configured,
            grid_dpi: DEFAULT_GRID_DPI,
            cell_ocr_dpi: DEFAULT_CELL_OCR_DPI,
            enable_cell_ocr_fallback: true,
            progress: None,
        }
    }

    /// Set the table reconstruction policy while preserving all other pipeline
    /// configuration.
    pub fn with_table_mode(mut self, table_mode: TableMode) -> Self {
        self.table_mode = table_mode;
        self
    }

    /// Set the configured Tesseract language string recorded in page
    /// diagnostics. Native-text pages retain this configuration but do not
    /// instantiate Tesseract.
    pub fn with_languages(mut self, languages: impl Into<String>) -> Self {
        self.languages = languages.into();
        self
    }

    pub fn with_grid_dpi(mut self, grid_dpi: u32) -> Self {
        self.grid_dpi = grid_dpi;
        self
    }

    pub fn with_language_routing(mut self, language_routing: LanguageRouting) -> Self {
        self.language_routing = language_routing;
        self
    }

    /// Set the density used only for bounded empty-cell ROI renders.
    pub fn with_cell_ocr_dpi(mut self, cell_ocr_dpi: u32) -> Self {
        self.cell_ocr_dpi = cell_ocr_dpi;
        self
    }

    pub fn with_cell_ocr_fallback(mut self, enabled: bool) -> Self {
        self.enable_cell_ocr_fallback = enabled;
        self
    }

    /// Adds an optional, non-textual progress observer for this run.
    pub fn with_progress(mut self, progress: Option<&'a dyn OcrProgressReporter>) -> Self {
        self.progress = progress;
        self
    }

    /// Execute the pipeline against every page reported by the source.
    pub fn run(self) -> Result<DocumentResult, Box<dyn std::error::Error>> {
        Ok(self.run_with_warnings()?.document)
    }

    /// Execute the pipeline and retain non-fatal per-page diagnostics.
    pub fn run_with_warnings(self) -> Result<PipelineOutcome, Box<dyn std::error::Error>> {
        let Pipeline {
            source,
            ocr,
            max_pixels,
            page_limit,
            table_mode,
            languages,
            language_routing,
            grid_dpi,
            cell_ocr_dpi,
            enable_cell_ocr_fallback,
            progress,
        } = self;

        let total = source.total_pages()?;
        let effective = page_limit.map_or(total, |limit| limit.min(total));
        let config = WorkerConfig {
            max_pixels,
            table_mode,
            languages,
            enable_cell_ocr_fallback,
        };
        let mut pages = Vec::with_capacity(effective as usize);
        let mut warnings = Vec::new();
        let mut page_processing = Vec::with_capacity(effective as usize);

        for page_number in 1..=effective {
            report_page_started(progress, page_number, effective);
            let prepared = prepare_page(
                source,
                page_number,
                max_pixels,
                table_mode,
                grid_dpi,
                cell_ocr_dpi,
                enable_cell_ocr_fallback,
                &config.languages,
                language_routing,
            )?;
            let processed = finish_prepared_page(prepared, ocr, &config)
                .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
            report_page_progress(progress, &processed.processing, effective);
            pages.push(processed.page);
            page_processing.push(processed.processing);
            warnings.extend(processed.warnings);
        }
        report_document_progress(progress, OcrProgressStage::Aggregation, effective);

        Ok(PipelineOutcome {
            document: DocumentResult::new(pages),
            warnings,
            page_processing,
        })
    }

    /// Execute page CPU/OCR work on a bounded worker pool while keeping every
    /// [`PageSource`] call on the producer thread.
    ///
    /// PDFium-backed sources are intentionally never shared with workers. The
    /// producer extracts native text and renders each page into an owned
    /// bitmap, performs the compact grid preflight needed to request safe
    /// high-detail cell ROIs, then hands only Rust-owned images to the bounded
    /// queue. Workers perform orientation, preprocessing, OCR, and word
    /// assignment. Results are sorted by page number before publication, so
    /// scheduling cannot change document order.
    ///
    /// A worker count of one uses the established sequential path exactly.
    pub fn run_with_warnings_parallel(
        self,
        max_workers: usize,
    ) -> Result<PipelineOutcome, Box<dyn std::error::Error>>
    where
        O: Sync,
    {
        if max_workers == 0 {
            return Err("max page workers must be greater than zero".into());
        }
        if max_workers == 1 {
            return self.run_with_warnings();
        }

        let Pipeline {
            source,
            ocr,
            max_pixels,
            page_limit,
            table_mode,
            languages,
            language_routing,
            grid_dpi,
            cell_ocr_dpi,
            enable_cell_ocr_fallback,
            progress,
        } = self;

        let total = source.total_pages()?;
        let effective = page_limit.map_or(total, |limit| limit.min(total));
        if effective == 0 {
            return Ok(PipelineOutcome {
                document: DocumentResult::new(Vec::new()),
                warnings: Vec::new(),
                page_processing: Vec::new(),
            });
        }

        let worker_count = max_workers.min(effective as usize);
        let config = WorkerConfig {
            max_pixels,
            table_mode,
            languages,
            enable_cell_ocr_fallback,
        };

        let processed = thread::scope(|scope| {
            // Capacity one bounds queued bitmap memory independently of the
            // configured worker count. At most one prepared page waits while
            // each worker owns one active page.
            let (work_sender, work_receiver) = mpsc::sync_channel::<PreparedPage>(1);
            let work_receiver = Arc::new(Mutex::new(work_receiver));
            let (result_sender, result_receiver) = mpsc::channel::<WorkerResult>();
            let mut handles = Vec::with_capacity(worker_count);

            for _ in 0..worker_count {
                let work_receiver = Arc::clone(&work_receiver);
                let result_sender = result_sender.clone();
                let config = config.clone();
                handles.push(scope.spawn(move || -> Result<(), String> {
                    loop {
                        let work = {
                            let receiver = work_receiver
                                .lock()
                                .map_err(|_| "page work queue lock was poisoned".to_string())?;
                            receiver.recv()
                        };
                        let Ok(work) = work else {
                            return Ok(());
                        };
                        let page_number = work.page_number;
                        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                            finish_prepared_page(work, ocr, &config)
                        }))
                        .map_err(|payload| {
                            format!(
                                "page {page_number} worker panicked: {}",
                                panic_message(payload)
                            )
                        })
                        .and_then(|result| result);
                        if result_sender
                            .send(WorkerResult {
                                page_number,
                                result,
                            })
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                }));
            }
            drop(result_sender);

            let mut producer_error = None;
            for page_number in 1..=effective {
                report_page_started(progress, page_number, effective);
                let prepared = match prepare_page(
                    source,
                    page_number,
                    max_pixels,
                    table_mode,
                    grid_dpi,
                    cell_ocr_dpi,
                    enable_cell_ocr_fallback,
                    &config.languages,
                    language_routing,
                ) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        report_failure(
                            progress,
                            Some(page_number),
                            effective,
                            "page_prepare_failed",
                        );
                        producer_error = Some(format!(
                            "failed to prepare page {page_number} on PDF producer thread: {error}"
                        ));
                        break;
                    }
                };
                if work_sender.send(prepared).is_err() {
                    producer_error = Some("all page workers stopped unexpectedly".to_string());
                    break;
                }
            }
            drop(work_sender);

            // Drain every result before joining. This guarantees that all
            // successfully prepared pages release their owned bitmaps even if
            // another page failed.
            let mut results = Vec::new();
            let mut worker_error = None;
            for message in result_receiver {
                match message.result {
                    Ok(processed) => {
                        report_page_progress(progress, &processed.processing, effective);
                        results.push(processed);
                    }
                    Err(error) if worker_error.is_none() => {
                        report_failure(
                            progress,
                            Some(message.page_number),
                            effective,
                            "page_processing_failed",
                        );
                        worker_error = Some(error);
                    }
                    Err(_) => {}
                }
            }

            for handle in handles {
                match handle.join() {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) if worker_error.is_none() => worker_error = Some(error),
                    Ok(Err(_)) => {}
                    Err(payload) if worker_error.is_none() => {
                        worker_error = Some(format!(
                            "page worker thread panicked: {}",
                            panic_message(payload)
                        ));
                    }
                    Err(_) => {}
                }
            }

            if let Some(error) = producer_error.or(worker_error) {
                return Err(error);
            }
            results.sort_unstable_by_key(|result| result.page_number);
            if results.len() != effective as usize {
                return Err(format!(
                    "page worker pool returned {} results for {effective} prepared pages",
                    results.len()
                ));
            }
            Ok(results)
        })
        .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;

        let mut pages = Vec::with_capacity(processed.len());
        let mut warnings = Vec::new();
        let mut page_processing = Vec::with_capacity(processed.len());
        for processed_page in processed {
            pages.push(processed_page.page);
            page_processing.push(processed_page.processing);
            warnings.extend(processed_page.warnings);
        }
        report_document_progress(progress, OcrProgressStage::Aggregation, effective);

        Ok(PipelineOutcome {
            document: DocumentResult::new(pages),
            warnings,
            page_processing,
        })
    }
}

#[derive(Clone)]
struct WorkerConfig {
    max_pixels: usize,
    table_mode: TableMode,
    languages: String,
    enable_cell_ocr_fallback: bool,
}

struct PreparedPage {
    page_number: u32,
    page_started: Instant,
    language_profile: OcrLanguageProfile,
    effective_languages: String,
    native_text_quality: Option<NativeTextQuality>,
    native_text_layout: Option<NativeTextLayout>,
    timings: PageStageTimings,
    warnings: Vec<PipelineWarning>,
    work: Option<PreparedWork>,
}

enum PreparedWork {
    NativeText {
        layer: NativeTextLayer,
        orientation: PageOrientation,
        grid_render: Option<PreparedGridRender>,
    },
    Ocr {
        image: GrayImage,
        dpi: f64,
        grid_thumbnail: Option<RenderedPage>,
        native_candidate: Option<NativeTextLayer>,
        native_orientation: Option<PageOrientation>,
    },
}

struct PreparedGridRender {
    rendered: RenderedPage,
    native_words: Vec<crate::dto::Word>,
    grid_candidates: Vec<TableGrid>,
    cell_ocr_regions: Vec<Option<RenderedPageRegion>>,
}

struct ProcessedPage {
    page_number: u32,
    page: PageResult,
    processing: PageProcessingInfo,
    warnings: Vec<PipelineWarning>,
}

struct WorkerResult {
    page_number: u32,
    result: Result<ProcessedPage, String>,
}

/// Performs every source/PDFium operation serially on the producer thread.
/// The returned page owns all raster and text data used by worker stages.
#[expect(
    clippy::too_many_arguments,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
fn prepare_page<S: PageSource>(
    source: &mut S,
    page_number: u32,
    max_pixels: usize,
    table_mode: TableMode,
    grid_dpi: u32,
    cell_ocr_dpi: u32,
    enable_cell_ocr_fallback: bool,
    languages: &str,
    language_routing: LanguageRouting,
) -> Result<PreparedPage, Box<dyn std::error::Error>> {
    let page_started = Instant::now();
    let mut timings = PageStageTimings::default();
    let mut warnings = Vec::new();

    let stage_started = Instant::now();
    let native_text_layer = match source.native_text_for_page(page_number) {
        Ok(layer) => layer,
        Err(error) => {
            warnings.push(PipelineWarning::NativeTextUnavailable {
                page_number,
                message: error.to_string(),
            });
            None
        }
    };
    timings.native_text_extraction_ms = elapsed_ms(stage_started);
    let language_profile = select_language_profile(
        language_routing,
        languages,
        native_text_layer.as_ref().map(|layer| layer.text.as_str()),
    );
    let page_languages = effective_languages(languages, language_profile);
    let native_text_quality = native_text_layer
        .as_ref()
        .map(|layer| analyze_native_text_for_languages(layer, &page_languages));
    let native_text_usable = native_text_quality
        .as_ref()
        .is_some_and(|quality| quality.is_usable);
    let native_text_needs_ocr_arbitration = native_text_quality.as_ref().is_some_and(|quality| {
        quality.is_near_usable
            || (quality.is_usable
                && quality.non_whitespace_characters < NATIVE_TEXT_ARBITRATION_NON_WHITESPACE)
    });

    let native_text_layout = if native_text_usable {
        let stage_started = Instant::now();
        let layout = match source.native_text_layout_for_page(page_number) {
            Ok(layout) => layout,
            Err(error) => {
                warnings.push(PipelineWarning::NativeTextLayoutUnavailable {
                    page_number,
                    message: error.to_string(),
                });
                None
            }
        };
        timings.native_text_layout_ms = elapsed_ms(stage_started);
        layout
    } else {
        None
    };

    let work = if native_text_usable && !native_text_needs_ocr_arbitration {
        let orientation = source.page_orientation_from_metadata(page_number)?;
        let grid_render = if matches!(table_mode, TableMode::Grid | TableMode::GridWithFallback) {
            let stage_started = Instant::now();
            let rendered =
                source.render_page_at_dpi_with_max_pixels(page_number, grid_dpi, max_pixels);
            timings.thumbnail_render_ms += elapsed_ms(stage_started);
            match rendered {
                Ok(rendered) => {
                    let (width, height) = rendered.image.dimensions();
                    let stage_started = Instant::now();
                    let native_words =
                        match source.native_words_for_rendered_page(page_number, width, height) {
                            Ok(Some(words)) => words,
                            Ok(None) => {
                                warnings.push(PipelineWarning::NativeWordGeometryUnavailable {
                                page_number,
                                message:
                                    "page source does not expose coordinate-bearing native words"
                                        .to_string(),
                            });
                                Vec::new()
                            }
                            Err(error) => {
                                warnings.push(PipelineWarning::NativeWordGeometryUnavailable {
                                    page_number,
                                    message: error.to_string(),
                                });
                                Vec::new()
                            }
                        };
                    timings.native_text_extraction_ms += elapsed_ms(stage_started);
                    let stage_started = Instant::now();
                    let grid_candidates = match detect_table_grids_safely(&rendered.image) {
                        Ok(grids) => grids,
                        Err(message) => {
                            warnings.push(PipelineWarning::GridDetectorFailed {
                                page_number,
                                message,
                            });
                            Vec::new()
                        }
                    };
                    timings.grid_detection_ms += elapsed_ms(stage_started);
                    let cell_ocr_regions = if enable_cell_ocr_fallback {
                        let thumbnail_pixels = usize::try_from(rendered.image.width())
                            .ok()
                            .and_then(|width| {
                                width.checked_mul(usize::try_from(rendered.image.height()).ok()?)
                            })
                            .unwrap_or(max_pixels);
                        let roi_pixel_budget = max_pixels.saturating_sub(thumbnail_pixels);
                        prepare_grid_cell_ocr_regions(
                            source,
                            page_number,
                            &rendered,
                            &native_words,
                            &grid_candidates,
                            cell_ocr_dpi,
                            roi_pixel_budget,
                            &mut timings,
                            &mut warnings,
                        )
                    } else {
                        (0..grid_candidates.len()).map(|_| None).collect()
                    };
                    Some(PreparedGridRender {
                        rendered,
                        native_words,
                        grid_candidates,
                        cell_ocr_regions,
                    })
                }
                Err(error) => {
                    warnings.push(PipelineWarning::TableRoutingPreflightFailed {
                        page_number,
                        message: error.to_string(),
                    });
                    None
                }
            }
        } else {
            None
        };

        PreparedWork::NativeText {
            layer: native_text_layer.expect("usable native text layer must exist"),
            orientation,
            grid_render,
        }
    } else {
        let native_orientation = if native_text_needs_ocr_arbitration {
            Some(source.page_orientation_from_metadata(page_number)?)
        } else {
            None
        };
        let native_candidate = if native_text_needs_ocr_arbitration {
            native_text_layer
        } else {
            None
        };
        let stage_started = Instant::now();
        let rendered = source.render_page_with_max_pixels(page_number, max_pixels)?;
        timings.full_render_ms += elapsed_ms(stage_started);
        let grid_thumbnail = if matches!(table_mode, TableMode::Grid | TableMode::GridWithFallback)
            && (rendered.dpi - f64::from(grid_dpi)).abs() > f64::EPSILON
        {
            let stage_started = Instant::now();
            let thumbnail =
                render_bounded_grid_thumbnail(source, page_number, &rendered, grid_dpi, max_pixels);
            timings.thumbnail_render_ms += elapsed_ms(stage_started);
            match thumbnail {
                Ok(thumbnail) => Some(thumbnail),
                Err(error) => {
                    warnings.push(PipelineWarning::TableRoutingPreflightFailed {
                        page_number,
                        message: error.to_string(),
                    });
                    None
                }
            }
        } else {
            None
        };
        PreparedWork::Ocr {
            image: rendered.image,
            dpi: rendered.dpi,
            grid_thumbnail,
            native_candidate,
            native_orientation,
        }
    };

    Ok(PreparedPage {
        page_number,
        page_started,
        language_profile,
        effective_languages: page_languages,
        native_text_quality,
        native_text_layout,
        timings,
        warnings,
        work: Some(work),
    })
}

fn finish_prepared_page<O: OcrEngine>(
    mut prepared: PreparedPage,
    ocr: &O,
    config: &WorkerConfig,
) -> Result<ProcessedPage, String> {
    let routed = LanguageRoutedOcrEngine::new(ocr, prepared.language_profile).map_err(|error| {
        format!(
            "page {} language route failed: {error}",
            prepared.page_number
        )
    })?;
    let ocr = &routed;
    let mut page_config = config.clone();
    page_config.languages = prepared.effective_languages.clone();
    let config = &page_config;
    ocr.begin_page().map_err(|error| {
        format!(
            "page {} OCR initialization failed: {error}",
            prepared.page_number
        )
    })?;
    let work = prepared
        .work
        .take()
        .ok_or_else(|| format!("page {} has no prepared work", prepared.page_number))?;
    let mut processed = match work {
        PreparedWork::NativeText {
            layer,
            orientation,
            grid_render,
        } => finish_prepared_native_page(prepared, layer, orientation, grid_render, ocr, config),
        PreparedWork::Ocr {
            image,
            dpi,
            grid_thumbnail,
            native_candidate,
            native_orientation,
        } => finish_prepared_ocr_page(
            prepared,
            image,
            dpi,
            grid_thumbnail,
            native_candidate,
            native_orientation,
            ocr,
            config,
        ),
    }?;
    processed.processing.quality = Some(assess_page_quality(
        &processed.page.text,
        &processed.processing.language,
        processed.processing.processing_mode,
        processed.processing.ocr_confidence,
        processed.processing.native_text_quality.as_ref(),
        processed.processing.native_text_layout.as_ref(),
    ));
    Ok(processed)
}

fn finish_prepared_native_page<O: OcrEngine>(
    mut prepared: PreparedPage,
    layer: NativeTextLayer,
    orientation: PageOrientation,
    grid_render: Option<PreparedGridRender>,
    ocr: &O,
    config: &WorkerConfig,
) -> Result<ProcessedPage, String> {
    let page_number = prepared.page_number;
    let Some(PreparedGridRender {
        rendered,
        native_words,
        grid_candidates,
        cell_ocr_regions,
    }) = grid_render
    else {
        prepared.timings.total_ms = elapsed_ms(prepared.page_started);
        return Ok(ProcessedPage {
            page_number,
            page: PageResult::new(page_number, PageKind::Text, layer.text, Vec::new()),
            processing: PageProcessingInfo {
                page_number,
                processing_mode: ProcessingMode::NativeText,
                native_text_quality: prepared.native_text_quality,
                native_text_layout: prepared.native_text_layout,
                ocr_confidence: None,
                orientation,
                language: config.languages.clone(),
                render_size: None,
                word_assignment: None,
                quality: None,
                timings: prepared.timings,
            },
            warnings: prepared.warnings,
        });
    };

    let (page_width, page_height) = rendered.image.dimensions();
    let render_size = RenderSize {
        width: page_width,
        height: page_height,
        dpi: rendered.dpi.round().clamp(0.0, f64::from(u32::MAX)) as u32,
    };
    if grid_candidates.is_empty() {
        let borderless = if config.table_mode == TableMode::GridWithFallback
            && !native_words.is_empty()
            && prepared
                .native_text_layout
                .as_ref()
                .is_none_or(NativeTextLayout::has_coordinate_clusters)
        {
            let stage_started = Instant::now();
            let tables = detect_borderless_tables_safely(&native_words, page_width, page_height)?;
            prepared.timings.grid_detection_ms += elapsed_ms(stage_started);
            tables
        } else {
            Vec::new()
        };
        if !borderless.is_empty() {
            let stage_started = Instant::now();
            let assignment = matrix_word_assignment_metrics(&native_words, &borderless);
            prepared.timings.assignment_ms += elapsed_ms(stage_started);
            prepared.timings.total_ms = elapsed_ms(prepared.page_started);
            return Ok(ProcessedPage {
                page_number,
                page: PageResult::new(page_number, PageKind::Mixed, layer.text, borderless),
                processing: PageProcessingInfo {
                    page_number,
                    processing_mode: ProcessingMode::NativeTextWithGrid,
                    native_text_quality: prepared.native_text_quality,
                    native_text_layout: prepared.native_text_layout,
                    ocr_confidence: None,
                    orientation,
                    language: config.languages.clone(),
                    render_size: Some(render_size),
                    word_assignment: Some(assignment),
                    quality: None,
                    timings: prepared.timings,
                },
                warnings: prepared.warnings,
            });
        }
        prepared.timings.total_ms = elapsed_ms(prepared.page_started);
        return Ok(ProcessedPage {
            page_number,
            page: PageResult::new(page_number, PageKind::Text, layer.text, Vec::new()),
            processing: PageProcessingInfo {
                page_number,
                processing_mode: ProcessingMode::NativeText,
                native_text_quality: prepared.native_text_quality,
                native_text_layout: prepared.native_text_layout,
                ocr_confidence: None,
                orientation,
                language: config.languages.clone(),
                render_size: None,
                word_assignment: None,
                quality: None,
                timings: prepared.timings,
            },
            warnings: prepared.warnings,
        });
    }

    let assignment = page_word_assignment_metrics(&native_words, &grid_candidates);
    let mut matrices = Vec::with_capacity(grid_candidates.len());
    let mut fallback_confidences = Vec::new();
    for (grid_index, mut grid) in grid_candidates.into_iter().enumerate() {
        let stage_started = Instant::now();
        let mut metrics = grid.assign_words(&native_words);
        prepared.timings.assignment_ms += elapsed_ms(stage_started);
        if config.enable_cell_ocr_fallback {
            let fallback =
                if let Some(region) = cell_ocr_regions.get(grid_index).and_then(Option::as_ref) {
                    fill_empty_grid_cells_with_ocr_from_region(
                        &mut grid,
                        &rendered,
                        region,
                        ocr,
                        config.max_pixels,
                        &mut prepared.timings,
                    )
                } else {
                    fill_empty_grid_cells_with_ocr_diagnostics(
                        &mut grid,
                        &rendered.image,
                        ocr,
                        config.max_pixels,
                        rendered.dpi.round().max(1.0) as u32,
                        &mut prepared.timings,
                    )
                };
            let fallback = fallback
                .map_err(|error| format!("page {page_number} cell OCR fallback failed: {error}"))?;
            fallback_confidences.extend(fallback.confidences);
            prepared.warnings.extend(
                fallback
                    .failures
                    .into_iter()
                    .map(|failure| failure.into_warning(page_number, grid_index)),
            );
            metrics.empty_cells = grid
                .cells
                .iter()
                .filter(|cell| cell.text.trim().is_empty())
                .count() as u32;
        }
        let stage_started = Instant::now();
        matrices.push(grid.to_matrix_with_type(classify_grid(
            &grid,
            &metrics,
            page_width,
            page_height,
        )));
        prepared.timings.assignment_ms += elapsed_ms(stage_started);
    }

    prepared.timings.total_ms = elapsed_ms(prepared.page_started);
    Ok(ProcessedPage {
        page_number,
        page: PageResult::new(page_number, PageKind::Mixed, layer.text, matrices),
        processing: PageProcessingInfo {
            page_number,
            processing_mode: ProcessingMode::NativeTextWithGrid,
            native_text_quality: prepared.native_text_quality,
            native_text_layout: prepared.native_text_layout,
            ocr_confidence: mean_confidence_values(&fallback_confidences),
            orientation,
            language: config.languages.clone(),
            render_size: Some(render_size),
            word_assignment: Some(assignment),
            quality: None,
            timings: prepared.timings,
        },
        warnings: prepared.warnings,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
fn finish_prepared_ocr_page<O: OcrEngine>(
    mut prepared: PreparedPage,
    image: GrayImage,
    dpi: f64,
    grid_thumbnail: Option<RenderedPage>,
    native_candidate: Option<NativeTextLayer>,
    native_orientation: Option<PageOrientation>,
    ocr: &O,
    config: &WorkerConfig,
) -> Result<ProcessedPage, String> {
    let page_number = prepared.page_number;
    // A near-usable native layer is deliberately retained for arbitration,
    // not accepted blindly. Grid evidence already enables the same guarded
    // comparison; this extends it to ordinary text pages without changing
    // the confidence or size thresholds below.
    let allow_non_structural_ocr_replacement =
        near_usable_native_requires_arbitration(prepared.native_text_quality.as_ref());

    let grid_thumbnail = grid_thumbnail.map(|rendered| rendered.image);
    let (orientation, image, grid_thumbnail) = if let Some(orientation) = native_orientation {
        // PDFium has already applied intrinsic PDF rotation to the bitmap.
        // Keep metadata for diagnostics without rotating the image twice.
        (orientation, image, grid_thumbnail)
    } else {
        let stage_started = Instant::now();
        let orientation = match detect_orientation(&image, ocr, dpi.round().max(1.0) as u32) {
            Ok(orientation) => orientation,
            Err(error) => {
                prepared
                    .warnings
                    .push(PipelineWarning::OrientationDetectionFailed {
                        page_number,
                        message: error.to_string(),
                    });
                PageOrientation::default()
            }
        };
        let detection_ms = elapsed_ms(stage_started);
        prepared.timings.orientation_ms += detection_ms;
        prepared.timings.tesseract_ms += detection_ms;
        let stage_started = Instant::now();
        let image = rotate_gray_image(image, orientation.angle)
            .map_err(|error| format!("failed to rotate page {page_number}: {error}"))?;
        let grid_thumbnail = grid_thumbnail
            .map(|thumbnail| rotate_gray_image(thumbnail, orientation.angle))
            .transpose()
            .map_err(|error| {
                format!("failed to rotate page {page_number} grid thumbnail: {error}")
            })?;
        prepared.timings.orientation_ms += elapsed_ms(stage_started);
        (orientation, image, grid_thumbnail)
    };

    let (page_width, page_height) = image.dimensions();
    let render_size = RenderSize {
        width: page_width,
        height: page_height,
        dpi: dpi.round().clamp(0.0, f64::from(u32::MAX)) as u32,
    };

    let mut grid_candidates = if matches!(
        config.table_mode,
        TableMode::Grid | TableMode::GridWithFallback
    ) {
        let stage_started = Instant::now();
        let detection = detect_table_grids_safely(&image);
        prepared.timings.grid_detection_ms += elapsed_ms(stage_started);
        match detection {
            Ok(grids) => grids,
            Err(message) => {
                prepared.warnings.push(PipelineWarning::GridDetectorFailed {
                    page_number,
                    message,
                });
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    if matches!(
        config.table_mode,
        TableMode::Grid | TableMode::GridWithFallback
    ) {
        if let Some(thumbnail) = grid_thumbnail.as_ref() {
            let stage_started = Instant::now();
            let detection = detect_table_grids_safely(thumbnail);
            prepared.timings.grid_detection_ms += elapsed_ms(stage_started);
            match detection {
                Ok(grids) => {
                    let (thumbnail_width, thumbnail_height) = thumbnail.dimensions();
                    let scaled = scale_table_grids(
                        grids,
                        thumbnail_width,
                        thumbnail_height,
                        page_width,
                        page_height,
                    );
                    merge_multiscale_table_grids(&mut grid_candidates, scaled);
                }
                Err(message) => {
                    prepared.warnings.push(PipelineWarning::GridDetectorFailed {
                        page_number,
                        message,
                    });
                }
            }
        }
    }

    let stage_started = Instant::now();
    let preprocessed = preprocess(image, config.max_pixels)
        .map_err(|error| format!("page {page_number} preprocessing failed: {error}"))?;
    prepared.timings.preprocessing_ms += elapsed_ms(stage_started);
    let ocr_request = OcrRequest::page(render_size.dpi.max(1));

    let (page, processing_mode, ocr_confidence, word_assignment) = if !grid_candidates.is_empty() {
        let stage_started = Instant::now();
        let (text, words) = ocr
            .recognize_text_and_words_with_request(&preprocessed, ocr_request)
            .map_err(|error| format!("page {page_number} OCR failed: {error}"))?;
        prepared.timings.tesseract_ms += elapsed_ms(stage_started);
        let word_confidence = mean_word_confidence(&words);
        let assignment = page_word_assignment_metrics(&words, &grid_candidates);
        let stage_started = Instant::now();
        let mut matrices = Vec::with_capacity(grid_candidates.len());
        for (grid_index, mut grid) in grid_candidates.into_iter().enumerate() {
            let mut metrics = grid.assign_words(&words);
            if config.enable_cell_ocr_fallback {
                let fallback = fill_empty_grid_cells_with_ocr_diagnostics(
                    &mut grid,
                    &preprocessed,
                    ocr,
                    config.max_pixels,
                    render_size.dpi.max(1),
                    &mut prepared.timings,
                )
                .map_err(|error| {
                    format!("page {page_number} raster cell OCR fallback failed: {error}")
                })?;
                prepared.warnings.extend(
                    fallback
                        .failures
                        .into_iter()
                        .map(|failure| failure.into_warning(page_number, grid_index)),
                );
                metrics.empty_cells = grid
                    .cells
                    .iter()
                    .filter(|cell| cell.text.trim().is_empty())
                    .count() as u32;
            }
            matrices.push(grid.to_matrix_with_type(classify_grid(
                &grid,
                &metrics,
                page_width,
                page_height,
            )));
        }
        prepared.timings.assignment_ms += elapsed_ms(stage_started);
        let confidence = text.confidence.or(word_confidence);
        let (page_text, used_native) =
            choose_canonical_page_text(native_candidate.as_ref(), &text.text, confidence, true);
        (
            PageResult::new(page_number, PageKind::Mixed, page_text, matrices),
            processing_mode_for_text_source(used_native, true),
            confidence,
            Some(assignment),
        )
    } else if matches!(config.table_mode, TableMode::Off | TableMode::Grid) {
        let stage_started = Instant::now();
        let text = ocr
            .recognize_text_with_request(&preprocessed, ocr_request)
            .map_err(|error| format!("page {page_number} text OCR failed: {error}"))?;
        prepared.timings.tesseract_ms += elapsed_ms(stage_started);
        let (page_text, used_native) = choose_canonical_page_text(
            native_candidate.as_ref(),
            &text.text,
            text.confidence,
            allow_non_structural_ocr_replacement,
        );
        (
            PageResult::new(page_number, PageKind::Text, page_text, Vec::new()),
            processing_mode_for_text_source(used_native, false),
            text.confidence,
            None,
        )
    } else if config.table_mode == TableMode::Legacy {
        let stage_started = Instant::now();
        let DetectionResult {
            is_table,
            confidence: _,
        } = detect_table(&preprocessed);
        prepared.timings.grid_detection_ms += elapsed_ms(stage_started);
        if is_table {
            let stage_started = Instant::now();
            let words = ocr
                .recognize_words_with_request(&preprocessed, ocr_request)
                .map_err(|error| format!("page {page_number} word OCR failed: {error}"))?;
            prepared.timings.tesseract_ms += elapsed_ms(stage_started);
            let confidence = mean_word_confidence(&words);
            let recognized_words = saturating_u32(words.len());
            let stage_started = Instant::now();
            let matrix = reconstruct_table(words, Y_TOLERANCE, X_TOLERANCE);
            prepared.timings.assignment_ms += elapsed_ms(stage_started);
            let table_text = matrix_to_text(&matrix);
            (
                PageResult::new(page_number, PageKind::Table, table_text, vec![matrix]),
                ProcessingMode::OcrWithGrid,
                confidence,
                Some(PageWordAssignmentMetrics {
                    recognized_words,
                    assigned_words: recognized_words,
                    words_outside_tables: 0,
                    duplicate_assignments: 0,
                }),
            )
        } else {
            let stage_started = Instant::now();
            let text = ocr
                .recognize_text_with_request(&preprocessed, ocr_request)
                .map_err(|error| format!("page {page_number} text OCR failed: {error}"))?;
            prepared.timings.tesseract_ms += elapsed_ms(stage_started);
            let (page_text, used_native) = choose_canonical_page_text(
                native_candidate.as_ref(),
                &text.text,
                text.confidence,
                allow_non_structural_ocr_replacement,
            );
            (
                PageResult::new(page_number, PageKind::Text, page_text, Vec::new()),
                processing_mode_for_text_source(used_native, false),
                text.confidence,
                None,
            )
        }
    } else {
        // GridWithFallback: one OCR call provides both the complete page text
        // and positioned words. A conservative borderless detector runs only
        // after ruled-grid detection returned no candidates.
        let stage_started = Instant::now();
        let (recognized, words) = ocr
            .recognize_text_and_words_with_request(&preprocessed, ocr_request)
            .map_err(|error| format!("page {page_number} OCR failed: {error}"))?;
        prepared.timings.tesseract_ms += elapsed_ms(stage_started);
        let word_confidence = mean_word_confidence(&words);
        let stage_started = Instant::now();
        let borderless = detect_borderless_tables_safely(&words, page_width, page_height)?;
        prepared.timings.grid_detection_ms += elapsed_ms(stage_started);

        if !borderless.is_empty() {
            let stage_started = Instant::now();
            let assignment = matrix_word_assignment_metrics(&words, &borderless);
            prepared.timings.assignment_ms += elapsed_ms(stage_started);
            let confidence = recognized.confidence.or(word_confidence);
            let (page_text, used_native) = choose_canonical_page_text(
                native_candidate.as_ref(),
                &recognized.text,
                confidence,
                true,
            );
            (
                PageResult::new(page_number, PageKind::Mixed, page_text, borderless),
                processing_mode_for_text_source(used_native, true),
                confidence,
                Some(assignment),
            )
        } else {
            // Keep the old raster/word-alignment route as the last
            // compatibility fallback. It is not treated as a confident
            // borderless result and remains `kind=unknown` in the stable API.
            let stage_started = Instant::now();
            let DetectionResult {
                is_table,
                confidence: _,
            } = detect_table(&preprocessed);
            prepared.timings.grid_detection_ms += elapsed_ms(stage_started);
            if is_table {
                let recognized_words = saturating_u32(words.len());
                let stage_started = Instant::now();
                let matrix = reconstruct_table(words, Y_TOLERANCE, X_TOLERANCE);
                prepared.timings.assignment_ms += elapsed_ms(stage_started);
                let confidence = recognized.confidence.or(word_confidence);
                let (page_text, used_native) = choose_canonical_page_text(
                    native_candidate.as_ref(),
                    &recognized.text,
                    confidence,
                    true,
                );
                (
                    PageResult::new(page_number, PageKind::Mixed, page_text, vec![matrix]),
                    processing_mode_for_text_source(used_native, true),
                    confidence,
                    Some(PageWordAssignmentMetrics {
                        recognized_words,
                        assigned_words: recognized_words,
                        words_outside_tables: 0,
                        duplicate_assignments: 0,
                    }),
                )
            } else {
                let confidence = recognized.confidence.or(word_confidence);
                let (page_text, used_native) = choose_canonical_page_text(
                    native_candidate.as_ref(),
                    &recognized.text,
                    confidence,
                    allow_non_structural_ocr_replacement,
                );
                (
                    PageResult::new(page_number, PageKind::Text, page_text, Vec::new()),
                    processing_mode_for_text_source(used_native, false),
                    confidence,
                    None,
                )
            }
        }
    };

    prepared.timings.total_ms = elapsed_ms(prepared.page_started);
    Ok(ProcessedPage {
        page_number,
        page,
        processing: PageProcessingInfo {
            page_number,
            processing_mode,
            native_text_quality: prepared.native_text_quality,
            native_text_layout: prepared.native_text_layout,
            ocr_confidence,
            orientation,
            language: config.languages.clone(),
            render_size: Some(render_size),
            word_assignment,
            quality: None,
            timings: prepared.timings,
        },
        warnings: prepared.warnings,
    })
}

fn choose_canonical_page_text(
    native: Option<&NativeTextLayer>,
    ocr_text: &str,
    ocr_confidence: Option<f32>,
    allow_ocr_replacement: bool,
) -> (String, bool) {
    let Some(native) = native else {
        return (ocr_text.to_string(), false);
    };
    let (native_non_whitespace, native_alphanumeric) = text_signal_counts(&native.text);
    let (ocr_non_whitespace, ocr_alphanumeric) = text_signal_counts(ocr_text);
    let ocr_is_materially_better = allow_ocr_replacement
        && ocr_confidence.unwrap_or(0.0) >= 0.65
        && ocr_non_whitespace >= 48
        && ocr_non_whitespace >= native_non_whitespace.saturating_mul(2)
        && ocr_alphanumeric >= native_alphanumeric.saturating_mul(2);
    if ocr_is_materially_better {
        (ocr_text.to_string(), false)
    } else {
        (native.text.clone(), true)
    }
}

fn near_usable_native_requires_arbitration(native_quality: Option<&NativeTextQuality>) -> bool {
    native_quality.is_some_and(|quality| quality.is_near_usable)
}

fn scale_table_grids(
    grids: Vec<TableGrid>,
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
) -> Vec<TableGrid> {
    if source_width == 0 || source_height == 0 || target_width == 0 || target_height == 0 {
        return Vec::new();
    }
    let scale_x = f64::from(target_width) / f64::from(source_width);
    let scale_y = f64::from(target_height) / f64::from(source_height);
    grids
        .into_iter()
        .map(|mut grid| {
            scale_bbox(&mut grid.bbox, scale_x, scale_y);
            for line in &mut grid.horizontal_lines {
                line.position *= scale_y;
                line.start *= scale_x;
                line.end *= scale_x;
            }
            for line in &mut grid.vertical_lines {
                line.position *= scale_x;
                line.start *= scale_y;
                line.end *= scale_y;
            }
            for intersection in &mut grid.intersections {
                intersection.x *= scale_x;
                intersection.y *= scale_y;
            }
            for cell in &mut grid.cells {
                if let Some(bbox) = cell.bbox.as_mut() {
                    scale_bbox(bbox, scale_x, scale_y);
                }
            }
            grid
        })
        .collect()
}

fn scale_bbox(bbox: &mut BoundingBox, scale_x: f64, scale_y: f64) {
    bbox.x0 *= scale_x;
    bbox.x1 *= scale_x;
    bbox.y0 *= scale_y;
    bbox.y1 *= scale_y;
}

fn render_bounded_grid_thumbnail<S: PageSource>(
    source: &mut S,
    page_number: u32,
    full_render: &RenderedPage,
    target_dpi: u32,
    max_pixels: usize,
) -> Result<RenderedPage, Box<dyn std::error::Error>> {
    if target_dpi == 0 {
        return Err("grid thumbnail DPI must be greater than zero".into());
    }
    if !full_render.dpi.is_finite() || full_render.dpi <= 0.0 {
        return Err(format!("invalid full-render DPI {}", full_render.dpi).into());
    }

    let scale = f64::from(target_dpi) / full_render.dpi;
    let desired_width = (f64::from(full_render.image.width()) * scale)
        .round()
        .clamp(1.0, f64::from(u32::MAX)) as u32;
    let desired_height = (f64::from(full_render.image.height()) * scale)
        .round()
        .clamp(1.0, f64::from(u32::MAX)) as u32;
    let (bounded_width, bounded_height) =
        dimensions_within_pixel_limit(desired_width, desired_height, max_pixels)?;
    let max_dimension = bounded_width.max(bounded_height);

    let thumbnail = source.render_table_thumbnail(page_number, max_dimension)?;
    bound_rendered_page_to_pixels(thumbnail, max_pixels)
}

fn bound_rendered_page_to_pixels(
    rendered: RenderedPage,
    max_pixels: usize,
) -> Result<RenderedPage, Box<dyn std::error::Error>> {
    let (target_width, target_height) =
        dimensions_within_pixel_limit(rendered.image.width(), rendered.image.height(), max_pixels)?;
    if (target_width, target_height) == rendered.image.dimensions() {
        return Ok(rendered);
    }

    let scale = f64::from(target_width) / f64::from(rendered.image.width());
    Ok(RenderedPage::new(
        rendered.page_num,
        imageops::resize(
            &rendered.image,
            target_width,
            target_height,
            imageops::FilterType::Triangle,
        ),
        rendered.dpi * scale,
    ))
}

fn merge_multiscale_table_grids(primary: &mut Vec<TableGrid>, additional: Vec<TableGrid>) {
    const DUPLICATE_IOU: f64 = 0.70;
    for candidate in additional {
        if let Some(index) = primary
            .iter()
            .position(|existing| bbox_iou(&existing.bbox, &candidate.bbox) >= DUPLICATE_IOU)
        {
            let topology_matches = grid_topology(&candidate) == grid_topology(&primary[index]);
            let candidate_is_preferred = if topology_matches {
                candidate.confidence > primary[index].confidence
            } else {
                grid_structural_richness(&candidate) > grid_structural_richness(&primary[index])
            };
            if candidate_is_preferred {
                primary[index] = candidate;
            }
        } else {
            primary.push(candidate);
        }
    }
    primary.sort_by(|left, right| {
        left.bbox
            .y0
            .total_cmp(&right.bbox.y0)
            .then_with(|| left.bbox.x0.total_cmp(&right.bbox.x0))
    });
}

fn grid_topology(grid: &TableGrid) -> (u32, u32) {
    (grid.rows(), grid.columns())
}

fn grid_structural_richness(grid: &TableGrid) -> (usize, usize, usize) {
    (
        grid.cells.len(),
        grid.intersections.len(),
        grid.horizontal_lines.len() + grid.vertical_lines.len(),
    )
}

fn bbox_iou(left: &BoundingBox, right: &BoundingBox) -> f64 {
    let intersection_width = (left.x1.min(right.x1) - left.x0.max(right.x0)).max(0.0);
    let intersection_height = (left.y1.min(right.y1) - left.y0.max(right.y0)).max(0.0);
    let intersection = intersection_width * intersection_height;
    let left_area = (left.x1 - left.x0).max(0.0) * (left.y1 - left.y0).max(0.0);
    let right_area = (right.x1 - right.x0).max(0.0) * (right.y1 - right.y0).max(0.0);
    let union = left_area + right_area - intersection;
    if union > 0.0 {
        intersection / union
    } else {
        0.0
    }
}

fn text_signal_counts(text: &str) -> (usize, usize) {
    let non_whitespace = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .count();
    let alphanumeric = text
        .chars()
        .filter(|character| character.is_alphanumeric())
        .count();
    (non_whitespace, alphanumeric)
}

fn processing_mode_for_text_source(used_native: bool, has_grid: bool) -> ProcessingMode {
    match (used_native, has_grid) {
        (true, true) => ProcessingMode::NativeTextWithGrid,
        (true, false) => ProcessingMode::NativeText,
        (false, true) => ProcessingMode::OcrWithGrid,
        (false, false) => ProcessingMode::Ocr,
    }
}

#[cfg(test)]
#[expect(
    clippy::too_many_arguments,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
fn process_one<S, O, F>(
    source: &mut S,
    ocr: &O,
    max_pixels: usize,
    page_num: u32,
    table_mode: TableMode,
    languages: &str,
    grid_dpi: u32,
    enable_cell_ocr_fallback: bool,
    progress: Option<&dyn OcrProgressReporter>,
    total_pages: u32,
    grid_detector: F,
) -> Result<(PageResult, PageProcessingInfo, Vec<PipelineWarning>), Box<dyn std::error::Error>>
where
    S: PageSource,
    O: OcrEngine,
    F: Fn(&GrayImage) -> Result<Vec<TableGrid>, String>,
{
    ocr.begin_page()?;
    let page_started = Instant::now();
    let mut timings = PageStageTimings::default();
    let mut warnings = Vec::new();
    let stage_started = Instant::now();
    let native_text_layer = match source.native_text_for_page(page_num) {
        Ok(layer) => layer,
        Err(error) => {
            warnings.push(PipelineWarning::NativeTextUnavailable {
                page_number: page_num,
                message: error.to_string(),
            });
            None
        }
    };
    timings.native_text_extraction_ms = elapsed_ms(stage_started);
    let native_text_quality = native_text_layer
        .as_ref()
        .map(|layer| analyze_native_text_for_languages(layer, languages));
    let native_text_usable = native_text_quality
        .as_ref()
        .is_some_and(|quality| quality.is_usable);
    let native_orientation = if native_text_usable {
        source.page_orientation_from_metadata(page_num)?
    } else {
        PageOrientation::default()
    };

    // PDFium text rectangles provide supporting layout evidence (block count
    // and recurring X/Y alignment). They are intentionally not enough by
    // themselves to force OCR: the ruled detector remains authoritative, and
    // the borderless stage must still reconstruct a high-confidence layout
    // from positioned native words.
    let native_text_layout = if native_text_usable {
        let stage_started = Instant::now();
        let layout = match source.native_text_layout_for_page(page_num) {
            Ok(layout) => layout,
            Err(error) => {
                warnings.push(PipelineWarning::NativeTextLayoutUnavailable {
                    page_number: page_num,
                    message: error.to_string(),
                });
                None
            }
        };
        timings.native_text_layout_ms = elapsed_ms(stage_started);
        layout
    } else {
        None
    };

    // Searchable pages use one low-DPI render for both routing and final grid
    // geometry. Keeping the rendered image and candidates avoids the previous
    // full render + second grid-detection pass.
    let native_grid_render = if native_text_usable
        && matches!(table_mode, TableMode::Grid | TableMode::GridWithFallback)
    {
        report_stage_started(progress, OcrProgressStage::Render, page_num, total_pages);
        let stage_started = Instant::now();
        let thumbnail = source.render_page_at_dpi_with_max_pixels(page_num, grid_dpi, max_pixels);
        timings.thumbnail_render_ms += elapsed_ms(stage_started);
        match thumbnail {
            Ok(thumbnail) => {
                report_stage_started(
                    progress,
                    OcrProgressStage::TableDetection,
                    page_num,
                    total_pages,
                );
                let stage_started = Instant::now();
                let detection = grid_detector(&thumbnail.image);
                timings.grid_detection_ms += elapsed_ms(stage_started);
                match detection {
                    Ok(grids) if !grids.is_empty() || table_mode == TableMode::GridWithFallback => {
                        Some((thumbnail, grids))
                    }
                    Ok(_) => None,
                    Err(message) => {
                        warnings.push(PipelineWarning::GridDetectorFailed {
                            page_number: page_num,
                            message,
                        });
                        None
                    }
                }
            }
            Err(error) => {
                warnings.push(PipelineWarning::TableRoutingPreflightFailed {
                    page_number: page_num,
                    message: error.to_string(),
                });
                None
            }
        }
    } else {
        None
    };

    if native_text_usable {
        let layer = native_text_layer
            .as_ref()
            .expect("usable native text layer must exist");
        if let Some((rendered, grid_candidates)) = native_grid_render {
            let (page_width, page_height) = rendered.image.dimensions();
            let render_size = RenderSize {
                width: page_width,
                height: page_height,
                dpi: rendered.dpi.round().clamp(0.0, f64::from(u32::MAX)) as u32,
            };
            let stage_started = Instant::now();
            let native_words =
                match source.native_words_for_rendered_page(page_num, page_width, page_height) {
                    Ok(Some(words)) => words,
                    Ok(None) => {
                        warnings.push(PipelineWarning::NativeWordGeometryUnavailable {
                            page_number: page_num,
                            message: "page source does not expose coordinate-bearing native words"
                                .to_string(),
                        });
                        Vec::new()
                    }
                    Err(error) => {
                        warnings.push(PipelineWarning::NativeWordGeometryUnavailable {
                            page_number: page_num,
                            message: error.to_string(),
                        });
                        Vec::new()
                    }
                };
            timings.native_text_extraction_ms += elapsed_ms(stage_started);

            if grid_candidates.is_empty() {
                let borderless = if table_mode == TableMode::GridWithFallback
                    && !native_words.is_empty()
                    && native_text_layout
                        .as_ref()
                        .is_none_or(NativeTextLayout::has_coordinate_clusters)
                {
                    let stage_started = Instant::now();
                    let tables =
                        detect_borderless_tables_safely(&native_words, page_width, page_height)
                            .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
                    timings.grid_detection_ms += elapsed_ms(stage_started);
                    tables
                } else {
                    Vec::new()
                };
                if borderless.is_empty() {
                    let page =
                        PageResult::new(page_num, PageKind::Text, layer.text.clone(), Vec::new());
                    timings.total_ms = elapsed_ms(page_started);
                    let processing = PageProcessingInfo {
                        page_number: page_num,
                        processing_mode: ProcessingMode::NativeText,
                        native_text_quality,
                        native_text_layout,
                        ocr_confidence: None,
                        orientation: native_orientation,
                        language: languages.to_string(),
                        render_size: None,
                        word_assignment: None,
                        quality: None,
                        timings,
                    };
                    return Ok((page, processing, warnings));
                }
                let stage_started = Instant::now();
                let assignment = matrix_word_assignment_metrics(&native_words, &borderless);
                timings.assignment_ms += elapsed_ms(stage_started);
                timings.total_ms = elapsed_ms(page_started);
                let page =
                    PageResult::new(page_num, PageKind::Mixed, layer.text.clone(), borderless);
                let processing = PageProcessingInfo {
                    page_number: page_num,
                    processing_mode: ProcessingMode::NativeTextWithGrid,
                    native_text_quality,
                    native_text_layout,
                    ocr_confidence: None,
                    orientation: native_orientation,
                    language: languages.to_string(),
                    render_size: Some(render_size),
                    word_assignment: Some(assignment),
                    quality: None,
                    timings,
                };
                return Ok((page, processing, warnings));
            }

            let assignment = page_word_assignment_metrics(&native_words, &grid_candidates);
            let mut matrices = Vec::with_capacity(grid_candidates.len());
            let mut fallback_confidences = Vec::new();
            for mut grid in grid_candidates {
                let stage_started = Instant::now();
                let mut metrics = grid.assign_words(&native_words);
                timings.assignment_ms += elapsed_ms(stage_started);
                if enable_cell_ocr_fallback {
                    report_stage_started(progress, OcrProgressStage::Ocr, page_num, total_pages);
                    fallback_confidences.extend(fill_empty_grid_cells_with_ocr(
                        &mut grid,
                        &rendered.image,
                        ocr,
                        max_pixels,
                        rendered.dpi.round().max(1.0) as u32,
                        &mut timings,
                    )?);
                    metrics.empty_cells = grid
                        .cells
                        .iter()
                        .filter(|cell| cell.text.trim().is_empty())
                        .count() as u32;
                }
                let stage_started = Instant::now();
                matrices.push(grid.to_matrix_with_type(classify_grid(
                    &grid,
                    &metrics,
                    page_width,
                    page_height,
                )));
                timings.assignment_ms += elapsed_ms(stage_started);
            }
            timings.total_ms = elapsed_ms(page_started);
            let page = PageResult::new(page_num, PageKind::Mixed, layer.text.clone(), matrices);
            let processing = PageProcessingInfo {
                page_number: page_num,
                processing_mode: ProcessingMode::NativeTextWithGrid,
                native_text_quality,
                native_text_layout,
                ocr_confidence: mean_confidence_values(&fallback_confidences),
                orientation: native_orientation,
                language: languages.to_string(),
                render_size: Some(render_size),
                word_assignment: Some(assignment),
                quality: None,
                timings,
            };
            return Ok((page, processing, warnings));
        } else {
            let page = PageResult::new(page_num, PageKind::Text, layer.text.clone(), Vec::new());
            timings.total_ms = elapsed_ms(page_started);
            let processing = PageProcessingInfo {
                page_number: page_num,
                processing_mode: ProcessingMode::NativeText,
                native_text_quality,
                native_text_layout,
                ocr_confidence: None,
                // Native text is not raster OCR. PDFium has already handled
                // intrinsic PDF rotation during text extraction, so no image
                // rotation is applied or claimed here.
                orientation: native_orientation,
                language: languages.to_string(),
                render_size: None,
                word_assignment: None,
                quality: None,
                timings,
            };
            return Ok((page, processing, warnings));
        }
    }

    // OCR-only pages retain the existing full-resolution path.
    report_stage_started(progress, OcrProgressStage::Render, page_num, total_pages);
    let stage_started = Instant::now();
    let rendered = source.render_page_with_max_pixels(page_num, max_pixels)?;
    timings.full_render_ms += elapsed_ms(stage_started);
    let (dpi, page_num) = (rendered.dpi, rendered.page_num);
    let raw = rendered.image;

    // 2. Orientation preflight runs on a bounded thumbnail and applies the
    // chosen right-angle rotation to the full bitmap *before* grid detection,
    // preprocessing, and final OCR. A failure is non-fatal and keeps 0°.
    report_stage_started(progress, OcrProgressStage::Ocr, page_num, total_pages);
    let stage_started = Instant::now();
    let orientation = match detect_orientation(&raw, ocr, dpi.round().max(1.0) as u32) {
        Ok(orientation) => orientation,
        Err(error) => {
            warnings.push(PipelineWarning::OrientationDetectionFailed {
                page_number: page_num,
                message: error.to_string(),
            });
            PageOrientation::default()
        }
    };
    let detection_ms = elapsed_ms(stage_started);
    timings.orientation_ms += detection_ms;
    timings.tesseract_ms += detection_ms;
    let stage_started = Instant::now();
    let raw = rotate_gray_image(raw, orientation.angle)
        .map_err(|error| format!("failed to rotate page {page_num}: {error}"))?;
    timings.orientation_ms += elapsed_ms(stage_started);
    let (page_width, page_height) = raw.dimensions();
    let render_size = RenderSize {
        width: page_width,
        height: page_height,
        dpi: dpi.round().clamp(0.0, f64::from(u32::MAX)) as u32,
    };

    // 3. Detect ruled grids from the oriented grayscale page. The detection
    // owns its debug masks internally, but production retains only candidate
    // table geometry before the original bitmap is consumed by OCR
    // preprocessing.
    let grid_candidates = if matches!(table_mode, TableMode::Grid | TableMode::GridWithFallback) {
        report_stage_started(
            progress,
            OcrProgressStage::TableDetection,
            page_num,
            total_pages,
        );
        let stage_started = Instant::now();
        let detection = grid_detector(&raw);
        timings.grid_detection_ms += elapsed_ms(stage_started);
        match detection {
            Ok(grids) => grids,
            Err(message) => {
                warnings.push(PipelineWarning::GridDetectorFailed {
                    page_number: page_num,
                    message,
                });
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    // 4. Preprocess for OCR.
    let stage_started = Instant::now();
    let preprocessed = preprocess(raw, max_pixels)?;
    timings.preprocessing_ms += elapsed_ms(stage_started);
    let page_processing = |processing_mode: ProcessingMode,
                           ocr_confidence: Option<f32>,
                           word_assignment: Option<PageWordAssignmentMetrics>,
                           mut timings: PageStageTimings| {
        timings.total_ms = elapsed_ms(page_started);
        PageProcessingInfo {
            page_number: page_num,
            processing_mode,
            native_text_quality: native_text_quality.clone(),
            native_text_layout: native_text_layout.clone(),
            ocr_confidence,
            orientation,
            language: languages.to_string(),
            render_size: Some(render_size),
            word_assignment,
            quality: None,
            timings,
        }
    };

    // 5a. Ruled-table branch. Each TableGrid has its own bounding box; no
    // page-wide TableMatrix is created here.
    if !grid_candidates.is_empty() {
        report_stage_started(progress, OcrProgressStage::Ocr, page_num, total_pages);
        let stage_started = Instant::now();
        let (recognized_text, words) = ocr.recognize_text_and_words(&preprocessed)?;
        timings.tesseract_ms += elapsed_ms(stage_started);
        let word_confidence = mean_word_confidence(&words);
        let stage_started = Instant::now();
        let assignment = page_word_assignment_metrics(&words, &grid_candidates);
        let mut matrices = Vec::with_capacity(grid_candidates.len());
        for mut grid in grid_candidates {
            let metrics = grid.assign_words(&words);
            matrices.push(grid.to_matrix_with_type(classify_grid(
                &grid,
                &metrics,
                page_width,
                page_height,
            )));
        }
        timings.assignment_ms += elapsed_ms(stage_started);
        let (page_text, processing_mode, ocr_confidence) = if native_text_usable {
            // Table reconstruction enriches a searchable page. It must never
            // replace the complete native layer with the subset found in cells.
            (
                native_text_layer
                    .as_ref()
                    .expect("usable native text layer must exist")
                    .text
                    .clone(),
                ProcessingMode::NativeTextWithGrid,
                word_confidence,
            )
        } else {
            (
                recognized_text.text,
                ProcessingMode::OcrWithGrid,
                recognized_text.confidence.or(word_confidence),
            )
        };
        let page = PageResult::new(page_num, PageKind::Mixed, page_text, matrices);
        drop(preprocessed);
        return Ok((
            page,
            page_processing(processing_mode, ocr_confidence, Some(assignment), timings),
            warnings,
        ));
    }

    if matches!(table_mode, TableMode::Off | TableMode::Grid) {
        let stage_started = Instant::now();
        let text = ocr.recognize_text_with_confidence(&preprocessed)?;
        timings.tesseract_ms += elapsed_ms(stage_started);
        let page = PageResult::new(page_num, PageKind::Text, text.text, Vec::new());
        drop(preprocessed);
        return Ok((
            page,
            page_processing(ProcessingMode::Ocr, text.confidence, None, timings),
            warnings,
        ));
    }

    if table_mode == TableMode::GridWithFallback {
        let stage_started = Instant::now();
        let (recognized, words) = ocr.recognize_text_and_words(&preprocessed)?;
        timings.tesseract_ms += elapsed_ms(stage_started);
        let word_confidence = mean_word_confidence(&words);
        let stage_started = Instant::now();
        let borderless = detect_borderless_tables_safely(&words, page_width, page_height)
            .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
        timings.grid_detection_ms += elapsed_ms(stage_started);
        if !borderless.is_empty() {
            let stage_started = Instant::now();
            let assignment = matrix_word_assignment_metrics(&words, &borderless);
            timings.assignment_ms += elapsed_ms(stage_started);
            let page = PageResult::new(page_num, PageKind::Mixed, recognized.text, borderless);
            drop(preprocessed);
            return Ok((
                page,
                page_processing(
                    ProcessingMode::OcrWithGrid,
                    recognized.confidence.or(word_confidence),
                    Some(assignment),
                    timings,
                ),
                warnings,
            ));
        }

        // The original line-based word-alignment route remains the last
        // compatibility fallback. Keep the complete OCR text as the primary
        // layer even when that legacy matrix is attached.
        let stage_started = Instant::now();
        let DetectionResult {
            is_table,
            confidence: _,
        } = detect_table(&preprocessed);
        timings.grid_detection_ms += elapsed_ms(stage_started);
        if is_table {
            let recognized_words = saturating_u32(words.len());
            let stage_started = Instant::now();
            let matrix = reconstruct_table(words, Y_TOLERANCE, X_TOLERANCE);
            timings.assignment_ms += elapsed_ms(stage_started);
            let page = PageResult::new(page_num, PageKind::Mixed, recognized.text, vec![matrix]);
            drop(preprocessed);
            return Ok((
                page,
                page_processing(
                    ProcessingMode::OcrWithGrid,
                    recognized.confidence.or(word_confidence),
                    Some(PageWordAssignmentMetrics {
                        recognized_words,
                        assigned_words: recognized_words,
                        words_outside_tables: 0,
                        duplicate_assignments: 0,
                    }),
                    timings,
                ),
                warnings,
            ));
        }
        let page = PageResult::new(page_num, PageKind::Text, recognized.text, Vec::new());
        drop(preprocessed);
        return Ok((
            page,
            page_processing(
                ProcessingMode::Ocr,
                recognized.confidence.or(word_confidence),
                None,
                timings,
            ),
            warnings,
        ));
    }

    // 4b. Legacy page-level detector. It remains available as an explicit
    // word-alignment fallback for pages without stable ruled-grid geometry.
    let stage_started = Instant::now();
    let DetectionResult {
        is_table,
        confidence: _,
    } = detect_table(&preprocessed);
    timings.grid_detection_ms += elapsed_ms(stage_started);

    // 4c. Word-alignment fallback.
    if is_table {
        let stage_started = Instant::now();
        let words = ocr.recognize_words(&preprocessed)?;
        timings.tesseract_ms += elapsed_ms(stage_started);
        let confidence = mean_word_confidence(&words);
        let recognized_words = saturating_u32(words.len());
        let stage_started = Instant::now();
        let matrix = reconstruct_table(words, Y_TOLERANCE, X_TOLERANCE);
        timings.assignment_ms += elapsed_ms(stage_started);
        let table_text = matrix_to_text(&matrix);
        let page = PageResult::new(page_num, PageKind::Table, table_text, vec![matrix]);
        // Drop the bitmap explicitly before returning to keep peak
        // memory per iteration predictable.
        drop(preprocessed);
        return Ok((
            page,
            page_processing(
                ProcessingMode::OcrWithGrid,
                confidence,
                Some(PageWordAssignmentMetrics {
                    recognized_words,
                    assigned_words: recognized_words,
                    words_outside_tables: 0,
                    duplicate_assignments: 0,
                }),
                timings,
            ),
            warnings,
        ));
    }

    // 4d. Text branch.
    let stage_started = Instant::now();
    let text = ocr.recognize_text_with_confidence(&preprocessed)?;
    timings.tesseract_ms += elapsed_ms(stage_started);
    let page = PageResult::new(page_num, PageKind::Text, text.text, Vec::new());
    drop(preprocessed);
    Ok((
        page,
        page_processing(ProcessingMode::Ocr, text.confidence, None, timings),
        warnings,
    ))
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000.0
}

#[expect(
    clippy::too_many_arguments,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
fn prepare_grid_cell_ocr_regions<S: PageSource>(
    source: &mut S,
    page_number: u32,
    rendered: &RenderedPage,
    native_words: &[crate::dto::Word],
    grid_candidates: &[TableGrid],
    cell_ocr_dpi: u32,
    max_pixels: usize,
    timings: &mut PageStageTimings,
    warnings: &mut Vec<PipelineWarning>,
) -> Vec<Option<RenderedPageRegion>> {
    let (page_width, page_height) = rendered.image.dimensions();
    let mut regions = Vec::with_capacity(grid_candidates.len());
    let mut allocated_roi_pixels = 0usize;

    for (grid_index, grid) in grid_candidates.iter().enumerate() {
        let mut assigned = grid.clone();
        let metrics = assigned.assign_words(native_words);
        let has_unresolved_cells = assigned.cells.iter().any(|cell| {
            cell.text.trim().is_empty()
                && cell.bbox.as_ref().is_some_and(|bbox| {
                    prepare_cell_ocr_crop(
                        &rendered.image,
                        bbox,
                        max_pixels,
                        rendered.dpi.round().max(1.0) as u32,
                    )
                    .is_ok_and(|crop| crop.is_some())
                })
        });
        if !has_unresolved_cells
            || classify_grid(&assigned, &metrics, page_width, page_height) == TableType::TitleBlock
        {
            regions.push(None);
            continue;
        }

        let Some((x, y, width, height)) =
            normalized_padded_grid_region(&grid.bbox, page_width, page_height)
        else {
            regions.push(None);
            continue;
        };
        let Some(estimated_pixels) = estimated_region_pixels(rendered, cell_ocr_dpi, width, height)
        else {
            warnings.push(PipelineWarning::TableRoutingPreflightFailed {
                page_number,
                message: format!("cell OCR ROI size overflow for grid {}", grid_index + 1),
            });
            regions.push(None);
            continue;
        };
        if allocated_roi_pixels
            .checked_add(estimated_pixels)
            .is_none_or(|total| total > max_pixels)
        {
            warnings.push(PipelineWarning::TableRoutingPreflightFailed {
                page_number,
                message: format!(
                    "cell OCR ROI budget exhausted before grid {}",
                    grid_index + 1
                ),
            });
            regions.push(None);
            continue;
        }
        let stage_started = Instant::now();
        let remaining_pixels = max_pixels.saturating_sub(allocated_roi_pixels);
        let region = source.render_region_at_dpi(
            page_number,
            cell_ocr_dpi,
            x,
            y,
            width,
            height,
            remaining_pixels,
        );
        timings.full_render_ms += elapsed_ms(stage_started);
        match region {
            Ok(Some(region)) => {
                let pixels =
                    (region.image.width() as usize).checked_mul(region.image.height() as usize);
                if pixels.is_none_or(|pixels| {
                    allocated_roi_pixels
                        .checked_add(pixels)
                        .is_none_or(|total| total > max_pixels)
                }) {
                    warnings.push(PipelineWarning::TableRoutingPreflightFailed {
                        page_number,
                        message: format!(
                            "cell OCR ROI for grid {} exceeds the page pixel limit",
                            grid_index + 1
                        ),
                    });
                    regions.push(None);
                } else {
                    allocated_roi_pixels += pixels.expect("validated ROI pixel count");
                    regions.push(Some(region));
                }
            }
            Ok(None) => regions.push(None),
            Err(error) => {
                warnings.push(PipelineWarning::TableRoutingPreflightFailed {
                    page_number,
                    message: format!(
                        "high-detail cell OCR ROI unavailable for grid {}: {error}",
                        grid_index + 1
                    ),
                });
                regions.push(None);
            }
        }
    }
    regions
}

fn estimated_region_pixels(
    rendered: &RenderedPage,
    target_dpi: u32,
    normalized_width: f32,
    normalized_height: f32,
) -> Option<usize> {
    if rendered.dpi <= 0.0
        || !rendered.dpi.is_finite()
        || target_dpi == 0
        || normalized_width <= 0.0
        || normalized_height <= 0.0
    {
        return None;
    }
    let scale = f64::from(target_dpi) / rendered.dpi;
    let width = (f64::from(rendered.image.width()) * scale * f64::from(normalized_width)).ceil();
    let height = (f64::from(rendered.image.height()) * scale * f64::from(normalized_height)).ceil();
    if !width.is_finite() || !height.is_finite() || width < 1.0 || height < 1.0 {
        return None;
    }
    let width = usize::try_from(width as u64).ok()?;
    let height = usize::try_from(height as u64).ok()?;
    width.checked_add(2)?.checked_mul(height.checked_add(2)?)
}

fn normalized_padded_grid_region(
    bbox: &BoundingBox,
    page_width: u32,
    page_height: u32,
) -> Option<(f32, f32, f32, f32)> {
    if page_width == 0 || page_height == 0 {
        return None;
    }
    let padding_x = ((bbox.x1 - bbox.x0).abs() * 0.01).clamp(2.0, 12.0);
    let padding_y = ((bbox.y1 - bbox.y0).abs() * 0.01).clamp(2.0, 12.0);
    let x0 = (bbox.x0 - padding_x)
        .floor()
        .clamp(0.0, f64::from(page_width));
    let y0 = (bbox.y0 - padding_y)
        .floor()
        .clamp(0.0, f64::from(page_height));
    let x1 = (bbox.x1 + padding_x)
        .ceil()
        .clamp(0.0, f64::from(page_width));
    let y1 = (bbox.y1 + padding_y)
        .ceil()
        .clamp(0.0, f64::from(page_height));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some((
        (x0 / f64::from(page_width)) as f32,
        (y0 / f64::from(page_height)) as f32,
        ((x1 - x0) / f64::from(page_width)) as f32,
        ((y1 - y0) / f64::from(page_height)) as f32,
    ))
}

struct CellOcrRaster<'a> {
    image: &'a GrayImage,
    dpi: u32,
    source_width: u32,
    source_height: u32,
    full_width: u32,
    full_height: u32,
    offset_x: u32,
    offset_y: u32,
}

impl<'a> CellOcrRaster<'a> {
    fn full_page(image: &'a GrayImage, dpi: u32) -> Self {
        Self {
            image,
            dpi,
            source_width: image.width(),
            source_height: image.height(),
            full_width: image.width(),
            full_height: image.height(),
            offset_x: 0,
            offset_y: 0,
        }
    }

    fn from_region(source_page: &RenderedPage, region: &'a RenderedPageRegion) -> Option<Self> {
        if source_page.image.width() == 0
            || source_page.image.height() == 0
            || region.dpi == 0
            || region.image.width() == 0
            || region.image.height() == 0
            || region.full_width == 0
            || region.full_height == 0
            || region
                .x
                .checked_add(region.image.width())
                .is_none_or(|right| right > region.full_width)
            || region
                .y
                .checked_add(region.image.height())
                .is_none_or(|bottom| bottom > region.full_height)
        {
            return None;
        }
        let expected_scale = f64::from(region.dpi) / source_page.dpi;
        let scale_x = f64::from(region.full_width) / f64::from(source_page.image.width());
        let scale_y = f64::from(region.full_height) / f64::from(source_page.image.height());
        let scale_is_consistent = |actual: f64| {
            actual.is_finite()
                && expected_scale.is_finite()
                && expected_scale > 0.0
                && ((actual - expected_scale).abs() / expected_scale) <= 0.02
        };
        if !scale_is_consistent(scale_x) || !scale_is_consistent(scale_y) {
            return None;
        }
        Some(Self {
            image: &region.image,
            dpi: region.dpi,
            source_width: source_page.image.width(),
            source_height: source_page.image.height(),
            full_width: region.full_width,
            full_height: region.full_height,
            offset_x: region.x,
            offset_y: region.y,
        })
    }

    fn map_bbox(&self, bbox: &BoundingBox) -> Option<BoundingBox> {
        if self.source_width == 0
            || self.source_height == 0
            || self.full_width == 0
            || self.full_height == 0
        {
            return None;
        }
        let scale_x = f64::from(self.full_width) / f64::from(self.source_width);
        let scale_y = f64::from(self.full_height) / f64::from(self.source_height);
        let x0 = bbox.x0 * scale_x - f64::from(self.offset_x);
        let y0 = bbox.y0 * scale_y - f64::from(self.offset_y);
        let x1 = bbox.x1 * scale_x - f64::from(self.offset_x);
        let y1 = bbox.y1 * scale_y - f64::from(self.offset_y);
        let tolerance = 1.0;
        if x0 < -tolerance
            || y0 < -tolerance
            || x1 > f64::from(self.image.width()) + tolerance
            || y1 > f64::from(self.image.height()) + tolerance
        {
            return None;
        }
        let mapped = BoundingBox::new(
            x0.clamp(0.0, f64::from(self.image.width())),
            y0.clamp(0.0, f64::from(self.image.height())),
            x1.clamp(0.0, f64::from(self.image.width())),
            y1.clamp(0.0, f64::from(self.image.height())),
        );
        (mapped.x1 > mapped.x0 && mapped.y1 > mapped.y0).then_some(mapped)
    }
}

pub(crate) struct CellOcrFailure {
    pub(crate) row: u32,
    pub(crate) column: u32,
    pub(crate) reason_code: &'static str,
}

impl CellOcrFailure {
    fn matches(&self, cell: &crate::dto::TableCell) -> bool {
        self.row == cell.row && self.column == cell.column
    }

    fn into_warning(self, page_number: u32, table_index: usize) -> PipelineWarning {
        PipelineWarning::CellOcrFallbackFailed {
            page_number,
            table_index,
            row: self.row,
            column: self.column,
            reason_code: self.reason_code.to_string(),
        }
    }
}

pub(crate) struct CellOcrOutcome {
    pub(crate) confidences: Vec<f32>,
    pub(crate) failures: Vec<CellOcrFailure>,
}

fn optional_cell_ocr_result<T, O: OcrEngine>(
    result: Result<T, Box<dyn std::error::Error>>,
    ocr: &O,
    cell: &crate::dto::TableCell,
    failures: &mut Vec<CellOcrFailure>,
) -> Result<Option<T>, Box<dyn std::error::Error>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) => match ocr.recoverable_cell_error_reason(error.as_ref()) {
            Some(reason_code) => {
                if !failures.iter().any(|failure| failure.matches(cell)) {
                    failures.push(CellOcrFailure {
                        row: cell.row,
                        column: cell.column,
                        reason_code,
                    });
                }
                Ok(None)
            }
            None => Err(error),
        },
    }
}

#[cfg(test)]
pub(crate) fn fill_empty_grid_cells_with_ocr<O: OcrEngine>(
    grid: &mut TableGrid,
    page: &GrayImage,
    ocr: &O,
    max_pixels: usize,
    dpi: u32,
    timings: &mut PageStageTimings,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    Ok(
        fill_empty_grid_cells_with_ocr_diagnostics(grid, page, ocr, max_pixels, dpi, timings)?
            .confidences,
    )
}

pub(crate) fn fill_empty_grid_cells_with_ocr_diagnostics<O: OcrEngine>(
    grid: &mut TableGrid,
    page: &GrayImage,
    ocr: &O,
    max_pixels: usize,
    dpi: u32,
    timings: &mut PageStageTimings,
) -> Result<CellOcrOutcome, Box<dyn std::error::Error>> {
    fill_empty_grid_cells_with_ocr_from_raster(
        grid,
        &CellOcrRaster::full_page(page, dpi),
        None,
        ocr,
        max_pixels,
        timings,
    )
}

fn fill_empty_grid_cells_with_ocr_from_region<O: OcrEngine>(
    grid: &mut TableGrid,
    source_page: &RenderedPage,
    region: &RenderedPageRegion,
    ocr: &O,
    max_pixels: usize,
    timings: &mut PageStageTimings,
) -> Result<CellOcrOutcome, Box<dyn std::error::Error>> {
    let fallback =
        CellOcrRaster::full_page(&source_page.image, source_page.dpi.round().max(1.0) as u32);
    let Some(primary) = CellOcrRaster::from_region(source_page, region) else {
        return fill_empty_grid_cells_with_ocr_from_raster(
            grid, &fallback, None, ocr, max_pixels, timings,
        );
    };
    fill_empty_grid_cells_with_ocr_from_raster(
        grid,
        &primary,
        Some(&fallback),
        ocr,
        max_pixels,
        timings,
    )
}

fn fill_empty_grid_cells_with_ocr_from_raster<O: OcrEngine>(
    grid: &mut TableGrid,
    raster: &CellOcrRaster<'_>,
    fallback_raster: Option<&CellOcrRaster<'_>>,
    ocr: &O,
    max_pixels: usize,
    timings: &mut PageStageTimings,
) -> Result<CellOcrOutcome, Box<dyn std::error::Error>> {
    // A later alternate/technical pass can fail after an earlier successful
    // mutation. Keep a transaction snapshot so that failed cells retain every
    // original field, including text, confidence, spans and physical bbox.
    let original_cells = grid.cells.clone();
    let mut failures = Vec::new();
    let mut fallback_indices = Vec::new();
    let mut alternate_crops = Vec::new();
    let mut retained_crop_pixels = 0usize;
    for (cell_index, cell) in grid.cells.iter_mut().enumerate() {
        if !cell.text.trim().is_empty() {
            continue;
        }
        let Some(bbox) = cell.bbox.as_ref() else {
            continue;
        };
        let stage_started = Instant::now();
        let mut prepared_crop = match raster.map_bbox(bbox) {
            Some(mapped_bbox) => {
                prepare_cell_ocr_crop(raster.image, &mapped_bbox, max_pixels, raster.dpi)?
            }
            None => None,
        };
        if prepared_crop.is_none() {
            if let Some(fallback) = fallback_raster {
                if let Some(mapped_bbox) = fallback.map_bbox(bbox) {
                    prepared_crop = prepare_cell_ocr_crop(
                        fallback.image,
                        &mapped_bbox,
                        max_pixels,
                        fallback.dpi,
                    )?;
                }
            }
        }
        timings.preprocessing_ms += elapsed_ms(stage_started);
        let Some(crop) = prepared_crop else {
            continue;
        };
        let stage_started = Instant::now();
        let recognized = ocr
            .recognize_text_with_request(&crop.image, OcrRequest::single_line(crop.effective_dpi));
        timings.tesseract_ms += elapsed_ms(stage_started);
        let Some(recognized) = optional_cell_ocr_result(recognized, ocr, cell, &mut failures)?
        else {
            continue;
        };
        let text = normalize_cell_ocr_text(&recognized.text);
        if accept_cell_ocr_candidate(&text, recognized.confidence) {
            cell.text = text;
            if let Some(confidence) = recognized.confidence {
                cell.confidence = confidence;
            }
            fallback_indices.push(cell_index);
        }

        // Retain the compact prepared pixels only until the table header has
        // been reconstructed. A standard nine-column engineering
        // specification can then recheck its manufacturer token with the
        // already-bundled English model. Other tables never pay for this pass.
        if matches!(cell.column, 0 | 4 | 6) {
            let crop_pixels = usize::try_from(crop.image.width())
                .ok()
                .and_then(|width| width.checked_mul(usize::try_from(crop.image.height()).ok()?));
            if crop_pixels.is_some_and(|pixels| {
                retained_crop_pixels
                    .checked_add(pixels)
                    .is_some_and(|total| total <= max_pixels)
            }) {
                retained_crop_pixels += crop_pixels.expect("validated retained crop pixels");
                alternate_crops.push((cell_index, crop));
            }
        }
    }

    if let Some(header_row) = numbered_engineering_specification_header_row(grid) {
        for (cell_index, crop) in alternate_crops {
            let cell = &grid.cells[cell_index];
            if cell.row <= header_row || failures.iter().any(|failure| failure.matches(cell)) {
                continue;
            }
            if matches!(cell.column, 0 | 6) {
                let role = if cell.column == 0 {
                    NumericCellRole::Position
                } else {
                    NumericCellRole::Quantity
                };
                let stage_started = Instant::now();
                let word = ocr.recognize_text_with_request(
                    &crop.image,
                    OcrRequest::numeric_cell(crop.effective_dpi),
                );
                timings.tesseract_ms += elapsed_ms(stage_started);
                let Some(word) = optional_cell_ocr_result(word, ocr, cell, &mut failures)? else {
                    continue;
                };
                let stage_started = Instant::now();
                let line = ocr.recognize_text_with_request(
                    &crop.image,
                    OcrRequest::numeric_line(crop.effective_dpi),
                );
                timings.tesseract_ms += elapsed_ms(stage_started);
                let Some(line) = optional_cell_ocr_result(line, ocr, cell, &mut failures)? else {
                    continue;
                };
                if let Some((text, confidence)) = choose_numeric_cell_candidate(
                    &cell.text,
                    cell.confidence,
                    &word.text,
                    word.confidence,
                    &line.text,
                    line.confidence,
                    role,
                ) {
                    let cell = &mut grid.cells[cell_index];
                    cell.text = text;
                    cell.confidence = confidence;
                    if !fallback_indices.contains(&cell_index) {
                        fallback_indices.push(cell_index);
                    }
                }
                continue;
            }
            if cell.column != 4 {
                continue;
            }
            if ocr.active_language_profile() == OcrLanguageProfile::SimplifiedChinese
                && crate::language::contains_han(&cell.text)
            {
                continue;
            }
            let stage_started = Instant::now();
            let english = ocr.recognize_text_with_request(
                &crop.image,
                OcrRequest::single_line_with_language(
                    crop.effective_dpi,
                    OcrLanguageProfile::EnglishOnly,
                ),
            );
            timings.tesseract_ms += elapsed_ms(stage_started);
            let Some(english) = optional_cell_ocr_result(english, ocr, cell, &mut failures)? else {
                continue;
            };
            let english_text = normalize_cell_ocr_text(&english.text);
            if !prefer_english_manufacturer_candidate(
                &cell.text,
                cell.confidence,
                &english_text,
                english.confidence,
            ) {
                continue;
            }
            let cell = &mut grid.cells[cell_index];
            cell.text = english_text;
            if let Some(confidence) = english.confidence {
                cell.confidence = confidence;
            }
            if !fallback_indices.contains(&cell_index) {
                fallback_indices.push(cell_index);
            }
        }
        // A failed alternate request invalidates that cell's tentative primary
        // candidate as evidence for other cells' table-wide consensus too.
        for (cell, original) in grid.cells.iter_mut().zip(&original_cells) {
            if failures.iter().any(|failure| failure.matches(cell)) {
                *cell = original.clone();
            }
        }
        for cell_index in apply_repeated_ascii_manufacturer_consensus(grid, header_row) {
            if !fallback_indices.contains(&cell_index) {
                fallback_indices.push(cell_index);
            }
        }
    }

    for cell_index in recheck_suspicious_nonempty_cells_with_diagnostics(
        grid,
        raster,
        fallback_raster,
        ocr,
        max_pixels,
        timings,
        &mut failures,
    )? {
        if !fallback_indices.contains(&cell_index) {
            fallback_indices.push(cell_index);
        }
    }

    for (cell, original) in grid.cells.iter_mut().zip(original_cells) {
        if failures.iter().any(|failure| failure.matches(cell)) {
            *cell = original;
        }
    }
    let confidences = fallback_indices
        .into_iter()
        .filter_map(|index| {
            let cell = &grid.cells[index];
            (cell.confidence > 0.0 && !failures.iter().any(|failure| failure.matches(cell)))
                .then_some(cell.confidence)
        })
        .collect();
    Ok(CellOcrOutcome {
        confidences,
        failures,
    })
}

/// Rechecks only bounded, role-confirmed technical cells. The configured
/// `rus+eng` page model remains authoritative; a replacement needs exact
/// agreement from the ordinary English cell pass and the separately trained,
/// versioned technical model. Searchable/native cells use confidence 1.0 and
/// are deliberately excluded.
#[cfg(test)]
fn recheck_suspicious_nonempty_cells<O: OcrEngine>(
    grid: &mut TableGrid,
    raster: &CellOcrRaster<'_>,
    fallback_raster: Option<&CellOcrRaster<'_>>,
    ocr: &O,
    max_pixels: usize,
    timings: &mut PageStageTimings,
) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
    let originals = grid.cells.clone();
    let mut failures = Vec::new();
    let mut replacements = recheck_suspicious_nonempty_cells_with_diagnostics(
        grid,
        raster,
        fallback_raster,
        ocr,
        max_pixels,
        timings,
        &mut failures,
    )?;
    for (cell, original) in grid.cells.iter_mut().zip(originals) {
        if failures.iter().any(|failure| failure.matches(cell)) {
            *cell = original;
        }
    }
    replacements.retain(|&index| {
        !failures
            .iter()
            .any(|failure| failure.matches(&grid.cells[index]))
    });
    Ok(replacements)
}

#[expect(
    clippy::needless_character_iteration,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
fn recheck_suspicious_nonempty_cells_with_diagnostics<O: OcrEngine>(
    grid: &mut TableGrid,
    raster: &CellOcrRaster<'_>,
    fallback_raster: Option<&CellOcrRaster<'_>>,
    ocr: &O,
    max_pixels: usize,
    timings: &mut PageStageTimings,
    failures: &mut Vec<CellOcrFailure>,
) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
    let cells = u32::try_from(grid.cells.len()).unwrap_or(u32::MAX);
    let empty_cells = u32::try_from(
        grid.cells
            .iter()
            .filter(|cell| cell.text.trim().is_empty())
            .count(),
    )
    .unwrap_or(u32::MAX);
    let structural_metrics = TableAssignmentMetrics {
        table_bbox: grid.bbox.clone(),
        rows: grid.rows(),
        columns: grid.columns(),
        cells,
        empty_cells,
        ocr_words_total: 0,
        words_assigned: 0,
        words_unassigned: 0,
        duplicate_assignments: 0,
        detector_confidence: grid.confidence,
    };
    if classify_grid(
        grid,
        &structural_metrics,
        raster.source_width,
        raster.source_height,
    ) == TableType::TitleBlock
    {
        return Ok(Vec::new());
    }

    let classification = classify_table(&grid.to_matrix(), None);
    let Some(header_row) = classification.header_rows.iter().copied().max() else {
        return Ok(Vec::new());
    };
    let roles = classification
        .column_roles
        .into_iter()
        .filter(|assignment| assignment.confidence >= TECHNICAL_ROLE_MIN_CONFIDENCE)
        .map(|assignment| (assignment.column as u32, assignment.role))
        .collect::<HashMap<_, _>>();
    if roles.len() < 2 {
        return Ok(Vec::new());
    }

    // Chinese descriptions are source content, never noise to strip before
    // an English-only technical recheck.
    let protect_chinese = ocr.active_language_profile() == OcrLanguageProfile::SimplifiedChinese;
    let mut replacements = Vec::new();
    for cell in &mut grid.cells {
        if cell.row as usize <= header_row
            || cell.text.trim().is_empty()
            || cell.confidence >= 0.999
            || failures.iter().any(|failure| failure.matches(cell))
        {
            continue;
        }
        let Some(role) = roles.get(&cell.column).copied() else {
            continue;
        };
        if !matches!(role, ColumnRole::Name | ColumnRole::Designation) {
            continue;
        }
        if protect_chinese && cell.text.chars().any(crate::language::is_han) {
            continue;
        }
        if let Some(cleaned) = extract_existing_english_segments(&cell.text) {
            if cleaned != cell.text.trim() {
                cell.text = cleaned;
            }
        }
    }

    let mut candidates = grid
        .cells
        .iter()
        .enumerate()
        .filter_map(|(index, cell)| {
            if cell.row as usize <= header_row
                || cell.text.trim().is_empty()
                || cell.confidence >= 0.999
                || failures.iter().any(|failure| failure.matches(cell))
            {
                return None;
            }
            let role = roles.get(&cell.column).copied()?;
            if !matches!(
                role,
                ColumnRole::Code
                    | ColumnRole::Designation
                    | ColumnRole::Manufacturer
                    | ColumnRole::CableMark
            ) {
                return None;
            }
            let text = cell.text.trim();
            if protect_chinese && text.chars().any(crate::language::is_han) {
                return None;
            }
            (is_ascii_technical_code(text)
                || cell.confidence < 0.75
                || text.chars().any(|character| !character.is_ascii())
                || (protect_chinese && suspicious_ascii_technical_token(text)))
            .then_some((index, cell.confidence))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.1.total_cmp(&right.1));
    candidates.truncate(TECHNICAL_CELL_RECHECK_LIMIT);

    for (cell_index, _) in candidates {
        let (bbox, primary_text, primary_confidence) = {
            let cell = &grid.cells[cell_index];
            let Some(bbox) = cell.bbox.clone() else {
                continue;
            };
            (bbox, cell.text.clone(), cell.confidence)
        };
        let stage_started = Instant::now();
        let mut prepared_crop = match raster.map_bbox(&bbox) {
            Some(mapped_bbox) => {
                prepare_cell_ocr_crop(raster.image, &mapped_bbox, max_pixels, raster.dpi)?
            }
            None => None,
        };
        if prepared_crop.is_none() {
            if let Some(fallback) = fallback_raster {
                if let Some(mapped_bbox) = fallback.map_bbox(&bbox) {
                    prepared_crop = prepare_cell_ocr_crop(
                        fallback.image,
                        &mapped_bbox,
                        max_pixels,
                        fallback.dpi,
                    )?;
                }
            }
        }
        timings.preprocessing_ms += elapsed_ms(stage_started);
        let Some(crop) = prepared_crop else {
            continue;
        };

        let request = OcrRequest::single_line_with_language(
            crop.effective_dpi,
            OcrLanguageProfile::EnglishOnly,
        );
        let stage_started = Instant::now();
        let technical = ocr.recognize_technical_text_with_request(&crop.image, request);
        timings.tesseract_ms += elapsed_ms(stage_started);
        let technical = match technical {
            Ok(result) => result,
            Err(error) => {
                if let Some(reason_code) = ocr.recoverable_cell_error_reason(error.as_ref()) {
                    let cell = &grid.cells[cell_index];
                    if !failures.iter().any(|failure| failure.matches(cell)) {
                        failures.push(CellOcrFailure {
                            row: cell.row,
                            column: cell.column,
                            reason_code,
                        });
                    }
                    continue;
                }
                // This independently trained model is optional. Preserve the
                // established fallback when it cannot load or run; failures
                // of the required configured/generic engine still propagate.
                break;
            }
        };
        let Some(technical) = technical else {
            // Model absence is still optional; do not impersonate independent
            // evidence with a second pass of the ordinary page model.
            break;
        };
        let stage_started = Instant::now();
        let generic = ocr.recognize_text_with_request(&crop.image, request);
        timings.tesseract_ms += elapsed_ms(stage_started);
        let Some(generic) =
            optional_cell_ocr_result(generic, ocr, &grid.cells[cell_index], failures)?
        else {
            continue;
        };
        if let Some((text, confidence)) = choose_technical_cell_candidate(
            &primary_text,
            primary_confidence,
            &generic.text,
            generic.confidence,
            &technical.text,
            technical.confidence,
        ) {
            let cell = &mut grid.cells[cell_index];
            cell.text = text;
            cell.confidence = confidence;
            if !replacements.contains(&cell_index) {
                replacements.push(cell_index);
            }
        }
    }

    Ok(replacements)
}

fn choose_technical_cell_candidate(
    primary_text: &str,
    primary_confidence: f32,
    generic_text: &str,
    generic_confidence: Option<f32>,
    technical_text: &str,
    technical_confidence: Option<f32>,
) -> Option<(String, f32)> {
    let generic = normalize_cell_ocr_text(generic_text);
    let technical = normalize_cell_ocr_text(technical_text);
    let generic_confidence = generic_confidence?;
    let technical_confidence = technical_confidence?;
    if technical_confidence < TECHNICAL_MODEL_MIN_CONFIDENCE || !is_ascii_technical_code(&technical)
    {
        return None;
    }
    let primary = normalize_cell_ocr_text(primary_text);
    if primary == technical {
        return None;
    }

    let primary_is_valid = is_ascii_technical_code(&primary);
    let edit_distance = character_edit_distance(
        &primary.to_ascii_uppercase(),
        &technical.to_ascii_uppercase(),
    );
    let exact_cross_model_consensus =
        generic_confidence >= TECHNICAL_MODEL_MIN_CONFIDENCE && generic == technical;
    if exact_cross_model_consensus {
        let agreed_confidence = generic_confidence.min(technical_confidence);
        let bounded_typo = edit_distance <= 1 || (edit_distance <= 2 && primary_confidence < 0.50);
        let invalid_low_confidence =
            !primary_is_valid && primary_confidence < 0.55 && agreed_confidence >= 0.65;
        return (bounded_typo || invalid_low_confidence)
            .then_some((technical, agreed_confidence.clamp(0.0, 1.0)));
    }

    // The ordinary page/cell model can repeat the same one-glyph cable-mark
    // error. A separately trained model may resolve that tie only for a
    // compound technical grammar, with a one-edit delta and a measurable
    // confidence advantage. This does not authorize broad dictionary-based
    // replacement or a change to the full-page rus+eng result.
    let compound_separators = technical
        .chars()
        .filter(|character| matches!(character, '*' | '+' | '.'))
        .count();
    let specialized_single_edit = primary == generic
        && edit_distance == 1
        && compound_separators >= 2
        && primary_confidence < 0.85
        && technical_confidence >= 0.85
        && technical_confidence >= generic_confidence + 0.02;
    specialized_single_edit.then_some((technical, technical_confidence.clamp(0.0, 1.0)))
}

fn is_ascii_technical_code(text: &str) -> bool {
    let text = text.trim();
    let length = text.chars().count();
    (3..=64).contains(&length)
        && text
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        && text.chars().any(|character| character.is_ascii_digit())
        && text.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character.is_ascii_whitespace()
                || matches!(
                    character,
                    '*' | '+' | '.' | ',' | '-' | '_' | '/' | '(' | ')'
                )
        })
}

// A multilingual recognizer can emit a confident but invalid punctuation
// character inside a Latin technical token. Selection only requests evidence:
// the existing bounded edit/independent-model agreement still decides changes.
fn suspicious_ascii_technical_token(text: &str) -> bool {
    text.is_ascii()
        && (3..=64).contains(&text.len())
        && text
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        && text.chars().any(|character| character.is_ascii_digit())
        && !is_ascii_technical_code(text)
}

fn character_edit_distance(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (row, left_character) in left.chars().enumerate() {
        let mut current = vec![row + 1];
        for (column, right_character) in right.iter().enumerate() {
            current.push(
                (current[column] + 1)
                    .min(previous[column + 1] + 1)
                    .min(previous[column] + usize::from(left_character != *right_character)),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

/// Removes only a noisy prefix separated by Tesseract's vertical-rule token.
/// Every retained segment is already present verbatim in the primary OCR, so
/// this cleanup cannot invent a translation or a domain value.
fn extract_existing_english_segments(primary_text: &str) -> Option<String> {
    let segments = primary_text.split('|').collect::<Vec<_>>();
    if segments.len() < 2 {
        return None;
    }
    let prefix = segments[0].trim();
    let contains_cjk_noise = prefix.chars().any(
        |character| matches!(character as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF),
    );
    if !contains_cjk_noise {
        return None;
    }
    let retained = segments
        .iter()
        .skip(1)
        .map(|segment| segment.trim().trim_matches(['"', '\'', '.', ';']))
        .filter(|segment| is_readable_ascii_phrase(segment) || is_ascii_technical_code(segment))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    (!retained.is_empty()).then(|| retained.join("\n"))
}

fn is_readable_ascii_phrase(text: &str) -> bool {
    if text.is_empty()
        || !text.is_ascii()
        || !text.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character.is_ascii_whitespace()
                || matches!(character, ',' | '-' | '/' | '(' | ')' | '.')
        })
    {
        return false;
    }
    let words = text
        .split_whitespace()
        .filter(|word| {
            word.chars()
                .any(|character| character.is_ascii_alphabetic())
        })
        .count();
    let letters = text
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .count();
    words >= 2 && letters >= 5
}

/// Repairs a rare one-glyph OCR outlier only when the same compact ASCII
/// manufacturer is repeated at least three times in the confirmed
/// manufacturer column of a numbered engineering specification. This is
/// table-local evidence, not a global brand dictionary or homoglyph rewrite.
fn apply_repeated_ascii_manufacturer_consensus(
    grid: &mut TableGrid,
    header_row: u32,
) -> Vec<usize> {
    let mut occurrences = HashMap::<String, (String, usize)>::new();
    for cell in &grid.cells {
        if cell.row <= header_row
            || cell.column != 4
            || cell.confidence >= 0.999
            || !is_compact_ascii_identifier(cell.text.trim())
        {
            continue;
        }
        let text = cell.text.trim();
        let key = text.to_ascii_uppercase();
        let entry = occurrences
            .entry(key)
            .or_insert_with(|| (text.to_string(), 0));
        entry.1 += 1;
    }
    let Some((dominant_key, (dominant_text, count))) =
        occurrences.into_iter().max_by_key(|(_, (_, count))| *count)
    else {
        return Vec::new();
    };
    if count < 3 || dominant_key.chars().count() < 3 {
        return Vec::new();
    }

    let mut changed = Vec::new();
    for (index, cell) in grid.cells.iter_mut().enumerate() {
        if cell.row <= header_row
            || cell.column != 4
            || cell.confidence >= 0.999
            || !is_compact_ascii_identifier(cell.text.trim())
        {
            continue;
        }
        let candidate = cell.text.trim().to_ascii_uppercase();
        if candidate == dominant_key || !ascii_edit_distance_at_most_one(&candidate, &dominant_key)
        {
            continue;
        }
        cell.text = dominant_text.clone();
        changed.push(index);
    }
    changed
}

fn ascii_edit_distance_at_most_one(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len().abs_diff(right.len()) > 1 {
        return false;
    }
    if left.len() == right.len() {
        return left
            .iter()
            .zip(right)
            .filter(|(left, right)| left != right)
            .count()
            <= 1;
    }
    let (shorter, longer) = if left.len() < right.len() {
        (left, right)
    } else {
        (right, left)
    };
    let mut short_index = 0;
    let mut long_index = 0;
    let mut skipped = false;
    while short_index < shorter.len() && long_index < longer.len() {
        if shorter[short_index] == longer[long_index] {
            short_index += 1;
            long_index += 1;
        } else if skipped {
            return false;
        } else {
            skipped = true;
            long_index += 1;
        }
    }
    true
}

struct PreparedCellOcrCrop {
    image: GrayImage,
    effective_dpi: u32,
}

/// Prepares only the bounded cell crop for OCR. The scale is derived from the
/// actual foreground height, not from a document-wide DPI assumption. This is
/// important for searchable engineering PDFs: their grid thumbnail remains
/// intentionally small, while tiny raster-only cell text still reaches
/// Tesseract at a useful glyph size.
fn prepare_cell_ocr_crop(
    page: &GrayImage,
    bbox: &crate::dto::BoundingBox,
    max_pixels: usize,
    dpi: u32,
) -> Result<Option<PreparedCellOcrCrop>, Box<dyn std::error::Error>> {
    let Some(mut crop) = informative_cell_crop(page, bbox) else {
        return Ok(None);
    };

    let mut foreground_threshold = otsu_level(&crop).clamp(96, 235);
    erase_boundary_grid_lines(&mut crop, foreground_threshold);
    // Recompute after rule removal: a dark frame must not determine the
    // threshold used to find the remaining glyphs.
    foreground_threshold = otsu_level(&crop).clamp(96, 235);

    let Some((ink_x0, ink_y0, ink_x1, ink_y1, dark_pixels)) =
        foreground_bounds(&crop, foreground_threshold)
    else {
        return Ok(None);
    };
    let area = crop.width() as usize * crop.height() as usize;
    // Grid rules have already been removed. Retain very small but legitimate
    // digits and punctuation; the confidence/script gate below rejects the
    // residual low-confidence line glyphs.
    let minimum_dark = (area / 500).max(3);
    if dark_pixels < minimum_dark {
        return Ok(None);
    }

    let ink_width = ink_x1 - ink_x0;
    let ink_height = ink_y1 - ink_y0;
    if ink_width == 0 || ink_height < 2 {
        return Ok(None);
    }

    let source_padding = (ink_height / 4).clamp(2, 8);
    let crop_x0 = ink_x0.saturating_sub(source_padding);
    let crop_y0 = ink_y0.saturating_sub(source_padding);
    let crop_x1 = ink_x1.saturating_add(source_padding).min(crop.width());
    let crop_y1 = ink_y1.saturating_add(source_padding).min(crop.height());
    let tight = imageops::crop_imm(
        &crop,
        crop_x0,
        crop_y0,
        crop_x1 - crop_x0,
        crop_y1 - crop_y0,
    )
    .to_image();
    let normalized = preprocess(tight, max_pixels)?;

    let scale = CELL_OCR_TARGET_INK_HEIGHT
        .div_ceil(ink_height.max(1))
        .clamp(1, CELL_OCR_MAX_SCALE);
    let scaled_width = normalized
        .width()
        .checked_mul(scale)
        .ok_or("cell OCR width overflow")?;
    let scaled_height = normalized
        .height()
        .checked_mul(scale)
        .ok_or("cell OCR height overflow")?;
    let border = (scale * 2).clamp(4, 12);
    let output_width = scaled_width
        .checked_add(border.saturating_mul(2))
        .ok_or("cell OCR padded width overflow")?;
    let output_height = scaled_height
        .checked_add(border.saturating_mul(2))
        .ok_or("cell OCR padded height overflow")?;
    let output_pixels = (output_width as usize)
        .checked_mul(output_height as usize)
        .ok_or("cell OCR pixel count overflow")?;
    if output_pixels > max_pixels {
        return Err(format!(
            "cell OCR crop too large after adaptive scaling: {output_pixels} pixels exceeds limit of {max_pixels}"
        )
        .into());
    }

    let scaled = if scale == 1 {
        normalized
    } else {
        imageops::resize(
            &normalized,
            scaled_width,
            scaled_height,
            imageops::FilterType::Lanczos3,
        )
    };
    let mut padded = GrayImage::from_pixel(output_width, output_height, image::Luma([255]));
    imageops::replace(&mut padded, &scaled, i64::from(border), i64::from(border));

    Ok(Some(PreparedCellOcrCrop {
        image: padded,
        effective_dpi: dpi.max(1).saturating_mul(scale).clamp(70, 2_400),
    }))
}

/// Removes only near-boundary, page-spanning strokes. Interior dashes,
/// underscores, `I`, `1`, and `l` are deliberately left untouched.
fn erase_boundary_grid_lines(image: &mut GrayImage, threshold: u8) {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return;
    }
    let x_band = (width / 8).clamp(1, 12);
    let y_band = (height / 8).clamp(1, 12);
    let mut erase_columns = vec![false; width as usize];
    let mut erase_rows = vec![false; height as usize];

    for x in (0..x_band).chain(width.saturating_sub(x_band)..width) {
        let dark = (0..height)
            .filter(|&y| image.get_pixel(x, y)[0] <= threshold)
            .count();
        if dark * 5 >= height as usize * 3 {
            for column in x.saturating_sub(1)..=x.saturating_add(1).min(width - 1) {
                erase_columns[column as usize] = true;
            }
        }
    }
    for y in (0..y_band).chain(height.saturating_sub(y_band)..height) {
        let dark = (0..width)
            .filter(|&x| image.get_pixel(x, y)[0] <= threshold)
            .count();
        if dark * 5 >= width as usize * 3 {
            for row in y.saturating_sub(1)..=y.saturating_add(1).min(height - 1) {
                erase_rows[row as usize] = true;
            }
        }
    }

    for (x, erase) in erase_columns.into_iter().enumerate() {
        if erase {
            for y in 0..height {
                image.put_pixel(x as u32, y, image::Luma([255]));
            }
        }
    }
    for (y, erase) in erase_rows.into_iter().enumerate() {
        if erase {
            for x in 0..width {
                image.put_pixel(x, y as u32, image::Luma([255]));
            }
        }
    }
}

fn foreground_bounds(image: &GrayImage, threshold: u8) -> Option<(u32, u32, u32, u32, usize)> {
    let mut x0 = image.width();
    let mut y0 = image.height();
    let mut x1 = 0;
    let mut y1 = 0;
    let mut count = 0usize;
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[0] > threshold {
            continue;
        }
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x.saturating_add(1));
        y1 = y1.max(y.saturating_add(1));
        count += 1;
    }
    (count > 0).then_some((x0, y0, x1, y1, count))
}

fn normalize_cell_ocr_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn numbered_engineering_specification_header_row(grid: &TableGrid) -> Option<u32> {
    let columns = grid.columns();
    if columns != 9 || grid.rows() < 4 {
        return None;
    }
    let required_matches = columns.saturating_sub(2).max(5);
    (0..grid.rows().min(4)).find(|&row| {
        let matches = grid
            .cells
            .iter()
            .filter(|cell| cell.row == row)
            .filter(|cell| {
                cell.text
                    .trim()
                    .parse::<u32>()
                    .is_ok_and(|value| value == cell.column + 1)
            })
            .count() as u32;
        matches >= required_matches
    })
}

#[derive(Clone, Copy)]
enum NumericCellRole {
    Position,
    Quantity,
}

/// Chooses between two isolated numeric OCR profiles only inside a confirmed
/// nine-column engineering specification. A valid primary value is never
/// replaced by a different value: this deliberately avoids inferring a
/// sequence from neighbouring rows. Invalid/empty primaries require either
/// cross-profile agreement or one strong candidate while the other profile
/// has no credible competing reading.
fn choose_numeric_cell_candidate(
    primary_text: &str,
    primary_confidence: f32,
    word_text: &str,
    word_confidence: Option<f32>,
    line_text: &str,
    line_confidence: Option<f32>,
    role: NumericCellRole,
) -> Option<(String, f32)> {
    let primary = compact_numeric_text(primary_text);
    let primary_is_valid = valid_numeric_cell(&primary, role);
    let word = numeric_candidate(word_text, word_confidence, role);
    let line = numeric_candidate(line_text, line_confidence, role);

    if primary_is_valid {
        let confirming_confidence = [word.as_ref(), line.as_ref()]
            .into_iter()
            .flatten()
            .filter(|(text, confidence)| text == &primary && *confidence >= 0.70)
            .map(|(_, confidence)| confidence)
            .max_by(|left, right| left.total_cmp(right));
        return confirming_confidence.and_then(|confidence| {
            (primary != primary_text.trim())
                .then_some((primary, primary_confidence.max(*confidence)))
        });
    }

    match (word, line) {
        (Some((word_text, word_confidence)), Some((line_text, line_confidence)))
            if word_text == line_text
                && word_confidence.max(line_confidence) >= 0.80
                && word_confidence.min(line_confidence) >= 0.50 =>
        {
            Some((word_text, word_confidence.max(line_confidence)))
        }
        (Some(word), Some(line)) => {
            let (winner, competitor) = if word.1 >= line.1 {
                (word, line)
            } else {
                (line, word)
            };
            (winner.1 >= 0.80 && competitor.1 < 0.40).then_some(winner)
        }
        (Some(candidate), None) | (None, Some(candidate)) => {
            (candidate.1 >= 0.80).then_some(candidate)
        }
        (None, None) => None,
    }
}

fn numeric_candidate(
    text: &str,
    confidence: Option<f32>,
    role: NumericCellRole,
) -> Option<(String, f32)> {
    let confidence = confidence?;
    let text = compact_numeric_text(text);
    valid_numeric_cell(&text, role).then_some((text, confidence.clamp(0.0, 1.0)))
}

fn compact_numeric_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn valid_numeric_cell(text: &str, role: NumericCellRole) -> bool {
    if text.is_empty() || !text.chars().any(|character| character.is_ascii_digit()) {
        return false;
    }
    let separators = text
        .chars()
        .filter(|character| matches!(character, '.' | ','))
        .count();
    if role_matches_quantity(role) && separators > 1 {
        return false;
    }
    text.split(['.', ',']).all(|segment| {
        !segment.is_empty() && segment.chars().all(|character| character.is_ascii_digit())
    })
}

fn role_matches_quantity(role: NumericCellRole) -> bool {
    matches!(role, NumericCellRole::Quantity)
}

fn prefer_english_manufacturer_candidate(
    primary_text: &str,
    primary_confidence: f32,
    english_text: &str,
    english_confidence: Option<f32>,
) -> bool {
    let Some(english_confidence) = english_confidence else {
        return false;
    };
    if english_confidence < 0.70 || !is_compact_ascii_identifier(english_text) {
        return false;
    }
    if primary_text.is_empty() {
        return true;
    }
    if !is_compact_manufacturer_token(primary_text) {
        return false;
    }
    if primary_text.eq_ignore_ascii_case(english_text) {
        return english_confidence > primary_confidence;
    }
    english_confidence >= primary_confidence + 0.05
}

fn is_compact_ascii_identifier(text: &str) -> bool {
    let length = text.chars().count();
    (2..=24).contains(&length)
        && text
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        && text.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '/')
        })
}

fn is_compact_manufacturer_token(text: &str) -> bool {
    let length = text.chars().count();
    (2..=24).contains(&length)
        && text.chars().any(|character| character.is_alphabetic())
        && text.chars().all(|character| {
            character.is_alphanumeric() || matches!(character, '-' | '_' | '.' | '/')
        })
}

fn accept_cell_ocr_candidate(text: &str, confidence: Option<f32>) -> bool {
    if text.is_empty() {
        return false;
    }
    if text
        .chars()
        .all(|character| character.is_whitespace() || matches!(character, '|' | '¦' | '│' | '┃'))
    {
        return false;
    }
    let Some(confidence) = confidence else {
        // Custom/test engines predating confidence support remain compatible.
        return true;
    };
    if confidence < CELL_OCR_MIN_CONFIDENCE {
        return false;
    }
    let letters = text.chars().filter(|character| character.is_alphabetic());
    let letter_count = letters.clone().count();
    let looks_like_grid_noise = letter_count > 0
        && letter_count <= 4
        && text.chars().all(|character| {
            character.is_whitespace()
                || matches!(character, 'и' | 'И' | 'о' | 'О' | 'i' | 'I' | 'l' | '|')
        });
    !looks_like_grid_noise || confidence >= CELL_OCR_GRID_NOISE_CONFIDENCE
}

fn informative_cell_crop(page: &GrayImage, bbox: &crate::dto::BoundingBox) -> Option<GrayImage> {
    let width = page.width();
    let height = page.height();
    let mut x0 = bbox.x0.floor().max(0.0).min(f64::from(width)) as u32;
    let mut y0 = bbox.y0.floor().max(0.0).min(f64::from(height)) as u32;
    let mut x1 = bbox.x1.ceil().max(0.0).min(f64::from(width)) as u32;
    let mut y1 = bbox.y1.ceil().max(0.0).min(f64::from(height)) as u32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    // Exclude grid rules along the cell boundary before testing whether the
    // nominally empty cell actually contains raster text.
    let inset_x = ((x1 - x0) / 25).clamp(1, 4);
    let inset_y = ((y1 - y0) / 25).clamp(1, 4);
    x0 = x0.saturating_add(inset_x);
    y0 = y0.saturating_add(inset_y);
    x1 = x1.saturating_sub(inset_x);
    y1 = y1.saturating_sub(inset_y);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let crop = image::imageops::crop_imm(page, x0, y0, x1 - x0, y1 - y0).to_image();
    if crop.width() < MIN_CELL_OCR_WIDTH || crop.height() < MIN_CELL_OCR_HEIGHT {
        return None;
    }
    Some(crop)
}

#[cfg(test)]
fn report_stage_started(
    progress: Option<&dyn OcrProgressReporter>,
    stage: OcrProgressStage,
    page_number: u32,
    total_pages: u32,
) {
    if let Some(reporter) = progress {
        reporter.report(&OcrProgressEvent::page_stage_started(
            stage,
            page_number,
            total_pages,
        ));
    }
}
fn report_page_started(
    progress: Option<&dyn OcrProgressReporter>,
    page_number: u32,
    total_pages: u32,
) {
    let Some(reporter) = progress else {
        return;
    };
    reporter.report(&OcrProgressEvent::page_started(page_number, total_pages));
}

fn report_page_progress(
    progress: Option<&dyn OcrProgressReporter>,
    processing: &PageProcessingInfo,
    total_pages: u32,
) {
    let Some(reporter) = progress else {
        return;
    };
    let timings = processing.timings;
    if timings.native_text_extraction_ms > 0.0 {
        reporter.report(&OcrProgressEvent::page(
            OcrProgressStage::NativeText,
            processing.page_number,
            total_pages,
            processing.processing_mode,
            timings.native_text_extraction_ms,
        ));
    }
    let render_ms = timings.thumbnail_render_ms + timings.full_render_ms;
    if render_ms > 0.0 {
        reporter.report(&OcrProgressEvent::page(
            OcrProgressStage::Render,
            processing.page_number,
            total_pages,
            processing.processing_mode,
            render_ms,
        ));
    }
    if timings.tesseract_ms > 0.0 {
        reporter.report(&OcrProgressEvent::page(
            OcrProgressStage::Ocr,
            processing.page_number,
            total_pages,
            processing.processing_mode,
            timings.tesseract_ms,
        ));
    }
    if timings.grid_detection_ms > 0.0 {
        reporter.report(&OcrProgressEvent::page(
            OcrProgressStage::TableDetection,
            processing.page_number,
            total_pages,
            processing.processing_mode,
            timings.grid_detection_ms,
        ));
    }
    reporter.report(&OcrProgressEvent::page(
        OcrProgressStage::Aggregation,
        processing.page_number,
        total_pages,
        processing.processing_mode,
        timings.total_ms,
    ));
}

fn report_document_progress(
    progress: Option<&dyn OcrProgressReporter>,
    stage: OcrProgressStage,
    total_pages: u32,
) {
    if let Some(reporter) = progress {
        reporter.report(&OcrProgressEvent::document(stage, total_pages, 0.0));
    }
}

fn report_failure(
    progress: Option<&dyn OcrProgressReporter>,
    page_number: Option<u32>,
    total_pages: u32,
    code: &str,
) {
    if let Some(reporter) = progress {
        reporter.report(&OcrProgressEvent::failure(page_number, total_pages, code));
    }
}

fn mean_confidence_values(values: &[f32]) -> Option<f32> {
    (!values.is_empty())
        .then(|| (values.iter().copied().sum::<f32>() / values.len() as f32).clamp(0.0, 1.0))
}

pub(crate) fn page_word_assignment_metrics(
    words: &[crate::dto::Word],
    grids: &[TableGrid],
) -> PageWordAssignmentMetrics {
    let mut assignment_counts = vec![0_u32; words.len()];
    for (index, word) in words.iter().enumerate() {
        for grid in grids {
            if word_is_assignable_to_grid(word, grid) {
                assignment_counts[index] = assignment_counts[index].saturating_add(1);
            }
        }
    }

    let assigned_words = assignment_counts.iter().filter(|count| **count > 0).count();
    let duplicate_assignments = assignment_counts
        .iter()
        .map(|count| count.saturating_sub(1))
        .sum::<u32>();
    PageWordAssignmentMetrics {
        recognized_words: saturating_u32(words.len()),
        assigned_words: saturating_u32(assigned_words),
        words_outside_tables: saturating_u32(words.len().saturating_sub(assigned_words)),
        duplicate_assignments,
    }
}

pub(crate) fn word_is_assignable_to_grid(word: &crate::dto::Word, grid: &TableGrid) -> bool {
    if intersection_area(&word.bbox, &grid.bbox) <= 0.0 {
        return false;
    }

    let center_x = (word.bbox.x0 + word.bbox.x1) / 2.0;
    let center_y = (word.bbox.y0 + word.bbox.y1) / 2.0;
    grid.cells.iter().any(|cell| {
        cell.bbox.as_ref().is_some_and(|bbox| {
            intersection_area(&word.bbox, bbox) > 0.0
                || (center_x >= bbox.x0
                    && center_x <= bbox.x1
                    && center_y >= bbox.y0
                    && center_y <= bbox.y1)
        })
    })
}

fn intersection_area(a: &crate::dto::BoundingBox, b: &crate::dto::BoundingBox) -> f64 {
    let width = (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.0);
    let height = (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
    width * height
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn detect_orientation<O: OcrEngine>(
    image: &GrayImage,
    ocr: &O,
    dpi: u32,
) -> Result<PageOrientation, Box<dyn std::error::Error>> {
    let thumbnail = orientation_thumbnail(image);
    let mut evidence = Vec::with_capacity(ORIENTATION_ANGLES.len());
    for angle in ORIENTATION_ANGLES {
        let candidate = rotate_gray_image(thumbnail.clone(), angle)?;
        let words = ocr.recognize_words_with_request(&candidate, OcrRequest::orientation(dpi))?;
        evidence.push(OrientationEvidence {
            angle,
            word_count: words.len(),
            mean_confidence: mean_word_confidence(&words).unwrap_or(0.0),
        });
    }
    Ok(choose_orientation(&evidence))
}

fn mean_word_confidence(words: &[crate::dto::Word]) -> Option<f32> {
    (!words.is_empty()).then(|| {
        (words.iter().map(|word| word.confidence).sum::<f32>() / words.len() as f32).clamp(0.0, 1.0)
    })
}

pub(crate) fn detect_table_grids_safely(image: &GrayImage) -> Result<Vec<TableGrid>, String> {
    std::panic::catch_unwind(AssertUnwindSafe(|| detect_table_grids(image).grids))
        .map_err(|payload| format!("grid detector panicked: {}", panic_message(payload)))
}

pub(crate) fn detect_borderless_tables_safely(
    words: &[crate::dto::Word],
    page_width: u32,
    page_height: u32,
) -> Result<Vec<TableMatrix>, String> {
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        detect_borderless_tables(words, page_width, page_height)
    }))
    .map_err(|payload| {
        format!(
            "borderless table detector panicked: {}",
            panic_message(payload)
        )
    })
}

pub(crate) fn matrix_word_assignment_metrics(
    words: &[crate::dto::Word],
    matrices: &[TableMatrix],
) -> PageWordAssignmentMetrics {
    let mut assignment_counts = vec![0_u32; words.len()];
    for (index, word) in words.iter().enumerate() {
        for matrix in matrices {
            let assigned = matrix.rows.iter().flatten().any(|cell| {
                cell.bbox.as_ref().is_some_and(|bbox| {
                    let center_x = (word.bbox.x0 + word.bbox.x1) / 2.0;
                    let center_y = (word.bbox.y0 + word.bbox.y1) / 2.0;
                    intersection_area(&word.bbox, bbox) > 0.0
                        || (center_x >= bbox.x0
                            && center_x <= bbox.x1
                            && center_y >= bbox.y0
                            && center_y <= bbox.y1)
                })
            });
            if assigned {
                assignment_counts[index] = assignment_counts[index].saturating_add(1);
            }
        }
    }
    let assigned_words = assignment_counts.iter().filter(|count| **count > 0).count();
    PageWordAssignmentMetrics {
        recognized_words: saturating_u32(words.len()),
        assigned_words: saturating_u32(assigned_words),
        words_outside_tables: saturating_u32(words.len().saturating_sub(assigned_words)),
        duplicate_assignments: assignment_counts
            .iter()
            .map(|count| count.saturating_sub(1))
            .sum(),
    }
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn matrix_to_text(m: &crate::dto::TableMatrix) -> String {
    let mut out = String::new();
    for (i, row) in m.rows.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let cells: Vec<String> = row.iter().map(|c| c.text.clone()).collect();
        out.push_str(&cells.join("\t"));
    }
    out
}

/// Convenience free-function mirroring [`Pipeline::run`].
pub fn run<S, O>(
    source: &mut S,
    ocr: &O,
    max_pixels: usize,
    page_limit: Option<u32>,
) -> Result<DocumentResult, Box<dyn std::error::Error>>
where
    S: PageSource,
    O: OcrEngine,
{
    Pipeline::new(source, ocr, max_pixels, page_limit).run()
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
        thread,
        time::{Duration, Instant},
    };

    use image::{GrayImage, Luma};

    use super::{
        accept_cell_ocr_candidate, apply_repeated_ascii_manufacturer_consensus,
        choose_numeric_cell_candidate, choose_technical_cell_candidate, estimated_region_pixels,
        extract_existing_english_segments, informative_cell_crop, normalized_padded_grid_region,
        numbered_engineering_specification_header_row, prefer_english_manufacturer_candidate,
        prepare_cell_ocr_crop, process_one, recheck_suspicious_nonempty_cells, valid_numeric_cell,
        CellOcrRaster, NumericCellRole, Pipeline, PipelineWarning, TableMode,
    };
    use crate::dto::{BoundingBox, PageKind, TableCell};
    use crate::ocr::{OcrEngine, StubOcrEngine};
    use crate::page_analysis::{NativeTextLayer, PageStageTimings, ProcessingMode};
    use crate::page_source::{PageSource, RenderedPage, RenderedPageRegion, StubPageSource};
    use crate::preprocessing::DEFAULT_MAX_PIXELS;
    use crate::progress::{OcrProgressEvent, OcrProgressStage};
    use crate::table_grid::{GridIntersection, GridLine, TableGrid};

    fn grid_fixture(bbox: BoundingBox, rows: usize, columns: usize, confidence: f32) -> TableGrid {
        let horizontal_lines = (0..=rows)
            .map(|index| GridLine {
                position: bbox.y0 + (bbox.y1 - bbox.y0) * index as f64 / rows as f64,
                start: bbox.x0,
                end: bbox.x1,
            })
            .collect();
        let vertical_lines = (0..=columns)
            .map(|index| GridLine {
                position: bbox.x0 + (bbox.x1 - bbox.x0) * index as f64 / columns as f64,
                start: bbox.y0,
                end: bbox.y1,
            })
            .collect();
        let intersections = (0..=rows)
            .flat_map(|row| {
                (0..=columns).map(move |column| GridIntersection {
                    x: bbox.x0 + (bbox.x1 - bbox.x0) * column as f64 / columns as f64,
                    y: bbox.y0 + (bbox.y1 - bbox.y0) * row as f64 / rows as f64,
                })
            })
            .collect();
        TableGrid {
            cells: (0..rows)
                .flat_map(|row| {
                    (0..columns).map(move |column| {
                        let x0 = bbox.x0 + (bbox.x1 - bbox.x0) * column as f64 / columns as f64;
                        let x1 =
                            bbox.x0 + (bbox.x1 - bbox.x0) * (column + 1) as f64 / columns as f64;
                        let y0 = bbox.y0 + (bbox.y1 - bbox.y0) * row as f64 / rows as f64;
                        let y1 = bbox.y0 + (bbox.y1 - bbox.y0) * (row + 1) as f64 / rows as f64;
                        TableCell::grid(row as u32, column as u32, BoundingBox::new(x0, y0, x1, y1))
                    })
                })
                .collect(),
            bbox,
            horizontal_lines,
            vertical_lines,
            intersections,
            confidence,
        }
    }

    #[derive(Debug, thiserror::Error)]
    enum CellIsolationError {
        #[error("recoverable private cell text MUST NOT appear in warnings")]
        Recognition,
        #[error("fatal initialization failure")]
        Initialization,
    }

    struct CellIsolationOcr {
        cell_calls: AtomicUsize,
        fatal: bool,
        fail_english: bool,
        technical_init_failure: bool,
        fail_numeric: bool,
    }

    impl CellIsolationOcr {
        fn new(fatal: bool, fail_english: bool) -> Self {
            Self {
                cell_calls: AtomicUsize::new(0),
                fatal,
                fail_english,
                technical_init_failure: false,
                fail_numeric: false,
            }
        }
    }

    impl OcrEngine for CellIsolationOcr {
        fn recoverable_cell_error_reason(
            &self,
            error: &(dyn std::error::Error + 'static),
        ) -> Option<&'static str> {
            matches!(
                error.downcast_ref::<CellIsolationError>(),
                Some(CellIsolationError::Recognition)
            )
            .then_some("recognition_failed")
        }

        fn recognize_text(&self, _: &GrayImage) -> Result<String, Box<dyn std::error::Error>> {
            Ok("Canonical complete page text 123".into())
        }

        fn recognize_words(
            &self,
            _: &GrayImage,
        ) -> Result<Vec<crate::dto::Word>, Box<dyn std::error::Error>> {
            Ok(vec![crate::dto::Word::new(
                "Original",
                BoundingBox::new(55.0, 60.0, 90.0, 85.0),
                0.83,
            )])
        }

        fn recognize_text_with_request(
            &self,
            image: &GrayImage,
            request: crate::ocr::OcrRequest,
        ) -> Result<crate::ocr::OcrTextResult, Box<dyn std::error::Error>> {
            if request.segmentation == crate::ocr::OcrSegmentation::Sparse {
                return Ok(crate::ocr::OcrTextResult {
                    text: String::new(),
                    confidence: None,
                });
            }
            if request.segmentation == crate::ocr::OcrSegmentation::Automatic {
                return Ok(crate::ocr::OcrTextResult {
                    text: self.recognize_text(image)?,
                    confidence: Some(0.83),
                });
            }
            let call = self.cell_calls.fetch_add(1, Ordering::SeqCst);
            if self.fatal {
                return Err(Box::new(CellIsolationError::Initialization));
            }
            if (self.fail_english
                && request.language_profile == crate::ocr::OcrLanguageProfile::EnglishOnly)
                || (self.fail_numeric
                    && matches!(
                        request.segmentation,
                        crate::ocr::OcrSegmentation::SingleWordNumeric
                            | crate::ocr::OcrSegmentation::SingleLineNumeric
                    ))
                || (!self.fail_english && call == 0)
            {
                return Err(Box::new(CellIsolationError::Recognition));
            }
            Ok(crate::ocr::OcrTextResult {
                text: "Recovered".into(),
                confidence: Some(0.91),
            })
        }

        fn recognize_words_with_request(
            &self,
            image: &GrayImage,
            request: crate::ocr::OcrRequest,
        ) -> Result<Vec<crate::dto::Word>, Box<dyn std::error::Error>> {
            if request.segmentation == crate::ocr::OcrSegmentation::Sparse {
                Ok(Vec::new())
            } else {
                self.recognize_words(image)
            }
        }

        fn recognize_technical_text_with_request(
            &self,
            _: &GrayImage,
            _: crate::ocr::OcrRequest,
        ) -> Result<Option<crate::ocr::OcrTextResult>, Box<dyn std::error::Error>> {
            if self.technical_init_failure {
                return Err(Box::new(CellIsolationError::Initialization));
            }
            Ok(Some(crate::ocr::OcrTextResult {
                text: "Recovered".into(),
                confidence: Some(0.91),
            }))
        }
    }

    fn cell_isolation_image() -> GrayImage {
        let mut image = GrayImage::from_pixel(300, 300, Luma([255]));
        for &y in &[30, 150, 270] {
            for x in 20..280 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        for &x in &[30, 150, 270] {
            for y in 20..280 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        for &(x, y) in &[(60, 65), (180, 65), (60, 185), (180, 185)] {
            for dx in 0..22 {
                for dy in 0..18 {
                    if !(3..=18).contains(&dx) || (7..10).contains(&dy) {
                        image.put_pixel(x + dx, y + dy, Luma([0]));
                    }
                }
            }
        }
        image
    }

    fn cell_isolation_prepared() -> super::PreparedPage {
        super::PreparedPage {
            page_number: 7,
            page_started: Instant::now(),
            language_profile: crate::ocr::OcrLanguageProfile::Configured,
            effective_languages: "rus+eng".into(),
            native_text_quality: None,
            native_text_layout: None,
            timings: PageStageTimings::default(),
            warnings: Vec::new(),
            work: None,
        }
    }

    fn cell_isolation_finish(
        native: bool,
        engine: &CellIsolationOcr,
    ) -> Result<super::ProcessedPage, String> {
        let image = cell_isolation_image();
        let config = super::WorkerConfig {
            max_pixels: DEFAULT_MAX_PIXELS,
            table_mode: TableMode::Grid,
            languages: "rus+eng".into(),
            enable_cell_ocr_fallback: true,
        };
        if native {
            super::finish_prepared_native_page(
                cell_isolation_prepared(),
                NativeTextLayer::new("Canonical complete page text 123".into(), 32),
                Default::default(),
                Some(super::PreparedGridRender {
                    rendered: RenderedPage::new(7, image.clone(), 300.0),
                    native_words: engine.recognize_words(&image).unwrap(),
                    grid_candidates: vec![grid_fixture(
                        BoundingBox::new(30.0, 30.0, 270.0, 270.0),
                        2,
                        2,
                        0.95,
                    )],
                    cell_ocr_regions: vec![None],
                }),
                engine,
                &config,
            )
        } else {
            super::finish_prepared_ocr_page(
                cell_isolation_prepared(),
                image,
                300.0,
                None,
                None,
                Some(Default::default()),
                engine,
                &config,
            )
        }
    }

    fn assert_cell_ocr_isolation_page(native: bool) {
        let engine = CellIsolationOcr::new(false, false);
        let image = cell_isolation_image();
        let expected_grid = if native {
            grid_fixture(BoundingBox::new(30.0, 30.0, 270.0, 270.0), 2, 2, 0.95)
        } else {
            super::detect_table_grids_safely(&image)
                .unwrap()
                .into_iter()
                .next()
                .expect("fixture has a ruled table")
        };
        let result = cell_isolation_finish(native, &engine)
            .expect("a recoverable cell error must not discard the page");
        assert_eq!(result.page.text, "Canonical complete page text 123");
        assert_eq!(result.page.tables.len(), 1);
        let cells = &result.page.tables[0].rows;
        assert_eq!(cells[0][0].text, "Original");
        assert_eq!(cells[0][0].confidence, 0.83);
        assert_eq!(cells[0][1].text, "");
        assert_eq!(cells[0][1].confidence, 0.0);
        assert_eq!(cells[1][0].text, "Recovered", "other cells must continue");
        assert_eq!(cells[1][1].text, "Recovered");
        for cell in cells.iter().flatten() {
            let original = expected_grid
                .cells
                .iter()
                .find(|original| original.row == cell.row && original.column == cell.column)
                .unwrap();
            assert_eq!(
                cell.bbox, original.bbox,
                "physical bbox must remain unchanged"
            );
            assert_eq!(
                (cell.row_span, cell.col_span),
                (original.row_span, original.col_span)
            );
        }
        assert_eq!(
            result.warnings,
            vec![PipelineWarning::CellOcrFallbackFailed {
                page_number: 7,
                table_index: 0,
                row: 0,
                column: 1,
                reason_code: "recognition_failed".into(),
            }]
        );
        assert!(!format!("{:?}", result.warnings).contains("private cell text"));
        assert_eq!(engine.cell_calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn cell_ocr_isolation_preserves_native_page_after_recoverable_error() {
        assert_cell_ocr_isolation_page(true);
    }

    #[test]
    fn cell_ocr_isolation_preserves_raster_page_after_recoverable_error() {
        assert_cell_ocr_isolation_page(false);
    }

    #[test]
    fn cell_ocr_failure_does_not_abort_document() {
        let mut source = StubPageSource::new(vec![cell_isolation_image(), cell_isolation_image()])
            .with_dpi(300.0);
        let engine = CellIsolationOcr::new(false, false);
        let outcome = Pipeline::new(&mut source, &engine, DEFAULT_MAX_PIXELS, None)
            .with_table_mode(TableMode::Grid)
            .run_with_warnings()
            .expect("a local cell error must not abort the document or later pages");
        assert_eq!(outcome.document.pages.len(), 2);
        assert_eq!(outcome.page_processing.len(), 2);
        for (index, page) in outcome.document.pages.iter().enumerate() {
            assert_eq!(page.page_number, index as u32 + 1);
            assert_eq!(page.text, "Canonical complete page text 123");
            assert_eq!(page.tables.len(), 1);
            assert_eq!(page.tables[0].rows[0][0].text, "Original");
            assert_eq!(page.tables[0].rows[1][0].text, "Recovered");
            assert_eq!(page.tables[0].rows[1][1].text, "Recovered");
        }
        assert_eq!(outcome.document.pages[0].tables[0].rows[0][1].text, "");
        assert_eq!(
            outcome.document.pages[1].tables[0].rows[0][1].text,
            "Recovered"
        );
        assert_eq!(
            outcome.warnings,
            vec![PipelineWarning::CellOcrFallbackFailed {
                page_number: 1,
                table_index: 0,
                row: 0,
                column: 1,
                reason_code: "recognition_failed".into(),
            }]
        );
        assert_eq!(engine.cell_calls.load(Ordering::SeqCst), 6);
    }

    #[test]
    fn cell_ocr_isolation_keeps_initialization_and_unknown_errors_fatal() {
        for native in [true, false] {
            let error = cell_isolation_finish(native, &CellIsolationOcr::new(true, false))
                .err()
                .expect("initialization errors must still fail the page");
            assert!(error.contains("fatal initialization failure"));
        }
        let mut failures = Vec::new();
        let result = super::optional_cell_ocr_result::<(), _>(
            Err("unclassified system failure".into()),
            &StubOcrEngine::new(),
            &TableCell::default(),
            &mut failures,
        );
        assert!(result.is_err());
        assert!(failures.is_empty());
        let engine = CellIsolationOcr::new(false, false);
        let routed = crate::ocr::LanguageRoutedOcrEngine::new(
            &engine,
            crate::ocr::OcrLanguageProfile::Configured,
        )
        .unwrap();
        assert_eq!(
            routed.recoverable_cell_error_reason(&CellIsolationError::Recognition),
            Some("recognition_failed")
        );
        assert_eq!(
            routed.recoverable_cell_error_reason(&CellIsolationError::Initialization),
            None
        );
    }

    fn cell_isolation_technical_fixture() -> (TableGrid, GrayImage) {
        let mut grid = grid_fixture(BoundingBox::new(20.0, 20.0, 280.0, 280.0), 3, 4, 0.95);
        for cell in &mut grid.cells {
            cell.text = if cell.row == 0 {
                ["No.", "Name", "Model", "Quantity"][cell.column as usize].into()
            } else {
                ["1", "Motor Cable", "M16X200", "10"][cell.column as usize].into()
            };
            cell.confidence = 0.6;
        }
        let classification = crate::table_semantics::classify_table(&grid.to_matrix(), None);
        assert!(
            classification.column_roles.iter().any(|role| {
                role.column == 2
                    && role.confidence >= super::TECHNICAL_ROLE_MIN_CONFIDENCE
                    && matches!(role.role, crate::table_semantics::ColumnRole::Designation)
            }),
            "fixture must select the existing technical recheck role"
        );
        let mut image = GrayImage::from_pixel(300, 300, Luma([255]));
        for cell in &grid.cells {
            let bbox = cell.bbox.as_ref().unwrap();
            for x in (bbox.x0 as u32 + 10)..(bbox.x0 as u32 + 25) {
                for y in (bbox.y0 as u32 + 15)..(bbox.y0 as u32 + 30) {
                    if x % 6 < 3 {
                        image.put_pixel(x, y, Luma([0]));
                    }
                }
            }
        }
        (grid, image)
    }

    #[test]
    fn cell_ocr_isolation_restores_nonempty_cell_after_later_technical_pass_error() {
        let (mut grid, image) = cell_isolation_technical_fixture();
        let before = grid.cells.clone();
        let outcome = super::fill_empty_grid_cells_with_ocr_diagnostics(
            &mut grid,
            &image,
            &CellIsolationOcr::new(false, true),
            DEFAULT_MAX_PIXELS,
            300,
            &mut PageStageTimings::default(),
        )
        .unwrap();
        assert!(
            !outcome.failures.is_empty(),
            "fixture must exercise the technical pass"
        );
        for failure in outcome.failures {
            let index = grid
                .cells
                .iter()
                .position(|cell| failure.matches(cell))
                .unwrap();
            assert_eq!(
                grid.cells[index], before[index],
                "restore text, confidence, spans and bbox after the later error"
            );
        }
    }

    #[test]
    fn cell_ocr_isolation_optional_technical_init_does_not_hide_required_generic_init() {
        let (mut grid, image) = cell_isolation_technical_fixture();
        let mut engine = CellIsolationOcr::new(false, true);
        engine.technical_init_failure = true;
        let outcome = super::fill_empty_grid_cells_with_ocr_diagnostics(
            &mut grid,
            &image,
            &engine,
            DEFAULT_MAX_PIXELS,
            300,
            &mut PageStageTimings::default(),
        )
        .expect("optional technical model failure retains the established fallback");
        assert!(outcome.failures.is_empty());
        assert_eq!(engine.cell_calls.load(Ordering::SeqCst), 0);
        let (mut grid, image) = cell_isolation_technical_fixture();
        let error = super::fill_empty_grid_cells_with_ocr_diagnostics(
            &mut grid,
            &image,
            &CellIsolationOcr::new(true, true),
            DEFAULT_MAX_PIXELS,
            300,
            &mut PageStageTimings::default(),
        )
        .err()
        .expect("required generic OCR initialization must still fail");
        assert!(error.to_string().contains("fatal initialization failure"));
    }

    #[test]
    fn cell_ocr_isolation_rolls_back_primary_success_after_numeric_pass_error() {
        let mut grid = grid_fixture(BoundingBox::new(0.0, 0.0, 900.0, 360.0), 4, 9, 0.95);
        for cell in &mut grid.cells {
            cell.text = if cell.row == 0 {
                (cell.column + 1).to_string()
            } else {
                "source".into()
            };
            cell.confidence = 1.0;
        }
        let target = grid
            .cells
            .iter()
            .position(|cell| cell.row == 1 && cell.column == 0)
            .unwrap();
        grid.cells[target].text = "  ".into();
        grid.cells[target].confidence = 0.27;
        let before = grid.cells.clone();
        assert_eq!(
            numbered_engineering_specification_header_row(&grid),
            Some(0)
        );
        let mut image = GrayImage::from_pixel(900, 360, Luma([255]));
        for x in 30..55 {
            for y in 120..140 {
                if x % 8 < 3 {
                    image.put_pixel(x, y, Luma([0]));
                }
            }
        }
        let mut engine = CellIsolationOcr::new(false, true);
        engine.fail_numeric = true;
        let outcome = super::fill_empty_grid_cells_with_ocr_diagnostics(
            &mut grid,
            &image,
            &engine,
            DEFAULT_MAX_PIXELS,
            300,
            &mut PageStageTimings::default(),
        )
        .unwrap();
        assert_eq!(
            engine.cell_calls.load(Ordering::SeqCst),
            2,
            "primary succeeds before numeric request fails"
        );
        assert_eq!(outcome.failures.len(), 1);
        assert!(outcome.failures[0].matches(&before[target]));
        assert_eq!(
            grid.cells, before,
            "every cell field returns to its pre-fallback value"
        );
        assert!(outcome.confidences.is_empty());
    }

    #[test]
    fn short_native_candidate_wins_against_low_confidence_noise() {
        let native = NativeTextLayer::new("Drawing title 84".to_string(), 16);
        let noisy = "random OCR output that is much longer but unreliable ".repeat(3);
        let (selected, used_native) =
            super::choose_canonical_page_text(Some(&native), &noisy, Some(0.54), false);
        assert!(used_native);
        assert_eq!(selected, native.text);
    }

    #[test]
    fn confident_materially_larger_ocr_replaces_partial_native_label() {
        let native = NativeTextLayer::new("Partial label 39".to_string(), 16);
        let complete = "Complete raster table row with equipment values and dimensions ".repeat(4);
        let (selected, used_native) =
            super::choose_canonical_page_text(Some(&native), &complete, Some(0.88), true);
        assert!(!used_native);
        assert_eq!(selected, complete);
    }

    #[test]
    fn near_usable_native_enables_non_grid_ocr_arbitration() {
        let quality = crate::page_analysis::NativeTextQuality {
            is_near_usable: true,
            ..Default::default()
        };
        assert!(super::near_usable_native_requires_arbitration(Some(
            &quality
        )));
        assert!(!super::near_usable_native_requires_arbitration(None));

        let native = NativeTextLayer::new("Partial label 39".to_string(), 16);
        let complete = "Complete page text with equipment values and dimensions ".repeat(4);
        let (selected, used_native) = super::choose_canonical_page_text(
            Some(&native),
            &complete,
            Some(0.88),
            super::near_usable_native_requires_arbitration(Some(&quality)),
        );
        assert!(!used_native);
        assert_eq!(selected, complete);
    }

    #[test]
    fn near_equal_ocr_does_not_replace_searchable_text_on_artifact_alone() {
        let native = NativeTextLayer::new(
            "Specification equipment pump motor valve panel cable terminal � quantity twelve"
                .to_string(),
            78,
        );
        let repaired =
            "Specification equipment pump motor valve panel cable terminal box quantity twelve";
        let (selected, used_native) =
            super::choose_canonical_page_text(Some(&native), repaired, Some(0.91), true);
        assert!(used_native);
        assert_eq!(selected, native.text);
    }

    #[test]
    fn multiscale_grids_are_scaled_and_deduplicated() {
        let thumbnail_grid = TableGrid {
            bbox: BoundingBox::new(10.0, 20.0, 30.0, 40.0),
            horizontal_lines: vec![GridLine {
                position: 20.0,
                start: 10.0,
                end: 30.0,
            }],
            vertical_lines: vec![GridLine {
                position: 10.0,
                start: 20.0,
                end: 40.0,
            }],
            intersections: vec![GridIntersection { x: 10.0, y: 20.0 }],
            cells: vec![TableCell {
                bbox: Some(BoundingBox::new(10.0, 20.0, 30.0, 40.0)),
                ..TableCell::default()
            }],
            confidence: 0.8,
        };
        let scaled = super::scale_table_grids(vec![thumbnail_grid], 100, 100, 200, 300);
        assert_eq!(scaled[0].bbox, BoundingBox::new(20.0, 60.0, 60.0, 120.0));
        assert_eq!(scaled[0].horizontal_lines[0].position, 60.0);
        assert_eq!(scaled[0].vertical_lines[0].position, 20.0);
        assert_eq!(scaled[0].intersections[0].x, 20.0);
        assert_eq!(scaled[0].intersections[0].y, 60.0);

        let mut primary = vec![TableGrid {
            confidence: 0.9,
            ..scaled[0].clone()
        }];
        let disjoint = TableGrid {
            bbox: BoundingBox::new(120.0, 30.0, 180.0, 90.0),
            confidence: 0.7,
            ..scaled[0].clone()
        };
        super::merge_multiscale_table_grids(&mut primary, vec![scaled[0].clone(), disjoint]);
        assert_eq!(primary.len(), 2);
        assert_eq!(primary[1].confidence, 0.9);
    }

    #[test]
    fn multiscale_merge_prefers_richer_incompatible_topology_over_confidence() {
        let bbox = BoundingBox::new(10.0, 20.0, 210.0, 220.0);
        let richer = grid_fixture(bbox.clone(), 3, 4, 0.61);
        let sparse_high_confidence = grid_fixture(bbox.clone(), 2, 2, 0.99);

        let mut primary = vec![richer.clone()];
        super::merge_multiscale_table_grids(&mut primary, vec![sparse_high_confidence.clone()]);
        assert_eq!(primary.len(), 1);
        assert_eq!(primary[0].rows(), 3);
        assert_eq!(primary[0].columns(), 4);
        assert_eq!(primary[0].confidence, richer.confidence);

        let mut primary = vec![sparse_high_confidence];
        super::merge_multiscale_table_grids(&mut primary, vec![richer]);
        assert_eq!(primary.len(), 1);
        assert_eq!(primary[0].rows(), 3);
        assert_eq!(primary[0].columns(), 4);
        assert_eq!(primary[0].confidence, 0.61);
    }

    #[test]
    fn multiscale_merge_deduplicates_bbox_jitter_for_matching_topology() {
        let mut primary = vec![grid_fixture(
            BoundingBox::new(10.0, 20.0, 210.0, 220.0),
            3,
            4,
            0.71,
        )];
        let jittered = grid_fixture(BoundingBox::new(12.0, 18.0, 212.0, 218.0), 3, 4, 0.83);

        super::merge_multiscale_table_grids(&mut primary, vec![jittered.clone()]);
        assert_eq!(primary.len(), 1);
        assert_eq!(primary[0].bbox, jittered.bbox);
        assert_eq!(primary[0].confidence, 0.83);
    }

    #[test]
    fn ocr_grid_thumbnail_uses_bounded_thumbnail_semantics() {
        struct OversizedThumbnailSource {
            requested_max_dimension: Arc<AtomicUsize>,
        }

        impl PageSource for OversizedThumbnailSource {
            fn total_pages(&self) -> Result<u32, Box<dyn std::error::Error>> {
                Ok(1)
            }

            fn render_page(
                &mut self,
                _page_num: u32,
            ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
                panic!("bounded thumbnail helper must not call render_page")
            }

            fn render_table_thumbnail(
                &mut self,
                page_num: u32,
                max_dimension: u32,
            ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
                self.requested_max_dimension
                    .store(max_dimension as usize, Ordering::SeqCst);
                // Deliberately violate the requested dimensions to prove that
                // the pipeline defensively re-bounds custom source output.
                Ok(RenderedPage::new(
                    page_num,
                    GrayImage::from_pixel(200, 200, Luma([127])),
                    100.0,
                ))
            }
        }

        let requested = Arc::new(AtomicUsize::new(0));
        let mut source = OversizedThumbnailSource {
            requested_max_dimension: Arc::clone(&requested),
        };
        let full_render = RenderedPage::new(1, GrayImage::from_pixel(400, 200, Luma([255])), 400.0);
        let thumbnail =
            super::render_bounded_grid_thumbnail(&mut source, 1, &full_render, 100, 1_000)
                .expect("bounded thumbnail");

        let requested = requested.load(Ordering::SeqCst);
        assert!(requested > 0 && requested <= 100);
        assert!(
            usize::try_from(thumbnail.image.width()).unwrap()
                * usize::try_from(thumbnail.image.height()).unwrap()
                <= 1_000
        );
    }

    #[test]
    fn grid_detector_failure_does_not_fail_document_ocr() {
        let mut source = StubPageSource::single(100, 100, 255);
        let ocr = StubOcrEngine::new();
        let (page, _processing, warnings) = process_one(
            &mut source,
            &ocr,
            DEFAULT_MAX_PIXELS,
            1,
            TableMode::Grid,
            "rus+eng",
            150,
            false,
            None,
            1,
            |_| Err("forced detector failure".to_string()),
        )
        .expect("OCR should continue when the grid detector fails");

        assert_eq!(page.kind, PageKind::Text);
        assert!(!page.text.is_empty());
        assert_eq!(
            warnings,
            vec![PipelineWarning::GridDetectorFailed {
                page_number: 1,
                message: "forced detector failure".to_string(),
            }]
        );
    }

    #[test]
    fn legacy_mode_still_uses_word_alignment_fallback() {
        let mut image = GrayImage::from_pixel(100, 100, Luma([255]));
        for &y in &[10, 50, 90] {
            for x in 0..100 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        for &x in &[10, 50, 90] {
            for y in 0..100 {
                image.put_pixel(x, y, Luma([0]));
            }
        }

        let mut source = StubPageSource::new(vec![image]);
        let ocr = StubOcrEngine::new();
        let outcome = Pipeline::new(&mut source, &ocr, DEFAULT_MAX_PIXELS, None)
            .with_table_mode(TableMode::Legacy)
            .run_with_warnings()
            .expect("legacy pipeline should run");

        assert_eq!(outcome.document.pages[0].kind, PageKind::Table);
        assert!(outcome.warnings.is_empty());
    }

    struct NativeTextStubSource {
        bitmap: GrayImage,
        layer: NativeTextLayer,
        metadata_angle: u16,
    }

    impl PageSource for NativeTextStubSource {
        fn total_pages(&self) -> Result<u32, Box<dyn std::error::Error>> {
            Ok(1)
        }

        fn render_page(
            &mut self,
            page_num: u32,
        ) -> Result<RenderedPage, Box<dyn std::error::Error>> {
            Ok(RenderedPage::new(page_num, self.bitmap.clone(), 300.0))
        }

        fn native_text_for_page(
            &self,
            _page_num: u32,
        ) -> Result<Option<NativeTextLayer>, Box<dyn std::error::Error>> {
            Ok(Some(self.layer.clone()))
        }

        fn page_orientation_from_metadata(
            &self,
            _page_num: u32,
        ) -> Result<crate::page_analysis::PageOrientation, Box<dyn std::error::Error>> {
            Ok(crate::page_analysis::PageOrientation {
                angle: self.metadata_angle,
                confidence: 1.0,
            })
        }

        fn native_words_for_rendered_page(
            &self,
            _page_num: u32,
            width: u32,
            height: u32,
        ) -> Result<Option<Vec<crate::dto::Word>>, Box<dyn std::error::Error>> {
            Ok(Some(vec![crate::dto::Word::new(
                "cell",
                crate::dto::BoundingBox::new(
                    f64::from(width) * 0.15,
                    f64::from(height) * 0.15,
                    f64::from(width) * 0.25,
                    f64::from(height) * 0.25,
                ),
                1.0,
            )]))
        }
    }

    struct CountingOcr {
        calls: AtomicUsize,
    }

    impl CountingOcr {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl OcrEngine for CountingOcr {
        fn recognize_text(&self, _image: &GrayImage) -> Result<String, Box<dyn std::error::Error>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok("unexpected OCR".to_string())
        }

        fn recognize_words(
            &self,
            _image: &GrayImage,
        ) -> Result<Vec<crate::dto::Word>, Box<dyn std::error::Error>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![crate::dto::Word::new(
                "cell",
                crate::dto::BoundingBox::new(40.0, 40.0, 60.0, 60.0),
                0.9,
            )])
        }
    }

    #[test]
    fn quality_native_text_uses_grid_preflight_but_bypasses_tesseract_without_a_grid() {
        let layer = NativeTextLayer::new(
            "Нативный текст Native text 123 для проверяемой PDF страницы.".to_string(),
            56,
        );
        let mut source = NativeTextStubSource {
            bitmap: GrayImage::from_pixel(1, 1, Luma([255])),
            layer,
            metadata_angle: 180,
        };
        let ocr = CountingOcr::new();
        let outcome = Pipeline::new(&mut source, &ocr, DEFAULT_MAX_PIXELS, None)
            .with_languages("rus+eng")
            .run_with_warnings()
            .expect("native text pipeline should succeed");

        assert_eq!(outcome.document.pages[0].kind, PageKind::Text);
        assert_eq!(
            outcome.page_processing[0].processing_mode,
            ProcessingMode::NativeText
        );
        assert!(outcome.page_processing[0]
            .native_text_quality
            .as_ref()
            .is_some_and(|quality| quality.is_usable));
        assert_eq!(outcome.page_processing[0].orientation.angle, 180);
        assert_eq!(ocr.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn quality_native_text_with_a_ruled_grid_uses_native_coordinates() {
        let mut image = GrayImage::from_pixel(300, 300, Luma([255]));
        for &y in &[30, 150, 270] {
            for x in 20..280 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        for &x in &[30, 150, 270] {
            for y in 20..280 {
                image.put_pixel(x, y, Luma([0]));
            }
        }

        let expected_native_text =
            "Searchable native text exists, but the ruled grid must use OCR layout.";
        let layer = NativeTextLayer::new(expected_native_text.to_string(), 70);
        let mut source = NativeTextStubSource {
            bitmap: image,
            layer,
            metadata_angle: 270,
        };
        let ocr = CountingOcr::new();
        let outcome = Pipeline::new(&mut source, &ocr, DEFAULT_MAX_PIXELS, None)
            .with_table_mode(TableMode::GridWithFallback)
            .with_languages("rus+eng")
            .with_cell_ocr_fallback(false)
            .run_with_warnings()
            .expect("grid-bearing native text page should use grid OCR");

        assert_eq!(outcome.document.pages[0].kind, PageKind::Mixed);
        assert_eq!(outcome.document.pages[0].text, expected_native_text);
        assert!(!outcome.document.pages[0].tables.is_empty());
        assert_eq!(
            outcome.page_processing[0].processing_mode,
            ProcessingMode::NativeTextWithGrid
        );
        assert_eq!(outcome.page_processing[0].orientation.angle, 270);
        assert!(outcome.page_processing[0].render_size.is_some());
        let assignment = outcome.page_processing[0]
            .word_assignment
            .expect("grid route should record word assignment");
        assert_eq!(assignment.recognized_words, 1);
        assert_eq!(assignment.assigned_words, 1);
        assert_eq!(assignment.words_outside_tables, 0);
        assert_eq!(outcome.page_processing[0].timings.full_render_ms, 0.0);
        assert_eq!(outcome.page_processing[0].timings.tesseract_ms, 0.0);
        assert_eq!(ocr.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn empty_low_resolution_grid_preserves_complete_native_text() {
        let mut image = GrayImage::from_pixel(300, 300, Luma([255]));
        for &y in &[30, 150, 270] {
            for x in 20..280 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        for &x in &[30, 150, 270] {
            for y in 20..280 {
                image.put_pixel(x, y, Luma([0]));
            }
        }

        let expected = "Complete native text survives a thumbnail grid false positive 123.";
        let mut source = NativeTextStubSource {
            bitmap: image,
            layer: NativeTextLayer::new(expected.to_string(), expected.chars().count()),
            metadata_angle: 0,
        };
        let ocr = CountingOcr::new();
        let detector_calls = AtomicUsize::new(0);
        let (page, processing, warnings) = process_one(
            &mut source,
            &ocr,
            DEFAULT_MAX_PIXELS,
            1,
            TableMode::GridWithFallback,
            "rus+eng",
            150,
            false,
            None,
            1,
            |_image| {
                detector_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Vec::new())
            },
        )
        .expect("empty low-resolution grid detection should preserve native text");

        assert_eq!(detector_calls.load(Ordering::SeqCst), 1);
        assert_eq!(ocr.calls.load(Ordering::SeqCst), 0);
        assert_eq!(page.kind, PageKind::Text);
        assert_eq!(page.text, expected);
        assert!(page.tables.is_empty());
        assert_eq!(processing.processing_mode, ProcessingMode::NativeText);
        assert!(warnings.is_empty());
    }

    #[test]
    fn raster_page_records_ocr_processing_and_render_size() {
        let mut source = StubPageSource::single(80, 40, 255).with_dpi(300.0);
        let ocr = StubOcrEngine::new();
        let outcome = Pipeline::new(&mut source, &ocr, DEFAULT_MAX_PIXELS, None)
            .with_table_mode(TableMode::Off)
            .with_languages("rus+eng")
            .run_with_warnings()
            .expect("raster pipeline should succeed");
        let processing = &outcome.page_processing[0];
        assert_eq!(processing.processing_mode, ProcessingMode::Ocr);
        assert_eq!(processing.language, "rus+eng");
        assert_eq!(processing.render_size.expect("render size").dpi, 300);
    }

    struct ReorderingOcr;

    impl OcrEngine for ReorderingOcr {
        fn recognize_text(&self, image: &GrayImage) -> Result<String, Box<dyn std::error::Error>> {
            // Make completion order differ predictably from source order.
            let delay_ms = match image.width() {
                40 => 30,
                20 => 10,
                _ => 0,
            };
            thread::sleep(Duration::from_millis(delay_ms));
            Ok(format!("width={}", image.width()))
        }

        fn recognize_words(
            &self,
            _image: &GrayImage,
        ) -> Result<Vec<crate::dto::Word>, Box<dyn std::error::Error>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn bounded_parallel_pipeline_preserves_source_page_order() {
        let pages = vec![
            GrayImage::from_pixel(40, 10, Luma([255])),
            GrayImage::from_pixel(10, 10, Luma([255])),
            GrayImage::from_pixel(20, 10, Luma([255])),
        ];
        let mut source = StubPageSource::new(pages);
        let outcome = Pipeline::new(&mut source, &ReorderingOcr, DEFAULT_MAX_PIXELS, None)
            .with_table_mode(TableMode::Off)
            .run_with_warnings_parallel(3)
            .expect("parallel pipeline should complete");

        assert_eq!(
            outcome
                .document
                .pages
                .iter()
                .map(|page| page.page_number)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(outcome.document.pages[0].text, "width=40");
        assert_eq!(outcome.document.pages[1].text, "width=10");
        assert_eq!(outcome.document.pages[2].text, "width=20");
        assert_eq!(
            outcome
                .page_processing
                .iter()
                .map(|processing| processing.page_number)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    struct OrientationOcr;

    impl OcrEngine for OrientationOcr {
        fn recognize_text(&self, _image: &GrayImage) -> Result<String, Box<dyn std::error::Error>> {
            Ok("rotated fixture".to_string())
        }

        fn recognize_words(
            &self,
            image: &GrayImage,
        ) -> Result<Vec<crate::dto::Word>, Box<dyn std::error::Error>> {
            // The marker moves into the upper-right corner only for the 90°
            // candidate. This makes the mock emulate an OCR engine that sees
            // readable text only at that orientation.
            let marker_is_upper_right = image.width() == 20
                && image.height() == 40
                && image.get_pixel(image.width() - 1, 0)[0] == 0;
            if !marker_is_upper_right {
                return Ok(Vec::new());
            }
            Ok((0..6)
                .map(|index| {
                    crate::dto::Word::new(
                        format!("word{index}"),
                        crate::dto::BoundingBox::new(1.0, 1.0, 2.0, 2.0),
                        0.9,
                    )
                })
                .collect())
        }
    }

    #[test]
    fn rotated_landscape_page_is_oriented_before_ocr() {
        let mut landscape = GrayImage::from_pixel(40, 20, Luma([255]));
        landscape.put_pixel(0, 0, Luma([0]));
        let mut source = StubPageSource::new(vec![landscape]).with_dpi(200.0);
        let outcome = Pipeline::new(&mut source, &OrientationOcr, DEFAULT_MAX_PIXELS, None)
            .with_table_mode(TableMode::Off)
            .run_with_warnings()
            .expect("rotated fixture should process");
        let processing = &outcome.page_processing[0];
        assert_eq!(processing.orientation.angle, 90);
        assert!(processing.orientation.confidence > 0.8);
        let size = processing.render_size.expect("render size");
        assert_eq!((size.width, size.height), (20, 40));
    }
    #[test]
    fn cell_ocr_fallback_skips_too_narrow_crops() {
        let mut image = GrayImage::from_pixel(12, 48, Luma([255]));
        for y in 2..46 {
            for x in 2..4 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        let bbox = BoundingBox::new(0.0, 0.0, 6.0, 48.0);

        assert!(informative_cell_crop(&image, &bbox).is_none());
    }

    #[test]
    fn cell_ocr_crop_rejects_grid_rules_without_text() {
        let mut image = GrayImage::from_pixel(100, 40, Luma([255]));
        for x in 0..100 {
            for y in [0, 1, 38, 39] {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        for y in 0..40 {
            for x in [0, 1, 98, 99] {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        let bbox = BoundingBox::new(0.0, 0.0, 100.0, 40.0);

        assert!(
            prepare_cell_ocr_crop(&image, &bbox, DEFAULT_MAX_PIXELS, 100)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn cell_ocr_crop_adapts_to_small_foreground_height() {
        let mut image = GrayImage::from_pixel(100, 40, Luma([255]));
        // Three compact synthetic glyphs, seven pixels high, surrounded by a
        // table border. Their exact shape is irrelevant to this pure crop
        // test; the foreground must survive while the border disappears.
        for base_x in [35, 43, 51] {
            for y in 16..23 {
                image.put_pixel(base_x, y, Luma([0]));
                image.put_pixel(base_x + 3, y, Luma([0]));
            }
            for x in base_x..=base_x + 3 {
                image.put_pixel(x, 16, Luma([0]));
                image.put_pixel(x, 22, Luma([0]));
            }
        }
        for x in 0..100 {
            image.put_pixel(x, 0, Luma([0]));
            image.put_pixel(x, 39, Luma([0]));
        }
        let bbox = BoundingBox::new(0.0, 0.0, 100.0, 40.0);

        let prepared = prepare_cell_ocr_crop(&image, &bbox, DEFAULT_MAX_PIXELS, 100)
            .unwrap()
            .expect("synthetic glyphs should be retained");
        assert_eq!(prepared.effective_dpi, 400);
        assert!(prepared.image.width() >= 100);
        assert!(prepared.image.height() >= 40);
        assert!(prepared.image.pixels().any(|pixel| pixel[0] < 128));
        assert_eq!(prepared.image.get_pixel(0, 0)[0], 255);
    }

    #[test]
    fn cell_ocr_candidate_gate_rejects_low_confidence_grid_glyphs_only() {
        assert!(!accept_cell_ocr_candidate("и", Some(0.60)));
        assert!(!accept_cell_ocr_candidate("ИИ", Some(0.20)));
        assert!(accept_cell_ocr_candidate("и", Some(0.90)));
        assert!(accept_cell_ocr_candidate("м", Some(0.40)));
        assert!(accept_cell_ocr_candidate("DKC", Some(0.40)));
        assert!(accept_cell_ocr_candidate("IP67", Some(0.30)));
        assert!(accept_cell_ocr_candidate("fixture", None));
        assert!(!accept_cell_ocr_candidate("|", Some(0.99)));
        assert!(!accept_cell_ocr_candidate("│", None));
    }

    #[test]
    fn numbered_nine_column_header_enables_only_the_bounded_specification_pass() {
        let mut grid = grid_fixture(BoundingBox::new(0.0, 0.0, 900.0, 160.0), 4, 9, 1.0);
        for cell in &mut grid.cells {
            if cell.row == 1 && cell.column != 1 {
                cell.text = (cell.column + 1).to_string();
            }
        }
        assert_eq!(
            numbered_engineering_specification_header_row(&grid),
            Some(1)
        );

        grid.cells
            .iter_mut()
            .find(|cell| cell.row == 1 && cell.column == 8)
            .unwrap()
            .text = "x".to_string();
        grid.cells
            .iter_mut()
            .find(|cell| cell.row == 1 && cell.column == 7)
            .unwrap()
            .text = "x".to_string();
        assert_eq!(numbered_engineering_specification_header_row(&grid), None);

        let nonstandard = grid_fixture(BoundingBox::new(0.0, 0.0, 800.0, 160.0), 4, 8, 1.0);
        assert_eq!(
            numbered_engineering_specification_header_row(&nonstandard),
            None
        );
    }

    #[test]
    fn english_manufacturer_candidate_requires_strong_compact_evidence() {
        assert!(prefer_english_manufacturer_candidate(
            "ОКЕ",
            0.72,
            "DKC",
            Some(0.86)
        ));
        assert!(prefer_english_manufacturer_candidate(
            "OKC",
            0.75,
            "DKC",
            Some(0.86)
        ));
        assert!(!prefer_english_manufacturer_candidate(
            "ОКС",
            0.92,
            "DKC",
            Some(0.86)
        ));
        assert!(!prefer_english_manufacturer_candidate(
            "ООО Конкорд",
            0.88,
            "OOO KOHKOPD",
            Some(0.91)
        ));
        assert!(!prefer_english_manufacturer_candidate(
            "ОКЕ",
            0.72,
            "DKC",
            Some(0.69)
        ));
    }

    #[test]
    fn numeric_cell_selector_requires_bounded_cross_profile_evidence() {
        assert_eq!(
            choose_numeric_cell_candidate(
                "5. 14",
                0.80,
                "5.14",
                Some(0.88),
                "5.14",
                Some(0.91),
                NumericCellRole::Position,
            ),
            Some(("5.14".to_string(), 0.91))
        );
        assert_eq!(
            choose_numeric_cell_candidate(
                "1?",
                0.72,
                "12",
                Some(0.94),
                "12",
                Some(0.61),
                NumericCellRole::Quantity,
            ),
            Some(("12".to_string(), 0.94))
        );
        assert_eq!(
            choose_numeric_cell_candidate(
                "#3",
                0.62,
                "3.3",
                Some(0.82),
                "3",
                Some(0.30),
                NumericCellRole::Position,
            ),
            Some(("3.3".to_string(), 0.82))
        );

        // A valid primary is not replaced by a different candidate, even
        // when the candidate would make a neighbouring sequence look nicer.
        assert_eq!(
            choose_numeric_cell_candidate(
                "5.75",
                0.95,
                "5.25",
                Some(0.88),
                "5.75",
                Some(0.95),
                NumericCellRole::Position,
            ),
            None
        );
        // Two credible but conflicting candidates are deliberately left for
        // review rather than guessed from row order.
        assert_eq!(
            choose_numeric_cell_candidate(
                "BS",
                0.69,
                "3.5",
                Some(0.90),
                "315",
                Some(0.62),
                NumericCellRole::Position,
            ),
            None
        );
    }

    #[test]
    fn numeric_cell_grammar_rejects_identifier_homoglyphs_and_formulas() {
        assert!(valid_numeric_cell("5.14", NumericCellRole::Position));
        assert!(valid_numeric_cell("12,5", NumericCellRole::Quantity));
        assert!(!valid_numeric_cell("О.14", NumericCellRole::Position));
        assert!(!valid_numeric_cell("1/2", NumericCellRole::Quantity));
        assert!(!valid_numeric_cell("1.2.3", NumericCellRole::Quantity));
        assert!(!valid_numeric_cell("+3", NumericCellRole::Quantity));
    }

    #[test]
    fn technical_cell_selector_requires_exact_independent_consensus() {
        let cases = [
            ("VVR3*254+2*10", 0.62, "VVR3*25+2*10"),
            ("VVR 42.5", 0.48, "VVR 4*2.5"),
            ("увы", 0.24, "RVV18*1.0"),
            ("16x20", 0.24, "M16X200"),
        ];
        for (primary, confidence, expected) in cases {
            assert_eq!(
                choose_technical_cell_candidate(
                    primary,
                    confidence,
                    expected,
                    Some(0.91),
                    expected,
                    Some(0.93),
                ),
                Some((expected.to_string(), 0.91)),
                "expected a bounded repair for {primary}"
            );
        }

        assert!(super::suspicious_ascii_technical_token("ABC 7#3.5"));
        assert!(!super::suspicious_ascii_technical_token("ABC 7*3.5"));
        assert!(!super::suspicious_ascii_technical_token("电缆 ABC 7#3.5"));
        assert_eq!(
            choose_technical_cell_candidate(
                "ABC 7#3.5",
                0.90,
                "ABC 7*3.5",
                Some(0.94),
                "ABC 7*3.5",
                Some(0.96)
            ),
            Some(("ABC 7*3.5".to_string(), 0.94))
        );

        assert_eq!(
            choose_technical_cell_candidate(
                "VVR3*254+2*10",
                0.8439,
                "VVR3*254+2*10",
                Some(0.8439),
                "VVR3*25+2*10",
                Some(0.8768),
            ),
            Some(("VVR3*25+2*10".to_string(), 0.8768)),
            "a one-edit compound cable mark may use the stronger isolated model"
        );

        for unchanged in [
            "RVV2*1.0",
            "RVV12*1.0",
            "RVV8*1.0",
            "RVV4*1.0",
            "M10X100",
            "M12X100",
        ] {
            assert_eq!(
                choose_technical_cell_candidate(
                    unchanged,
                    0.70,
                    unchanged,
                    Some(0.90),
                    unchanged,
                    Some(0.90),
                ),
                None,
                "an already-correct value must remain unchanged"
            );
        }

        assert_eq!(
            choose_technical_cell_candidate(
                "RVV8*1.0",
                0.42,
                "RVV6*1.0",
                Some(0.93),
                "RVV8*1.0",
                Some(0.95),
            ),
            None,
            "model disagreement must never authorize a replacement"
        );
        assert_eq!(
            choose_technical_cell_candidate(
                "M10X100",
                0.96,
                "M16X200",
                Some(0.96),
                "M16X200",
                Some(0.97),
            ),
            None,
            "a distant valid high-confidence primary must not be overwritten"
        );
        assert_eq!(
            choose_technical_cell_candidate(
                "VVR3*25+2*10",
                0.92,
                "VVR3*25+2*10",
                Some(0.92),
                "VVR3*25+2*16",
                Some(0.98),
            ),
            None,
            "the specialized model cannot overwrite a high-confidence valid cable mark alone"
        );
        assert_eq!(
            choose_technical_cell_candidate(
                "VVR3*25+2*10",
                0.80,
                "VVR3×25+2×10",
                Some(0.95),
                "VVR3×25+2×10",
                Some(0.95),
            ),
            None,
            "multiplication signs must not be invented in place of raw asterisks"
        );
    }

    #[test]
    fn english_segment_cleanup_retains_only_existing_verbatim_segments() {
        assert_eq!(
            extract_existing_english_segments("电机线缆 | Motor Cable"),
            Some("Motor Cable".to_string())
        );
        assert_eq!(
            extract_existing_english_segments("profinet线缆 | PROFINET Cable"),
            Some("PROFINET Cable".to_string())
        );
        assert_eq!(
            extract_existing_english_segments(
                "宽200*高150不锈钢桥架 | Cable Tray, Stainless Steel, | 200(W)*150(H)"
            ),
            Some("Cable Tray, Stainless Steel,\n200(W)*150(H)".to_string())
        );
        assert_eq!(extract_existing_english_segments("Motor Cable"), None);
        assert_eq!(extract_existing_english_segments("噪声 | x"), None);
        assert_eq!(
            extract_existing_english_segments("Cable | Tray, Stainless Steel"),
            None,
            "a meaningful ASCII prefix must never be discarded"
        );
        assert_eq!(
            extract_existing_english_segments("Кабель | Cable"),
            None,
            "Cyrillic content is supported input, not unsupported-script noise"
        );
    }

    #[test]
    fn chinese_technical_recheck_preserves_mixed_descriptions_and_cell_geometry() {
        struct ChineseEngine;
        impl OcrEngine for ChineseEngine {
            fn active_language_profile(&self) -> crate::ocr::OcrLanguageProfile {
                crate::ocr::OcrLanguageProfile::SimplifiedChinese
            }
            fn recognize_text(&self, _: &GrayImage) -> Result<String, Box<dyn std::error::Error>> {
                panic!("protected Chinese cells must not invoke English OCR")
            }
            fn recognize_words(
                &self,
                _: &GrayImage,
            ) -> Result<Vec<crate::dto::Word>, Box<dyn std::error::Error>> {
                panic!("protected Chinese cells must not invoke English OCR")
            }
        }
        let mut grid = grid_fixture(BoundingBox::new(20.0, 20.0, 900.0, 500.0), 3, 4, 0.95);
        for cell in &mut grid.cells {
            cell.text = if cell.row == 0 {
                ["No.", "Name", "Model", "Quantity"][cell.column as usize].into()
            } else {
                ["1", "电机线缆 | Motor Cable", "电缆 RVV18*1.0", "10"][cell.column as usize].into()
            };
            cell.confidence = 0.7;
        }
        let classification = crate::table_semantics::classify_table(&grid.to_matrix(), None);
        assert!(!classification.header_rows.is_empty());
        assert!(
            classification
                .column_roles
                .iter()
                .filter(|role| role.confidence >= 0.70)
                .count()
                >= 2
        );
        let before = serde_json::to_value(&grid).unwrap();
        let page = GrayImage::from_pixel(1000, 1000, Luma([255]));
        let raster = CellOcrRaster::full_page(&page, 300);
        recheck_suspicious_nonempty_cells(
            &mut grid,
            &raster,
            None,
            &ChineseEngine,
            DEFAULT_MAX_PIXELS,
            &mut PageStageTimings::default(),
        )
        .unwrap();
        assert_eq!(serde_json::to_value(&grid).unwrap(), before);
    }

    #[test]
    fn technical_recheck_never_modifies_a_structural_title_block() {
        let mut grid = grid_fixture(BoundingBox::new(700.0, 750.0, 990.0, 990.0), 6, 6, 0.9);
        for cell in &mut grid.cells {
            cell.text = if cell.row == 0 {
                format!("header {}", cell.column)
            } else {
                "噪声 | M16X200".to_string()
            };
            cell.confidence = 0.2;
        }
        let before = grid
            .cells
            .iter()
            .map(|cell| cell.text.clone())
            .collect::<Vec<_>>();
        let page = GrayImage::from_pixel(1_000, 1_000, Luma([255]));
        let raster = CellOcrRaster::full_page(&page, 300);
        let mut timings = PageStageTimings::default();

        let changed = recheck_suspicious_nonempty_cells(
            &mut grid,
            &raster,
            None,
            &StubOcrEngine::new(),
            DEFAULT_MAX_PIXELS,
            &mut timings,
        )
        .unwrap();

        assert!(changed.is_empty());
        assert_eq!(
            grid.cells
                .iter()
                .map(|cell| cell.text.clone())
                .collect::<Vec<_>>(),
            before
        );
    }

    #[test]
    fn cell_ocr_region_maps_thumbnail_coordinates_without_changing_grid_geometry() {
        let source_page = RenderedPage::new(1, GrayImage::new(100, 50), 100.0);
        let region = RenderedPageRegion {
            image: GrayImage::new(120, 60),
            dpi: 300,
            full_width: 300,
            full_height: 150,
            x: 90,
            y: 30,
        };
        let raster =
            CellOcrRaster::from_region(&source_page, &region).expect("valid region metadata");
        let mapped = raster
            .map_bbox(&BoundingBox::new(40.0, 15.0, 60.0, 25.0))
            .expect("cell intersects the rendered ROI");
        assert_eq!((mapped.x0, mapped.y0), (30.0, 15.0));
        assert_eq!((mapped.x1, mapped.y1), (90.0, 45.0));
        assert_eq!(raster.dpi, 300);
    }

    #[test]
    fn padded_grid_region_is_normalized_and_clipped_to_the_page() {
        let region =
            normalized_padded_grid_region(&BoundingBox::new(0.0, 10.0, 90.0, 50.0), 100, 60)
                .expect("valid grid region");
        assert_eq!(region.0, 0.0);
        assert!(region.1 >= 0.0);
        assert!(region.2 > 0.9 && region.2 <= 1.0);
        assert!(region.3 > 0.0 && region.1 + region.3 <= 1.0 + f32::EPSILON);
    }

    #[test]
    fn roi_budget_estimate_scales_with_density_and_area() {
        let rendered = RenderedPage::new(1, GrayImage::new(1_000, 500), 100.0);
        let pixels = estimated_region_pixels(&rendered, 300, 0.5, 0.5).expect("bounded estimate");
        assert!((1_100_000..1_200_000).contains(&pixels));
        assert!(estimated_region_pixels(&rendered, 0, 0.5, 0.5).is_none());
    }

    #[test]
    fn invalid_or_partial_roi_metadata_is_not_used_for_cell_ocr() {
        let source_page = RenderedPage::new(1, GrayImage::new(100, 50), 100.0);
        let invalid = RenderedPageRegion {
            image: GrayImage::new(30, 20),
            dpi: 300,
            full_width: 100,
            full_height: 100,
            x: 80,
            y: 10,
        };
        assert!(CellOcrRaster::from_region(&source_page, &invalid).is_none());

        let valid = RenderedPageRegion {
            image: GrayImage::new(30, 20),
            dpi: 300,
            full_width: 300,
            full_height: 150,
            x: 270,
            y: 10,
        };
        let raster = CellOcrRaster::from_region(&source_page, &valid).expect("valid ROI");
        assert!(raster
            .map_bbox(&BoundingBox::new(0.0, 0.0, 20.0, 10.0))
            .is_none());

        let inconsistent_scale = RenderedPageRegion {
            image: GrayImage::new(30, 20),
            dpi: 300,
            full_width: 300,
            full_height: 100,
            x: 270,
            y: 10,
        };
        assert!(CellOcrRaster::from_region(&source_page, &inconsistent_scale).is_none());
    }

    #[test]
    fn repeated_manufacturer_consensus_repairs_only_one_glyph_ascii_outlier() {
        let mut grid = grid_fixture(BoundingBox::new(0.0, 0.0, 900.0, 240.0), 6, 9, 1.0);
        for cell in &mut grid.cells {
            if cell.row == 1 {
                cell.text = (cell.column + 1).to_string();
                cell.confidence = 0.8;
            } else if cell.column == 4 && (2..=4).contains(&cell.row) {
                cell.text = "DKC".to_string();
                cell.confidence = 0.8;
            } else if cell.row == 5 && cell.column == 4 {
                cell.text = "DKCE".to_string();
                cell.confidence = 0.5;
            }
        }

        let changed = apply_repeated_ascii_manufacturer_consensus(&mut grid, 1);
        assert_eq!(changed.len(), 1);
        assert_eq!(
            grid.cells
                .iter()
                .find(|cell| cell.row == 5 && cell.column == 4)
                .unwrap()
                .text,
            "DKC"
        );
    }

    #[test]
    fn sequential_multipage_pipeline_finishes_and_reports_every_page() {
        const PAGE_COUNT: u32 = 35;

        struct PageBoundaryOcr {
            begin_calls: AtomicUsize,
        }

        impl OcrEngine for PageBoundaryOcr {
            fn begin_page(&self) -> Result<(), Box<dyn std::error::Error>> {
                self.begin_calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }

            fn recognize_text(
                &self,
                image: &GrayImage,
            ) -> Result<String, Box<dyn std::error::Error>> {
                Ok(format!("page {}x{}", image.width(), image.height()))
            }

            fn recognize_words(
                &self,
                _image: &GrayImage,
            ) -> Result<Vec<crate::dto::Word>, Box<dyn std::error::Error>> {
                Ok(Vec::new())
            }
        }

        let pages = (0..PAGE_COUNT)
            .map(|_| GrayImage::from_pixel(32, 32, Luma([255])))
            .collect();
        let mut source = StubPageSource::new(pages);
        let events = Arc::new(Mutex::new(Vec::<OcrProgressEvent>::new()));
        let event_sink = Arc::clone(&events);
        let progress = move |event: &OcrProgressEvent| {
            event_sink
                .lock()
                .expect("test progress lock")
                .push(event.clone());
        };

        let ocr = PageBoundaryOcr {
            begin_calls: AtomicUsize::new(0),
        };

        let started = Instant::now();
        let outcome = Pipeline::new(&mut source, &ocr, DEFAULT_MAX_PIXELS, None)
            .with_table_mode(TableMode::Off)
            .with_progress(Some(&progress))
            .run_with_warnings_parallel(1)
            .expect("35-page sequential pipeline should complete");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "test source must not silently hang"
        );
        assert_eq!(outcome.document.pages.len(), PAGE_COUNT as usize);
        assert_eq!(ocr.begin_calls.load(Ordering::SeqCst), PAGE_COUNT as usize);
        assert!(outcome
            .document
            .pages
            .iter()
            .all(|page| page.text == outcome.document.pages[0].text));
        assert!(outcome
            .page_processing
            .iter()
            .all(|page| page.processing_mode == ProcessingMode::Ocr));

        let events = events.lock().expect("test progress lock");
        assert_eq!(
            events
                .iter()
                .filter(|event| {
                    event.stage == OcrProgressStage::NativeText
                        && event.page_number.is_some()
                        && event.processing_mode.is_none()
                })
                .count(),
            PAGE_COUNT as usize
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| {
                    event.stage == OcrProgressStage::Aggregation && event.page_number.is_some()
                })
                .count(),
            PAGE_COUNT as usize
        );
        assert!(events.iter().any(|event| {
            event.stage == OcrProgressStage::Aggregation && event.page_number.is_none()
        }));
    }
}
