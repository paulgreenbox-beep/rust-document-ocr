//! Grid-based table reconstruction for ruled tables.
//!
//! This module deliberately handles only tables with visible horizontal and
//! vertical rules. The separate `borderless_table` module runs only after this
//! detector finds no ruled grid, so improvements to word alignment cannot
//! degrade the established line-based path. The legacy reconstruction remains
//! available as the final compatibility fallback.

use std::collections::VecDeque;

use image::{GrayImage, Luma};
use imageproc::contrast::adaptive_threshold;
use serde::{Deserialize, Serialize};

use crate::dto::{BoundingBox, TableCell, TableMatrix, TableType, Word};
use crate::page_analysis::{is_cyrillic, is_latin};

/// Sparse content is supporting evidence only when a ruled grid also has the
/// geometry of a lower-right drawing title block. A sparse table elsewhere on
/// the page remains a data table, so legitimate technical tables with many
/// empty cells are not reclassified merely because they are sparse.
pub const TITLE_BLOCK_MAX_FILLED_CELL_RATIO: f32 = 0.25;

/// A physical horizontal or vertical line after adjacent raster segments have
/// been merged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridLine {
    /// Constant axis coordinate: `y` for horizontal lines and `x` for vertical lines.
    pub position: f64,
    /// Inclusive start coordinate along the other axis.
    pub start: f64,
    /// Exclusive end coordinate along the other axis.
    pub end: f64,
}

/// A confirmed horizontal-by-vertical grid-line intersection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridIntersection {
    pub x: f64,
    pub y: f64,
}

/// Parameters derived from a rendered page. They are retained in review
/// reports so that a detection result is reproducible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridDetectionConfig {
    /// Radius of the local window used by imageproc's adaptive mean threshold.
    pub adaptive_radius: u32,
    /// Threshold bias passed to imageproc's adaptive threshold.
    pub adaptive_delta: i32,
    /// Length of the horizontal morphology mask, in pixels.
    pub horizontal_kernel: u32,
    /// Length of the vertical morphology mask, in pixels.
    pub vertical_kernel: u32,
    /// Shorter horizontal runs are ignored as decoration or text.
    pub min_horizontal_span: u32,
    /// Shorter vertical runs are ignored as decoration or text.
    pub min_vertical_span: u32,
    /// Neighboring raster runs within this many pixels are one physical line.
    pub line_merge_distance: u32,
    /// Tolerance when joining horizontal and vertical line segments.
    pub intersection_tolerance: u32,
    /// Minimum accepted grid-cell width.
    pub min_cell_width: u32,
    /// Minimum accepted grid-cell height.
    pub min_cell_height: u32,
}

impl GridDetectionConfig {
    /// Scale morphology relative to the rendered page rather than pinning it
    /// to a single PDF DPI.
    pub fn for_image(width: u32, height: u32) -> Self {
        let min_side = width.min(height).max(1);
        Self {
            adaptive_radius: (min_side / 160).clamp(3, 75),
            adaptive_delta: 10,
            horizontal_kernel: odd_kernel((width / 30).max(3)),
            vertical_kernel: odd_kernel((height / 30).max(3)),
            min_horizontal_span: (width / 20).max(24),
            min_vertical_span: (height / 20).max(24),
            line_merge_distance: 3,
            intersection_tolerance: 4,
            min_cell_width: (width / 250).clamp(12, 80),
            min_cell_height: (height / 250).clamp(12, 80),
        }
    }
}

/// A ruled table with geometry, grid cells, and assignment confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableGrid {
    pub bbox: BoundingBox,
    pub horizontal_lines: Vec<GridLine>,
    pub vertical_lines: Vec<GridLine>,
    pub intersections: Vec<GridIntersection>,
    pub cells: Vec<TableCell>,
    pub confidence: f32,
}

impl TableGrid {
    pub fn rows(&self) -> u32 {
        self.horizontal_lines.len().saturating_sub(1) as u32
    }

    pub fn columns(&self) -> u32 {
        self.vertical_lines.len().saturating_sub(1) as u32
    }

    /// Assign each OCR word inside the table box to at most one grid cell.
    /// Intersection area wins; a word-centre fallback handles tiny or
    /// anti-aliased OCR boxes that only touch a cell edge.
    pub fn assign_words(&mut self, words: &[Word]) -> TableAssignmentMetrics {
        for cell in &mut self.cells {
            cell.text.clear();
            cell.confidence = 0.0;
        }

        let mut assigned: Vec<Vec<&Word>> = vec![Vec::new(); self.cells.len()];
        let mut metrics = TableAssignmentMetrics {
            table_bbox: self.bbox.clone(),
            rows: self.rows(),
            columns: self.columns(),
            cells: self.cells.len() as u32,
            empty_cells: 0,
            ocr_words_total: 0,
            words_assigned: 0,
            words_unassigned: 0,
            duplicate_assignments: 0,
            detector_confidence: self.confidence,
        };

        for word in words {
            if !boxes_overlap(&word.bbox, &self.bbox) {
                continue;
            }
            metrics.ocr_words_total += 1;

            let mut best: Option<(usize, f64)> = None;
            for (index, cell) in self.cells.iter().enumerate() {
                let Some(cell_box) = cell.bbox.as_ref() else {
                    continue;
                };
                let score = intersection_area(&word.bbox, cell_box);
                if score > 0.0 && best.is_none_or(|(_, current)| score > current) {
                    best = Some((index, score));
                }
            }

            let cell_index = best.map(|(index, _)| index).or_else(|| {
                let (center_x, center_y) = box_center(&word.bbox);
                self.cells.iter().position(|cell| {
                    cell.bbox
                        .as_ref()
                        .is_some_and(|bbox| point_in_box(center_x, center_y, bbox))
                })
            });

            if let Some(index) = cell_index {
                assigned[index].push(word);
                metrics.words_assigned += 1;
            } else {
                metrics.words_unassigned += 1;
            }
        }

        for (cell, cell_words) in self.cells.iter_mut().zip(assigned) {
            if cell_words.is_empty() {
                metrics.empty_cells += 1;
                continue;
            }
            cell.text = words_to_cell_text(&cell_words);
            cell.confidence = cell_words.iter().map(|word| word.confidence).sum::<f32>()
                / cell_words.len() as f32;
        }

        metrics
    }

    /// Convert a geometrically reconstructed grid into the existing XLSX/JSON
    /// matrix contract without discarding detector provenance.
    pub fn to_matrix(&self) -> TableMatrix {
        self.to_matrix_with_type(TableType::DataTable)
    }

    pub fn to_matrix_with_type(&self, table_type: TableType) -> TableMatrix {
        let rows = self.rows() as usize;
        let columns = self.columns() as usize;
        let mut matrix = vec![vec![TableCell::default(); columns]; rows];

        // Empty physical slots still have a stable coordinate in both the
        // legacy matrix and the host-facing projection. Keep their geometry
        // absent, but do not let `Default` collapse every placeholder to 0,0.
        for (row, line) in matrix.iter_mut().enumerate() {
            for (column, cell) in line.iter_mut().enumerate() {
                cell.row = row as u32;
                cell.column = column as u32;
            }
        }

        for cell in &self.cells {
            let row = cell.row as usize;
            let column = cell.column as usize;
            if let Some(target) = matrix.get_mut(row).and_then(|line| line.get_mut(column)) {
                *target = cell.clone();
            }
        }

        TableMatrix::grid_with_type(matrix, self.bbox.clone(), self.confidence, table_type)
    }
}

/// Per-table metrics written to the review JSON and used for acceptance checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableAssignmentMetrics {
    pub table_bbox: BoundingBox,
    pub rows: u32,
    pub columns: u32,
    pub cells: u32,
    pub empty_cells: u32,
    pub ocr_words_total: u32,
    pub words_assigned: u32,
    pub words_unassigned: u32,
    pub duplicate_assignments: u32,
    pub detector_confidence: f32,
}

impl TableAssignmentMetrics {
    pub fn filled_cell_ratio(&self) -> f32 {
        if self.cells == 0 {
            0.0
        } else {
            (self.cells - self.empty_cells) as f32 / self.cells as f32
        }
    }
}

/// Classifies the grid without losing it. A title block requires the geometry
/// of a compact lower-right drawing frame; sparse content alone is not enough.
/// This preserves small, legitimate technical tables that happen to have many
/// empty cells. Consumers preserve both roles and decide how to present each
/// one.
pub fn classify_grid(
    grid: &TableGrid,
    metrics: &TableAssignmentMetrics,
    page_width: u32,
    page_height: u32,
) -> TableType {
    let page_width = f64::from(page_width.max(1));
    let page_height = f64::from(page_height.max(1));
    let box_width = grid.bbox.x1 - grid.bbox.x0;
    let box_height = grid.bbox.y1 - grid.bbox.y0;
    let lower_right_drawing_area = grid.bbox.x0 >= page_width * 0.50
        && grid.bbox.y0 >= page_height * 0.70
        && grid.bbox.x1 >= page_width * 0.65
        && grid.bbox.y1 >= page_height * 0.90
        && box_width <= page_width * 0.35
        && box_height <= page_height * 0.30;

    let sparse_lower_right_frame = lower_right_drawing_area
        && metrics.filled_cell_ratio() < TITLE_BLOCK_MAX_FILLED_CELL_RATIO
        && grid.rows() >= 3
        && grid.columns() >= 3;
    let compact_lower_right_frame =
        lower_right_drawing_area && grid.rows() >= 4 && grid.columns() >= 4;

    if sparse_lower_right_frame || compact_lower_right_frame {
        TableType::TitleBlock
    } else {
        TableType::DataTable
    }
}

/// Detection output retains the masks only for diagnostic/review tooling.
/// Production pipeline callers consume `grids` and then promptly drop masks.
#[derive(Debug)]
pub struct GridDetection {
    pub config: GridDetectionConfig,
    pub binary_mask: GrayImage,
    pub horizontal_mask: GrayImage,
    pub vertical_mask: GrayImage,
    /// Merged physical rules retained for review tooling. Production callers
    /// can drop them together with the diagnostic masks after extracting grids.
    pub horizontal_lines: Vec<GridLine>,
    pub vertical_lines: Vec<GridLine>,
    pub grids: Vec<TableGrid>,
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy)]
enum Extremum {
    Min,
    Max,
}

#[derive(Debug, Clone, Copy)]
struct RawLine {
    position: u32,
    start: u32,
    end: u32,
}

#[derive(Debug)]
struct LineBucket {
    position_sum: u64,
    samples: u32,
    start: u32,
    end: u32,
    last_position: u32,
}

#[derive(Debug, Clone)]
struct CandidateCell {
    bbox: BoundingBox,
}

/// Detect independent ruled table areas on a page.
pub fn detect_table_grids(image: &GrayImage) -> GridDetection {
    detect_table_grids_with_config(
        image,
        GridDetectionConfig::for_image(image.width(), image.height()),
    )
}

/// As [`detect_table_grids`], with an explicit configuration for reproducible
/// tests and controlled experiments.
pub fn detect_table_grids_with_config(
    image: &GrayImage,
    config: GridDetectionConfig,
) -> GridDetection {
    if image.width() == 0 || image.height() == 0 {
        return GridDetection {
            config,
            binary_mask: GrayImage::new(image.width(), image.height()),
            horizontal_mask: GrayImage::new(image.width(), image.height()),
            vertical_mask: GrayImage::new(image.width(), image.height()),
            horizontal_lines: Vec::new(),
            vertical_lines: Vec::new(),
            grids: Vec::new(),
        };
    }

    // imageproc's adaptive threshold makes bright background white. Invert it
    // so ink, rules, and text are foreground for grayscale morphology.
    let thresholded =
        adaptive_threshold_safe(image, config.adaptive_radius.max(1), config.adaptive_delta);
    let binary_mask = GrayImage::from_fn(image.width(), image.height(), |x, y| {
        Luma([u8::MAX - thresholded.get_pixel(x, y)[0]])
    });

    // Directional opening is erosion followed by dilation. Text and isolated
    // specks cannot survive the long 1×N / N×1 structuring element, which also
    // serves as this iteration's small-noise filter.
    let horizontal_mask = directional_open(
        &binary_mask,
        Direction::Horizontal,
        config.horizontal_kernel,
    );
    let vertical_mask = directional_open(&binary_mask, Direction::Vertical, config.vertical_kernel);

    let horizontal_lines = merge_parallel_lines(
        collect_line_segments(
            &horizontal_mask,
            Direction::Horizontal,
            config.min_horizontal_span,
        ),
        config.line_merge_distance,
    );
    let vertical_lines = merge_parallel_lines(
        collect_line_segments(
            &vertical_mask,
            Direction::Vertical,
            config.min_vertical_span,
        ),
        config.line_merge_distance,
    );

    let x_positions = stable_positions(
        vertical_lines.iter().map(|line| line.position).collect(),
        f64::from(config.min_cell_width),
    );
    let y_positions = stable_positions(
        horizontal_lines.iter().map(|line| line.position).collect(),
        f64::from(config.min_cell_height),
    );
    let candidate_cells = collect_candidate_cells(
        &x_positions,
        &y_positions,
        &horizontal_lines,
        &vertical_lines,
        &config,
    );
    let grids =
        grids_from_candidate_cells(&candidate_cells, image.width(), image.height(), &config);

    GridDetection {
        config,
        binary_mask,
        horizontal_mask,
        vertical_mask,
        horizontal_lines,
        vertical_lines,
        grids,
    }
}

fn adaptive_threshold_safe(image: &GrayImage, radius: u32, delta: i32) -> GrayImage {
    // imageproc's implementation uses a u32 summed-area table. Its largest
    // value is the sum of all source pixels, so a full-page scan can overflow
    // above u32::MAX / 255 (for example, a 6,622×4,678 render). Keep its
    // well-tested implementation for safe images and use an equivalent
    // linear-memory local-mean pass for larger pages.
    let pixel_count = u64::from(image.width()) * u64::from(image.height());
    // The lower cap is intentional: imageproc's u32 integral image can
    // overflow below the mathematical maximum on some high-DPI inputs.
    const IMAGEPROC_SAFE_PIXEL_LIMIT: u64 = 8_000_000;
    let imageproc_limit =
        (u64::from(u32::MAX) / u64::from(u8::MAX)).min(IMAGEPROC_SAFE_PIXEL_LIMIT);
    if pixel_count <= imageproc_limit {
        return adaptive_threshold(image, radius, delta);
    }

    adaptive_threshold_sliding(image, radius as usize, delta)
}

fn adaptive_threshold_sliding(image: &GrayImage, radius: usize, delta: i32) -> GrayImage {
    let (width, height) = image.dimensions();
    let width = width as usize;
    let height = height as usize;
    if width == 0 || height == 0 {
        return GrayImage::new(width as u32, height as u32);
    }

    let source = image.as_raw();
    let mut output = GrayImage::new(width as u32, height as u32);
    let target = output.as_mut();
    let mut vertical_sums = vec![0u64; width];
    let mut source_rows: VecDeque<Vec<u32>> = VecDeque::new();
    let mut next_row = 0usize;
    let mut first_buffered_row = 0usize;

    for y in 0..height {
        let bottom = y.saturating_add(radius).min(height - 1);
        while next_row <= bottom {
            let start = next_row * width;
            let row = horizontal_window_sums(&source[start..start + width], radius);
            for (sum, value) in vertical_sums.iter_mut().zip(&row) {
                *sum += u64::from(*value);
            }
            source_rows.push_back(row);
            next_row += 1;
        }

        let top = y.saturating_sub(radius);
        while first_buffered_row < top {
            let expired = source_rows
                .pop_front()
                .expect("row buffer includes every row in the local window");
            for (sum, value) in vertical_sums.iter_mut().zip(expired) {
                *sum -= u64::from(value);
            }
            first_buffered_row += 1;
        }

        let vertical_count = next_row - first_buffered_row;
        for x in 0..width {
            let left = x.saturating_sub(radius);
            let right = x.saturating_add(radius).min(width - 1);
            let local_count = (vertical_count * (right - left + 1)) as u64;
            let local_mean = (vertical_sums[x] / local_count) as i32;
            target[y * width + x] = if i32::from(source[y * width + x]) >= local_mean - delta {
                u8::MAX
            } else {
                0
            };
        }
    }

    output
}

fn horizontal_window_sums(values: &[u8], radius: usize) -> Vec<u32> {
    let mut sums = Vec::with_capacity(values.len());
    let mut sum = 0u32;
    let mut next_index = 0usize;
    let mut first_index = 0usize;

    for center in 0..values.len() {
        let right = center.saturating_add(radius).min(values.len() - 1);
        while next_index <= right {
            sum += u32::from(values[next_index]);
            next_index += 1;
        }

        let left = center.saturating_sub(radius);
        while first_index < left {
            sum -= u32::from(values[first_index]);
            first_index += 1;
        }
        sums.push(sum);
    }

    sums
}

#[expect(
    clippy::manual_is_multiple_of,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
fn odd_kernel(value: u32) -> u32 {
    let bounded = value.clamp(3, 511);
    if bounded % 2 == 0 {
        bounded.saturating_sub(1).max(3)
    } else {
        bounded
    }
}

fn directional_open(image: &GrayImage, direction: Direction, kernel: u32) -> GrayImage {
    let kernel = odd_kernel(kernel);
    // `imageproc::morphology::grayscale_open()` accepts an arbitrary Mask,
    // but visits every element of a long one-dimensional mask for every
    // pixel. A 300-DPI A1 page can make that quadratic-in-kernel work take
    // minutes. This is the same flat directional erosion + dilation expressed
    // as a monotonic sliding window, so it remains a true morphological
    // opening while running in O(width * height) per direction.
    let eroded = directional_extreme(image, direction, kernel, Extremum::Min);
    directional_extreme(&eroded, direction, kernel, Extremum::Max)
}

fn directional_extreme(
    image: &GrayImage,
    direction: Direction,
    kernel: u32,
    extremum: Extremum,
) -> GrayImage {
    let (width, height) = image.dimensions();
    let width = width as usize;
    let height = height as usize;
    let mut output = GrayImage::new(width as u32, height as u32);
    let source = image.as_raw();
    let target = output.as_mut();

    match direction {
        Direction::Horizontal => {
            for row in 0..height {
                let start = row * width;
                let end = start + width;
                let filtered = sliding_extreme(&source[start..end], kernel as usize, extremum);
                target[start..end].copy_from_slice(&filtered);
            }
        }
        Direction::Vertical => {
            let mut column = Vec::with_capacity(height);
            for x in 0..width {
                column.clear();
                for y in 0..height {
                    column.push(source[y * width + x]);
                }
                let filtered = sliding_extreme(&column, kernel as usize, extremum);
                for (y, value) in filtered.into_iter().enumerate() {
                    target[y * width + x] = value;
                }
            }
        }
    }

    output
}

fn sliding_extreme(values: &[u8], kernel: usize, extremum: Extremum) -> Vec<u8> {
    if values.is_empty() {
        return Vec::new();
    }

    let half_kernel = kernel / 2;
    let mut next_index = 0usize;
    let mut window = VecDeque::with_capacity(kernel.min(values.len()));
    let mut output = Vec::with_capacity(values.len());

    for center in 0..values.len() {
        let right = center.saturating_add(half_kernel).min(values.len() - 1);
        while next_index <= right {
            while let Some(&last) = window.back() {
                let replace_last = match extremum {
                    Extremum::Min => values[next_index] <= values[last],
                    Extremum::Max => values[next_index] >= values[last],
                };
                if replace_last {
                    window.pop_back();
                } else {
                    break;
                }
            }
            window.push_back(next_index);
            next_index += 1;
        }

        let left = center.saturating_sub(half_kernel);
        while window.front().is_some_and(|&index| index < left) {
            window.pop_front();
        }
        output.push(values[*window.front().expect("window contains current pixel")]);
    }

    output
}

fn collect_line_segments(mask: &GrayImage, direction: Direction, min_span: u32) -> Vec<RawLine> {
    let (width, height) = mask.dimensions();
    let mut lines = Vec::new();

    match direction {
        Direction::Horizontal => {
            for y in 0..height {
                for (start, end) in foreground_runs((0..width).map(|x| mask.get_pixel(x, y)[0])) {
                    if end - start >= min_span {
                        lines.push(RawLine {
                            position: y,
                            start,
                            end,
                        });
                    }
                }
            }
        }
        Direction::Vertical => {
            for x in 0..width {
                for (start, end) in foreground_runs((0..height).map(|y| mask.get_pixel(x, y)[0])) {
                    if end - start >= min_span {
                        lines.push(RawLine {
                            position: x,
                            start,
                            end,
                        });
                    }
                }
            }
        }
    }

    lines
}

fn foreground_runs(values: impl IntoIterator<Item = u8>) -> Vec<(u32, u32)> {
    let mut runs = Vec::new();
    let mut start = None;
    let mut index = 0u32;

    for value in values {
        if value >= 128 {
            start.get_or_insert(index);
        } else if let Some(begin) = start.take() {
            runs.push((begin, index));
        }
        index += 1;
    }
    if let Some(begin) = start {
        runs.push((begin, index));
    }
    runs
}

fn merge_parallel_lines(mut lines: Vec<RawLine>, max_gap: u32) -> Vec<GridLine> {
    lines.sort_by_key(|line| (line.position, line.start));
    let mut buckets: Vec<LineBucket> = Vec::new();

    for line in lines {
        // A one-pixel-thick physical rule is often emitted as several raw
        // raster segments at the same coordinate (for example, on either
        // side of a small scan gap). Do not only inspect the last bucket: the
        // next adjacent pixel column/row may need to join an earlier segment.
        let bucket_index = buckets
            .iter()
            .enumerate()
            .filter(|(_, bucket)| {
                line.position.saturating_sub(bucket.last_position) <= max_gap
                    && ranges_touch_or_overlap(
                        line.start,
                        line.end,
                        bucket.start,
                        bucket.end,
                        max_gap,
                    )
            })
            .max_by_key(|(_, bucket)| {
                line.end
                    .min(bucket.end)
                    .saturating_sub(line.start.max(bucket.start))
            })
            .map(|(index, _)| index);

        if let Some(index) = bucket_index {
            let bucket = &mut buckets[index];
            bucket.position_sum += u64::from(line.position);
            bucket.samples += 1;
            bucket.start = bucket.start.min(line.start);
            bucket.end = bucket.end.max(line.end);
            bucket.last_position = line.position;
        } else {
            buckets.push(LineBucket {
                position_sum: u64::from(line.position),
                samples: 1,
                start: line.start,
                end: line.end,
                last_position: line.position,
            });
        }
    }

    buckets
        .into_iter()
        .map(|bucket| GridLine {
            position: bucket.position_sum as f64 / f64::from(bucket.samples),
            start: f64::from(bucket.start),
            end: f64::from(bucket.end),
        })
        .collect()
}

fn ranges_touch_or_overlap(a_start: u32, a_end: u32, b_start: u32, b_end: u32, gap: u32) -> bool {
    a_start <= b_end.saturating_add(gap) && b_start <= a_end.saturating_add(gap)
}

fn collect_candidate_cells(
    x_positions: &[f64],
    y_positions: &[f64],
    horizontal: &[GridLine],
    vertical: &[GridLine],
    config: &GridDetectionConfig,
) -> Vec<CandidateCell> {
    // A single printed rule may shift by several raster pixels between
    // separate table regions. Axis canonicalisation retains every segment,
    // so boundary checks must accept any segment belonging to that canonical
    // axis instead of requiring the first representative coordinate exactly.
    let tolerance = f64::from(
        config.intersection_tolerance.saturating_add(
            config
                .min_cell_width
                .max(config.min_cell_height)
                .div_ceil(2),
        ),
    );
    let mut cells = Vec::new();

    for y_pair in y_positions.windows(2) {
        let (y0, y1) = (y_pair[0], y_pair[1]);
        if y1 - y0 < f64::from(config.min_cell_height) {
            continue;
        }
        // A page can contain multiple tables with near-aligned columns. Use
        // only vertical axes that actually cover this y-band, otherwise a
        // line from a later table can incorrectly block an upper-table cell.
        let active_x_positions: Vec<f64> = x_positions
            .iter()
            .copied()
            .filter(|&x| has_vertical_boundary(vertical, x, y0, y1, tolerance))
            .collect();
        for x_pair in active_x_positions.windows(2) {
            let (x0, x1) = (x_pair[0], x_pair[1]);
            if x1 - x0 < f64::from(config.min_cell_width) {
                continue;
            }
            if has_horizontal_boundary(horizontal, y0, x0, x1, tolerance)
                && has_horizontal_boundary(horizontal, y1, x0, x1, tolerance)
            {
                cells.push(CandidateCell {
                    bbox: BoundingBox::new(x0, y0, x1, y1),
                });
            }
        }
    }

    cells
}

fn grids_from_candidate_cells(
    candidates: &[CandidateCell],
    image_width: u32,
    image_height: u32,
    config: &GridDetectionConfig,
) -> Vec<TableGrid> {
    let mut visited = vec![false; candidates.len()];
    let mut grids = Vec::new();
    for start in 0..candidates.len() {
        if visited[start] {
            continue;
        }

        let mut queue = VecDeque::from([start]);
        let mut component = Vec::new();
        while let Some(index) = queue.pop_front() {
            if visited[index] {
                continue;
            }
            visited[index] = true;
            component.push(index);

            let cell = &candidates[index];
            for (neighbor, other) in candidates.iter().enumerate() {
                if !visited[neighbor] && cells_share_an_edge(cell, other) {
                    queue.push_back(neighbor);
                }
            }
        }

        if component.len() < 4 {
            continue;
        }
        if let Some(grid) =
            table_from_cell_component(&component, candidates, image_width, image_height, config)
        {
            grids.push(grid);
        }
    }

    grids
}

fn cells_share_an_edge(left: &CandidateCell, right: &CandidateCell) -> bool {
    const EDGE_TOLERANCE: f64 = 0.01;
    let left_box = &left.bbox;
    let right_box = &right.bbox;
    let same_vertical_span = (left_box.y0 - right_box.y0).abs() < EDGE_TOLERANCE
        && (left_box.y1 - right_box.y1).abs() < EDGE_TOLERANCE;
    let same_horizontal_span = (left_box.x0 - right_box.x0).abs() < EDGE_TOLERANCE
        && (left_box.x1 - right_box.x1).abs() < EDGE_TOLERANCE;

    (same_vertical_span
        && ((left_box.x1 - right_box.x0).abs() < EDGE_TOLERANCE
            || (right_box.x1 - left_box.x0).abs() < EDGE_TOLERANCE))
        || (same_horizontal_span
            && ((left_box.y1 - right_box.y0).abs() < EDGE_TOLERANCE
                || (right_box.y1 - left_box.y0).abs() < EDGE_TOLERANCE))
}

fn table_from_cell_component(
    component: &[usize],
    candidates: &[CandidateCell],
    image_width: u32,
    image_height: u32,
    _config: &GridDetectionConfig,
) -> Option<TableGrid> {
    let mut xs = Vec::with_capacity(component.len() * 2);
    let mut ys = Vec::with_capacity(component.len() * 2);
    for &index in component {
        let bbox = &candidates[index].bbox;
        xs.extend([bbox.x0, bbox.x1]);
        ys.extend([bbox.y0, bbox.y1]);
    }
    deduplicate_positions(&mut xs);
    deduplicate_positions(&mut ys);
    // A one-dimensional ruled strip is not a table. Reject it here rather
    // than letting a page frame or a single numbered column consume one of
    // the limited review slots.
    if xs.len() < 3 || ys.len() < 3 {
        return None;
    }

    let bbox = BoundingBox::new(xs[0], ys[0], *xs.last()?, *ys.last()?);
    if is_page_border(&bbox, image_width, image_height) {
        return None;
    }

    let mut cells = Vec::with_capacity(component.len());
    for &index in component {
        let candidate = &candidates[index];
        let row = position_index(&ys, candidate.bbox.y0)?;
        let column = position_index(&xs, candidate.bbox.x0)?;
        // Span inference is intentionally conservative in v1: a cell exists
        // only when all four borders were observed. Missing internal borders
        // are not merged unless a dedicated pass can prove their extent.
        cells.push(TableCell::grid(
            row as u32,
            column as u32,
            candidate.bbox.clone(),
        ));
    }
    cells.sort_by_key(|cell| (cell.row, cell.column));

    let possible_cells = (xs.len() - 1) * (ys.len() - 1);
    let cell_coverage = cells.len() as f32 / possible_cells.max(1) as f32;
    // Sparse technical diagrams can have many orthogonal strokes but are not
    // dense ruled tables. Requiring a substantial fraction of verified cells
    // keeps this first detector limited to document tables.
    if cell_coverage < 0.45 {
        return None;
    }

    let mut intersections = Vec::with_capacity(cells.len() * 4);
    for cell in &cells {
        let bbox = cell.bbox.as_ref()?;
        intersections.extend([
            GridIntersection {
                x: bbox.x0,
                y: bbox.y0,
            },
            GridIntersection {
                x: bbox.x1,
                y: bbox.y0,
            },
            GridIntersection {
                x: bbox.x0,
                y: bbox.y1,
            },
            GridIntersection {
                x: bbox.x1,
                y: bbox.y1,
            },
        ]);
    }
    intersections.sort_by(|left, right| {
        left.y
            .total_cmp(&right.y)
            .then_with(|| left.x.total_cmp(&right.x))
    });
    intersections
        .dedup_by(|left, right| (left.x - right.x).abs() < 0.01 && (left.y - right.y).abs() < 0.01);

    let possible_intersections = xs.len() * ys.len();
    let intersection_density = intersections.len() as f32 / possible_intersections.max(1) as f32;
    let line_score = ((xs.len() + ys.len()) as f32 / 8.0).min(1.0);
    let confidence =
        (0.5 * cell_coverage + 0.35 * intersection_density + 0.15 * line_score).clamp(0.0, 1.0);

    Some(TableGrid {
        bbox: bbox.clone(),
        horizontal_lines: ys
            .into_iter()
            .map(|position| GridLine {
                position,
                start: bbox.x0,
                end: bbox.x1,
            })
            .collect(),
        vertical_lines: xs
            .into_iter()
            .map(|position| GridLine {
                position,
                start: bbox.y0,
                end: bbox.y1,
            })
            .collect(),
        intersections,
        cells,
        confidence,
    })
}

fn deduplicate_positions(positions: &mut Vec<f64>) {
    positions.sort_by(|left, right| left.total_cmp(right));
    positions.dedup_by(|left, right| (*left - *right).abs() < 0.01);
}

fn position_index(positions: &[f64], value: f64) -> Option<usize> {
    positions
        .iter()
        .position(|position| (*position - value).abs() < 0.01)
}

fn stable_positions(mut positions: Vec<f64>, min_spacing: f64) -> Vec<f64> {
    positions.sort_by(|left, right| left.total_cmp(right));
    let mut stable: Vec<(f64, u32, f64)> = Vec::new();
    for position in positions {
        if let Some((sum, count, last)) = stable.last_mut() {
            if (position - *last).abs() < min_spacing {
                *sum += position;
                *count += 1;
                *last = position;
                continue;
            }
        }
        stable.push((position, 1, position));
    }
    stable
        .into_iter()
        .map(|(sum, count, _)| sum / f64::from(count))
        .collect()
}

fn has_horizontal_boundary(lines: &[GridLine], y: f64, x0: f64, x1: f64, tolerance: f64) -> bool {
    lines.iter().any(|line| {
        (line.position - y).abs() <= tolerance
            && line.start <= x0 + tolerance
            && line.end >= x1 - tolerance
    })
}

fn has_vertical_boundary(lines: &[GridLine], x: f64, y0: f64, y1: f64, tolerance: f64) -> bool {
    lines.iter().any(|line| {
        (line.position - x).abs() <= tolerance
            && line.start <= y0 + tolerance
            && line.end >= y1 - tolerance
    })
}

fn is_page_border(bbox: &BoundingBox, width: u32, height: u32) -> bool {
    let margin_x = (f64::from(width) * 0.03).max(8.0);
    let margin_y = (f64::from(height) * 0.03).max(8.0);
    bbox.x0 <= margin_x
        && bbox.y0 <= margin_y
        && bbox.x1 >= f64::from(width) - margin_x
        && bbox.y1 >= f64::from(height) - margin_y
}

fn boxes_overlap(left: &BoundingBox, right: &BoundingBox) -> bool {
    left.x0 < right.x1 && left.x1 > right.x0 && left.y0 < right.y1 && left.y1 > right.y0
}

fn intersection_area(left: &BoundingBox, right: &BoundingBox) -> f64 {
    let width = (left.x1.min(right.x1) - left.x0.max(right.x0)).max(0.0);
    let height = (left.y1.min(right.y1) - left.y0.max(right.y0)).max(0.0);
    width * height
}

fn box_center(bbox: &BoundingBox) -> (f64, f64) {
    ((bbox.x0 + bbox.x1) / 2.0, (bbox.y0 + bbox.y1) / 2.0)
}

fn point_in_box(x: f64, y: f64, bbox: &BoundingBox) -> bool {
    x >= bbox.x0 && x <= bbox.x1 && y >= bbox.y0 && y <= bbox.y1
}

fn words_to_cell_text(words: &[&Word]) -> String {
    let mut sorted = words.to_vec();
    sorted.sort_by(|left, right| {
        left.bbox
            .y0
            .total_cmp(&right.bbox.y0)
            .then_with(|| left.bbox.x0.total_cmp(&right.bbox.x0))
    });

    let mut heights: Vec<f64> = sorted
        .iter()
        .map(|word| (word.bbox.y1 - word.bbox.y0).max(1.0))
        .collect();
    heights.sort_by(|left, right| left.total_cmp(right));
    let median_height = heights[heights.len() / 2];
    let line_tolerance = (median_height * 0.6).max(3.0);

    let mut lines: Vec<(f64, Vec<&Word>)> = Vec::new();
    for word in sorted {
        if lines
            .last()
            .is_some_and(|(anchor, _)| (word.bbox.y0 - *anchor).abs() <= line_tolerance)
        {
            lines.last_mut().expect("line exists").1.push(word);
        } else {
            lines.push((word.bbox.y0, vec![word]));
        }
    }

    lines
        .into_iter()
        .map(|(_, mut line)| {
            line.sort_by(|left, right| left.bbox.x0.total_cmp(&right.bbox.x0));
            line_words_to_cell_text(&line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Repairs only strongly evidenced letter fragmentation inside one table-cell
/// line. The canonical page text is deliberately untouched: corpus A/B showed
/// that applying this transformation document-wide damages legitimate labels.
fn line_words_to_cell_text(line: &[&Word]) -> String {
    let mut parts = Vec::with_capacity(line.len());
    let mut index = 0usize;
    while index < line.len() {
        let Some(script) = single_supported_letter_script(&line[index].text) else {
            parts.push(line[index].text.clone());
            index += 1;
            continue;
        };
        let run_start = index;
        index += 1;
        while index < line.len()
            && single_supported_letter_script(&line[index].text) == Some(script)
        {
            index += 1;
        }
        let run = &line[run_start..index];
        if run.len() >= 4 {
            if let Some(joined) = join_fragmented_letter_run(run) {
                parts.push(joined);
                continue;
            }
        }
        parts.extend(run.iter().map(|word| word.text.clone()));
    }
    parts.join(" ")
}

fn single_supported_letter_script(text: &str) -> Option<u8> {
    let mut characters = text.chars();
    let character = characters.next()?;
    if characters.next().is_some() {
        return None;
    }
    if is_cyrillic(character) {
        Some(1)
    } else if is_latin(character) {
        Some(2)
    } else {
        None
    }
}

fn join_fragmented_letter_run(run: &[&Word]) -> Option<String> {
    let mut widths = Vec::with_capacity(run.len());
    let mut gaps = Vec::with_capacity(run.len().saturating_sub(1));
    for word in run {
        let width = word.bbox.x1 - word.bbox.x0;
        if !width.is_finite() || width <= 0.0 {
            return None;
        }
        widths.push(width);
    }
    for pair in run.windows(2) {
        let gap = (pair[1].bbox.x0 - pair[0].bbox.x1).max(0.0);
        if !gap.is_finite() {
            return None;
        }
        gaps.push(gap);
    }
    let median_width = median_f64(&mut widths)?;
    let mut positive_gaps = gaps
        .iter()
        .copied()
        .filter(|gap| *gap > 0.0)
        .collect::<Vec<_>>();
    let median_gap = median_f64(&mut positive_gaps);

    let mut output = String::with_capacity(run.len() * 2);
    output.push_str(&run[0].text);
    for (index, word) in run.iter().enumerate().skip(1) {
        let gap = gaps[index - 1];
        let preserves_word_boundary =
            gap > median_width * 0.5 || median_gap.is_some_and(|median| gap > median * 3.0);
        if preserves_word_boundary {
            output.push(' ');
        }
        output.push_str(&word.text);
    }
    Some(output)
}

#[expect(
    clippy::manual_is_multiple_of,
    reason = "Preserve the accepted OCR baseline during source-only publication."
)]
fn median_f64(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    let middle = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_text_repairs_same_script_letter_runs_and_preserves_large_gaps() {
        let letters = ["M", "e", "a", "n", "W", "e", "l", "l"];
        let x = [0.0, 10.0, 20.0, 30.0, 50.0, 60.0, 70.0, 80.0];
        let words = letters
            .iter()
            .zip(x)
            .map(|(text, x0)| Word::new(*text, BoundingBox::new(x0, 10.0, x0 + 8.0, 20.0), 0.9))
            .collect::<Vec<_>>();
        assert_eq!(
            line_words_to_cell_text(&words.iter().collect::<Vec<_>>()),
            "Mean Well"
        );

        let cyrillic = ["Б", "е", "з", "г"]
            .iter()
            .enumerate()
            .map(|(index, text)| {
                let x0 = index as f64 * 10.0;
                Word::new(*text, BoundingBox::new(x0, 10.0, x0 + 8.0, 20.0), 0.9)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            line_words_to_cell_text(&cyrillic.iter().collect::<Vec<_>>()),
            "Безг"
        );
    }

    #[test]
    #[expect(
        clippy::useless_vec,
        reason = "Preserve the accepted OCR baseline during source-only publication."
    )]
    fn cell_text_does_not_join_short_mixed_or_cross_line_runs() {
        let make_line = |letters: &[&str]| {
            letters
                .iter()
                .enumerate()
                .map(|(index, text)| {
                    let x0 = index as f64 * 10.0;
                    Word::new(*text, BoundingBox::new(x0, 10.0, x0 + 8.0, 20.0), 0.9)
                })
                .collect::<Vec<_>>()
        };
        let short = make_line(&["A", "B", "C"]);
        assert_eq!(
            line_words_to_cell_text(&short.iter().collect::<Vec<_>>()),
            "A B C"
        );

        let mixed = make_line(&["M", "е", "a", "n"]);
        assert_eq!(
            line_words_to_cell_text(&mixed.iter().collect::<Vec<_>>()),
            "M е a n"
        );

        let cross_line = vec![
            Word::new("M", BoundingBox::new(0.0, 10.0, 8.0, 20.0), 0.9),
            Word::new("e", BoundingBox::new(10.0, 10.0, 18.0, 20.0), 0.9),
            Word::new("a", BoundingBox::new(0.0, 30.0, 8.0, 40.0), 0.9),
            Word::new("n", BoundingBox::new(10.0, 30.0, 18.0, 40.0), 0.9),
        ];
        assert_eq!(
            words_to_cell_text(&cross_line.iter().collect::<Vec<_>>()),
            "M e\na n"
        );
    }

    fn ruled_grid() -> GrayImage {
        let mut image = GrayImage::from_pixel(200, 100, Luma([255]));
        for &y in &[10, 50, 90] {
            for x in 20..181 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        for &x in &[20, 100, 180] {
            for y in 10..91 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        image
    }

    fn ruled_grid_with_dimensions(rows: u32, columns: u32) -> GrayImage {
        let spacing = 40;
        let left = 20;
        let top = 20;
        let width = left * 2 + (columns + 1) * spacing;
        let height = top * 2 + (rows + 1) * spacing;
        let mut image = GrayImage::from_pixel(width, height, Luma([255]));

        for row in 0..=rows {
            let y = top + row * spacing;
            for x in left..=(width - left - 1) {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        for column in 0..=columns {
            let x = left + column * spacing;
            for y in top..=(height - top - 1) {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        image
    }

    #[test]
    fn detects_cells_from_a_ruled_grid() {
        let detection = detect_table_grids(&ruled_grid());
        let grid = detection
            .grids
            .iter()
            .find(|grid| grid.rows() == 2 && grid.columns() == 2)
            .expect("2x2 grid should be detected");

        assert_eq!(grid.cells.len(), 4);
        assert!(grid.confidence > 0.5);
    }

    #[test]
    fn detects_11_by_3_grid_regression_fixture() {
        let detection = detect_table_grids(&ruled_grid_with_dimensions(11, 3));
        assert!(
            detection
                .grids
                .iter()
                .any(|grid| grid.rows() == 11 && grid.columns() == 3),
            "11x3 ruled grid should be reconstructed"
        );
    }

    #[test]
    fn detects_3_by_3_grid_regression_fixture() {
        let detection = detect_table_grids(&ruled_grid_with_dimensions(3, 3));
        assert!(
            detection
                .grids
                .iter()
                .any(|grid| grid.rows() == 3 && grid.columns() == 3),
            "3x3 ruled grid should be reconstructed"
        );
    }

    #[test]
    fn detects_2_by_3_grid_regression_fixture() {
        let detection = detect_table_grids(&ruled_grid_with_dimensions(2, 3));
        assert!(
            detection
                .grids
                .iter()
                .any(|grid| grid.rows() == 2 && grid.columns() == 3),
            "2x3 ruled grid should be reconstructed"
        );
    }

    #[test]
    fn assigns_each_word_to_one_cell() {
        let mut detection = detect_table_grids(&ruled_grid());
        let grid = detection
            .grids
            .iter_mut()
            .find(|grid| grid.rows() == 2 && grid.columns() == 2)
            .expect("2x2 grid should be detected");
        let words = vec![
            Word::new("north", BoundingBox::new(30.0, 20.0, 70.0, 35.0), 0.9),
            Word::new("east", BoundingBox::new(120.0, 20.0, 150.0, 35.0), 0.8),
        ];

        let metrics = grid.assign_words(&words);
        assert_eq!(metrics.ocr_words_total, 2);
        assert_eq!(metrics.words_assigned, 2);
        assert_eq!(metrics.duplicate_assignments, 0);
        assert_eq!(grid.to_matrix().rows[0][0].text, "north");
        assert_eq!(grid.to_matrix().rows[0][1].text, "east");
    }

    #[test]
    fn rejects_a_full_page_border() {
        let mut image = GrayImage::from_pixel(200, 100, Luma([255]));
        for &y in &[0, 99] {
            for x in 0..200 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        for &x in &[0, 199] {
            for y in 0..100 {
                image.put_pixel(x, y, Luma([0]));
            }
        }

        assert!(detect_table_grids(&image).grids.is_empty());
    }

    #[test]
    fn sliding_threshold_matches_imageproc_on_small_images() {
        let image = GrayImage::from_fn(9, 7, |x, y| {
            let value = ((x * 37 + y * 19) % 256) as u8;
            Luma([value])
        });

        assert_eq!(
            adaptive_threshold_sliding(&image, 2, 10),
            adaptive_threshold(&image, 2, 10)
        );
    }

    #[test]
    fn separates_independent_ruled_tables() {
        let mut image = GrayImage::from_pixel(500, 250, Luma([255]));
        for &y in &[20, 100, 180] {
            for x in 20..181 {
                image.put_pixel(x, y, Luma([0]));
            }
            for x in 300..461 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        for &x in &[20, 100, 180, 300, 380, 460] {
            for y in 20..181 {
                image.put_pixel(x, y, Luma([0]));
            }
        }

        let mut dimensions: Vec<(u32, u32)> = detect_table_grids(&image)
            .grids
            .iter()
            .map(|grid| (grid.rows(), grid.columns()))
            .collect();
        dimensions.sort_unstable();
        assert_eq!(dimensions, vec![(2, 2), (2, 2)]);
    }

    #[test]
    #[expect(
        clippy::needless_borrow,
        reason = "Preserve the accepted OCR baseline during source-only publication."
    )]
    fn keeps_sparse_grid_away_from_drawing_corner_as_data_table() {
        let mut detection = detect_table_grids(&ruled_grid());
        let grid = detection
            .grids
            .iter_mut()
            .find(|grid| grid.rows() == 2 && grid.columns() == 2)
            .expect("2x2 grid should be detected");

        let empty_metrics = grid.assign_words(&[]);
        assert_eq!(
            classify_grid(&grid, &empty_metrics, 200, 100),
            TableType::DataTable
        );
    }

    #[test]
    fn classifies_compact_lower_right_grid_as_title_block() {
        let grid = TableGrid {
            bbox: BoundingBox::new(60.0, 75.0, 90.0, 95.0),
            horizontal_lines: (0..12)
                .map(|index| GridLine {
                    position: 75.0 + f64::from(index) * (20.0 / 11.0),
                    start: 60.0,
                    end: 90.0,
                })
                .collect(),
            vertical_lines: (0..7)
                .map(|index| GridLine {
                    position: 60.0 + f64::from(index) * 5.0,
                    start: 75.0,
                    end: 95.0,
                })
                .collect(),
            intersections: Vec::new(),
            cells: Vec::new(),
            confidence: 1.0,
        };
        let metrics = TableAssignmentMetrics {
            table_bbox: grid.bbox.clone(),
            rows: 11,
            columns: 6,
            cells: 66,
            empty_cells: 0,
            ocr_words_total: 66,
            words_assigned: 66,
            words_unassigned: 0,
            duplicate_assignments: 0,
            detector_confidence: 1.0,
        };

        assert_eq!(
            classify_grid(&grid, &metrics, 100, 100),
            TableType::TitleBlock
        );

        let sparse_metrics = TableAssignmentMetrics {
            empty_cells: 66,
            ocr_words_total: 0,
            words_assigned: 0,
            words_unassigned: 0,
            ..metrics
        };
        assert_eq!(
            classify_grid(&grid, &sparse_metrics, 100, 100),
            TableType::TitleBlock
        );
    }
    #[test]
    fn matrix_keeps_coordinates_for_missing_physical_cell() {
        let mut cells = Vec::new();
        for row in 0..3 {
            for column in 0..3 {
                if (row, column) == (2, 1) {
                    continue;
                }
                let mut cell = TableCell::grid(
                    row,
                    column,
                    BoundingBox::new(
                        f64::from(column) * 10.0,
                        f64::from(row) * 10.0,
                        f64::from(column + 1) * 10.0,
                        f64::from(row + 1) * 10.0,
                    ),
                );
                cell.text = format!("{row},{column}");
                cell.confidence = 0.8;
                cells.push(cell);
            }
        }
        let grid = TableGrid {
            bbox: BoundingBox::new(0.0, 0.0, 30.0, 30.0),
            horizontal_lines: (0..=3)
                .map(|index| GridLine {
                    position: f64::from(index) * 10.0,
                    start: 0.0,
                    end: 30.0,
                })
                .collect(),
            vertical_lines: (0..=3)
                .map(|index| GridLine {
                    position: f64::from(index) * 10.0,
                    start: 0.0,
                    end: 30.0,
                })
                .collect(),
            intersections: Vec::new(),
            cells,
            confidence: 0.9,
        };

        let matrix = grid.to_matrix();
        assert_eq!(matrix.rows.len(), 3);
        assert!(matrix.rows.iter().all(|row| row.len() == 3));
        let missing = &matrix.rows[2][1];
        assert_eq!((missing.row, missing.column), (2, 1));
        assert_eq!((missing.row_span, missing.col_span), (1, 1));
        assert!(missing.bbox.is_none());
        assert!(missing.text.is_empty());

        for row in 0..3 {
            for column in 0..3 {
                let matrix_cell = &matrix.rows[row as usize][column as usize];
                if (row, column) == (2, 1) {
                    continue;
                }
                let physical = grid
                    .cells
                    .iter()
                    .find(|cell| cell.row == row && cell.column == column)
                    .expect("physical cell");
                assert_eq!(matrix_cell.row, physical.row);
                assert_eq!(matrix_cell.column, physical.column);
                assert_eq!(matrix_cell.row_span, physical.row_span);
                assert_eq!(matrix_cell.col_span, physical.col_span);
                assert_eq!(matrix_cell.text, physical.text);
                assert_eq!(matrix_cell.confidence, physical.confidence);
                let matrix_bbox = matrix_cell.bbox.as_ref().expect("physical geometry");
                let physical_bbox = physical.bbox.as_ref().expect("source geometry");
                assert_eq!(matrix_bbox.x0, physical_bbox.x0);
                assert_eq!(matrix_bbox.y0, physical_bbox.y0);
                assert_eq!(matrix_bbox.x1, physical_bbox.x1);
                assert_eq!(matrix_bbox.y1, physical_bbox.y1);
            }
        }
    }
}
