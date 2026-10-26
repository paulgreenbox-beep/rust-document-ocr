//! Produce a visual review package for the ruled-table grid detector.
//!
//! The binary consumes already-rendered real PDF pages and the corresponding
//! Tesseract TSV word boxes. It is deliberately separate from the production
//! CLI: no PDFium, Tesseract, DOCX, or normal OCR output path is changed by
//! this review tool.

use std::error::Error;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use clap::Parser;
use image::imageops::crop_imm;
use image::{GrayImage, Rgb, RgbImage};
use imageproc::drawing::{draw_filled_circle_mut, draw_hollow_rect_mut};
use imageproc::rect::Rect;
use ocr_pipeline::dto::{BoundingBox, TableMatrix, TableType, Word};
use ocr_pipeline::export_xlsx;
use ocr_pipeline::reconstruction::reconstruct_table;
use ocr_pipeline::table_grid::{
    classify_grid, detect_table_grids, GridDetectionConfig, GridIntersection, GridLine,
    TableAssignmentMetrics, TableGrid,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(about = "Create a grid-detector review package from real page PNG/TSV fixtures")]
struct Args {
    /// JSON manifest with real rendered pages and Tesseract TSV word boxes.
    #[arg(long)]
    manifest: PathBuf,
    /// Fresh output directory. It must not exist.
    #[arg(long)]
    output_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ReviewManifest {
    inputs: Vec<ReviewInput>,
    #[serde(default = "default_max_tables")]
    max_tables: usize,
}

#[derive(Debug, Deserialize)]
struct ReviewInput {
    page_number: u32,
    image: PathBuf,
    tsv: PathBuf,
}

#[derive(Debug, Serialize)]
struct ReviewSummary {
    config_note: &'static str,
    tables: Vec<ReviewTable>,
}

#[derive(Debug, Serialize)]
struct ReviewTable {
    ordinal: usize,
    page_number: u32,
    table_type: TableType,
    grid: TableGrid,
    metrics: TableAssignmentMetrics,
    old_columns: usize,
    grid_columns: u32,
    old_non_empty_cells: usize,
    grid_non_empty_cells: usize,
    assigned_words: u32,
    artifacts: ReviewArtifacts,
}

#[derive(Debug, Serialize)]
struct ReviewArtifacts {
    original_crop_png: String,
    binary_mask_png: String,
    horizontal_mask_png: String,
    vertical_mask_png: String,
    intersections_overlay_png: String,
    cells_overlay_png: String,
    table_json: String,
}

#[derive(Debug, Serialize)]
struct TableJson<'a> {
    page_number: u32,
    table_type: TableType,
    config: &'a GridDetectionConfig,
    grid: &'a TableGrid,
    metrics: &'a TableAssignmentMetrics,
}

#[derive(Debug, Serialize)]
struct PageLineDiagnostics<'a> {
    config: &'a GridDetectionConfig,
    horizontal_lines: &'a [GridLine],
    vertical_lines: &'a [GridLine],
}

fn default_max_tables() -> usize {
    3
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let manifest: ReviewManifest = serde_json::from_reader(fs::File::open(&args.manifest)?)?;
    validate_manifest(&manifest)?;
    validate_output_dir(&args.output_dir)?;

    // The output is created only after all input paths and the manifest have
    // been validated. It is never overwritten or cleaned up by this tool.
    fs::create_dir(&args.output_dir)?;

    let mut tables = Vec::new();
    let mut matrices: Vec<(u32, TableMatrix)> = Vec::new();

    'inputs: for input in manifest.inputs {
        let image = image::open(&input.image)?.to_luma8();
        let words = parse_tsv_words(&input.tsv)?;
        let detection = detect_table_grids(&image);
        let config = detection.config;
        let binary_mask = detection.binary_mask;
        let horizontal_mask = detection.horizontal_mask;
        let vertical_mask = detection.vertical_mask;
        let horizontal_lines = detection.horizontal_lines;
        let vertical_lines = detection.vertical_lines;
        let mut grids = detection.grids;
        write_page_diagnostics(
            &args.output_dir,
            input.page_number,
            &image,
            &binary_mask,
            &horizontal_mask,
            &vertical_mask,
            &config,
            &horizontal_lines,
            &vertical_lines,
        )?;
        eprintln!(
            "page={} horizontal_lines={} vertical_lines={} ruled_grid_candidates={}",
            input.page_number,
            horizontal_lines.len(),
            vertical_lines.len(),
            grids.len()
        );
        grids.sort_by(|left, right| {
            left.bbox
                .y0
                .total_cmp(&right.bbox.y0)
                .then_with(|| left.bbox.x0.total_cmp(&right.bbox.x0))
        });

        for mut grid in grids {
            if tables.len() >= manifest.max_tables {
                break 'inputs;
            }

            let metrics = grid.assign_words(&words);
            let table_type = classify_grid(&grid, &metrics, image.width(), image.height());
            eprintln!(
                "page={} table_type={:?} filled_cell_ratio={:.3}",
                input.page_number,
                table_type,
                metrics.filled_cell_ratio()
            );
            let words_in_grid: Vec<Word> = words
                .iter()
                .filter(|word| boxes_overlap(&word.bbox, &grid.bbox))
                .cloned()
                .collect();
            let old = reconstruct_table(words_in_grid, 5.0, 10.0);
            let ordinal = tables.len() + 1;
            let artifact_dir_name = format!("table-{ordinal:02}-page-{:04}", input.page_number);
            let artifact_dir = args.output_dir.join(&artifact_dir_name);
            fs::create_dir(&artifact_dir)?;

            let artifacts = write_artifacts(
                &artifact_dir,
                &image,
                &binary_mask,
                &horizontal_mask,
                &vertical_mask,
                &grid,
                &metrics,
                &config,
                input.page_number,
                table_type.clone(),
            )?;

            let matrix = grid.to_matrix_with_type(table_type.clone());
            matrices.push((input.page_number, matrix));
            tables.push(ReviewTable {
                ordinal,
                page_number: input.page_number,
                table_type,
                old_columns: old.rows.iter().map(Vec::len).max().unwrap_or(0),
                grid_columns: grid.columns(),
                old_non_empty_cells: non_empty_cells(&old),
                grid_non_empty_cells: grid
                    .cells
                    .iter()
                    .filter(|cell| !cell.text.is_empty())
                    .count(),
                assigned_words: metrics.words_assigned,
                grid,
                metrics,
                artifacts,
            });
        }
    }

    if tables.len() < manifest.max_tables {
        return Err(format!(
            "only {} ruled tables were found; expected {}",
            tables.len(),
            manifest.max_tables
        )
        .into());
    }

    let export_inputs: Vec<(u32, &TableMatrix)> = matrices
        .iter()
        .map(|(page, matrix)| (*page, matrix))
        .collect();
    export_xlsx(&export_inputs, &args.output_dir.join("grid-tables.xlsx"))?;

    let summary = ReviewSummary {
        config_note: "imageproc adaptive mean threshold plus directional grayscale opening; no borderless-table path",
        tables,
    };
    write_json(&args.output_dir.join("summary.json"), &summary)?;
    Ok(())
}

fn write_page_diagnostics(
    output_dir: &Path,
    page_number: u32,
    image: &GrayImage,
    binary_mask: &GrayImage,
    horizontal_mask: &GrayImage,
    vertical_mask: &GrayImage,
    config: &GridDetectionConfig,
    horizontal_lines: &[GridLine],
    vertical_lines: &[GridLine],
) -> Result<(), Box<dyn Error>> {
    let directory = output_dir.join(format!("page-{page_number:04}-diagnostics"));
    fs::create_dir(&directory)?;
    image.save(directory.join("source.png"))?;
    binary_mask.save(directory.join("binary-mask.png"))?;
    horizontal_mask.save(directory.join("horizontal-mask.png"))?;
    vertical_mask.save(directory.join("vertical-mask.png"))?;
    write_json(
        &directory.join("lines.json"),
        &PageLineDiagnostics {
            config,
            horizontal_lines,
            vertical_lines,
        },
    )?;
    Ok(())
}

fn validate_manifest(manifest: &ReviewManifest) -> Result<(), Box<dyn Error>> {
    if manifest.inputs.is_empty() {
        return Err("review manifest has no inputs".into());
    }
    if manifest.max_tables == 0 {
        return Err("max_tables must be greater than zero".into());
    }
    for input in &manifest.inputs {
        if !input.image.is_file() {
            return Err(format!("input image does not exist: {}", input.image.display()).into());
        }
        if !input.tsv.is_file() {
            return Err(format!("input TSV does not exist: {}", input.tsv.display()).into());
        }
    }
    Ok(())
}

fn validate_output_dir(path: &Path) -> Result<(), Box<dyn Error>> {
    if path.exists() {
        return Err(format!("output directory already exists: {}", path.display()).into());
    }
    let parent = path
        .parent()
        .ok_or_else(|| format!("output directory has no parent: {}", path.display()))?;
    if !parent.is_dir() {
        return Err(format!("output parent does not exist: {}", parent.display()).into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_artifacts(
    directory: &Path,
    image: &GrayImage,
    binary_mask: &GrayImage,
    horizontal_mask: &GrayImage,
    vertical_mask: &GrayImage,
    grid: &TableGrid,
    metrics: &TableAssignmentMetrics,
    config: &GridDetectionConfig,
    page_number: u32,
    table_type: TableType,
) -> Result<ReviewArtifacts, Box<dyn Error>> {
    let original_crop = crop(image, &grid.bbox)?;
    let binary_crop = crop(binary_mask, &grid.bbox)?;
    let horizontal_crop = crop(horizontal_mask, &grid.bbox)?;
    let vertical_crop = crop(vertical_mask, &grid.bbox)?;

    let original_path = directory.join("original-crop.png");
    let binary_path = directory.join("binary-mask.png");
    let horizontal_path = directory.join("horizontal-mask.png");
    let vertical_path = directory.join("vertical-mask.png");
    let intersections_path = directory.join("intersections-overlay.png");
    let cells_path = directory.join("cells-overlay.png");
    let json_path = directory.join("table.json");

    original_crop.save(&original_path)?;
    binary_crop.save(&binary_path)?;
    horizontal_crop.save(&horizontal_path)?;
    vertical_crop.save(&vertical_path)?;
    intersections_overlay(&original_crop, grid, &intersections_path)?;
    cells_overlay(&original_crop, grid, &cells_path)?;
    write_json(
        &json_path,
        &TableJson {
            page_number,
            table_type,
            config,
            grid,
            metrics,
        },
    )?;

    Ok(ReviewArtifacts {
        original_crop_png: relative_name(&original_path),
        binary_mask_png: relative_name(&binary_path),
        horizontal_mask_png: relative_name(&horizontal_path),
        vertical_mask_png: relative_name(&vertical_path),
        intersections_overlay_png: relative_name(&intersections_path),
        cells_overlay_png: relative_name(&cells_path),
        table_json: relative_name(&json_path),
    })
}

fn crop(image: &GrayImage, bbox: &BoundingBox) -> Result<GrayImage, Box<dyn Error>> {
    let width = image.width();
    let height = image.height();
    let x0 = bbox
        .x0
        .floor()
        .clamp(0.0, f64::from(width.saturating_sub(1))) as u32;
    let y0 = bbox
        .y0
        .floor()
        .clamp(0.0, f64::from(height.saturating_sub(1))) as u32;
    let x1 = bbox.x1.ceil().clamp(f64::from(x0 + 1), f64::from(width)) as u32;
    let y1 = bbox.y1.ceil().clamp(f64::from(y0 + 1), f64::from(height)) as u32;
    Ok(crop_imm(image, x0, y0, x1 - x0, y1 - y0).to_image())
}

fn intersections_overlay(
    crop: &GrayImage,
    grid: &TableGrid,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    let mut overlay = gray_to_rgb(crop);
    for intersection in &grid.intersections {
        draw_intersection(&mut overlay, intersection, &grid.bbox);
    }
    overlay.save(output)?;
    Ok(())
}

fn cells_overlay(crop: &GrayImage, grid: &TableGrid, output: &Path) -> Result<(), Box<dyn Error>> {
    let mut overlay = gray_to_rgb(crop);
    for cell in &grid.cells {
        let Some(bbox) = cell.bbox.as_ref() else {
            continue;
        };
        let left = (bbox.x0 - grid.bbox.x0).round().max(0.0) as i32;
        let top = (bbox.y0 - grid.bbox.y0).round().max(0.0) as i32;
        let width = (bbox.x1 - bbox.x0).round().max(1.0) as u32;
        let height = (bbox.y1 - bbox.y0).round().max(1.0) as u32;
        draw_hollow_rect_mut(
            &mut overlay,
            Rect::at(left, top).of_size(width, height),
            Rgb([0, 220, 0]),
        );
    }
    overlay.save(output)?;
    Ok(())
}

fn draw_intersection(overlay: &mut RgbImage, point: &GridIntersection, bbox: &BoundingBox) {
    let x = (point.x - bbox.x0).round() as i32;
    let y = (point.y - bbox.y0).round() as i32;
    draw_filled_circle_mut(overlay, (x, y), 4, Rgb([235, 40, 40]));
}

fn gray_to_rgb(image: &GrayImage) -> RgbImage {
    RgbImage::from_fn(image.width(), image.height(), |x, y| {
        let value = image.get_pixel(x, y)[0];
        Rgb([value, value, value])
    })
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), Box<dyn Error>> {
    let file = fs::File::create(path)?;
    serde_json::to_writer_pretty(BufWriter::new(file), value)?;
    Ok(())
}

fn relative_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact")
        .to_string()
}

fn parse_tsv_words(path: &Path) -> Result<Vec<Word>, Box<dyn Error>> {
    let source = fs::read_to_string(path)?;
    let mut words = Vec::new();
    for (line_number, line) in source.lines().enumerate().skip(1) {
        let fields: Vec<&str> = line.splitn(12, '\t').collect();
        if fields.len() != 12 {
            return Err(format!("{}:{line_number}: malformed TSV row", path.display()).into());
        }
        let text = fields[11].trim();
        if text.is_empty() {
            continue;
        }
        let left: f64 = fields[6].parse()?;
        let top: f64 = fields[7].parse()?;
        let width: f64 = fields[8].parse()?;
        let height: f64 = fields[9].parse()?;
        let confidence: f32 = fields[10].parse()?;
        if confidence < 0.0 {
            continue;
        }
        words.push(Word::new(
            text,
            BoundingBox::new(left, top, left + width, top + height),
            (confidence / 100.0).clamp(0.0, 1.0),
        ));
    }
    Ok(words)
}

fn boxes_overlap(left: &BoundingBox, right: &BoundingBox) -> bool {
    left.x0 < right.x1 && left.x1 > right.x0 && left.y0 < right.y1 && left.y1 > right.y0
}

fn non_empty_cells(matrix: &TableMatrix) -> usize {
    matrix
        .rows
        .iter()
        .flatten()
        .filter(|cell| !cell.text.is_empty())
        .count()
}
