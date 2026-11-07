//! Private, source-backed precision for already detected raster cell edges.
//! No bitmap is allocated and no new table or unobserved edge is introduced.

use pdfium_render::prelude::*;

use super::PdfWordBounds;
use crate::dto::BoundingBox;

const DEVICE_SCALE: u32 = 1024;
const SNAP_LIMIT: f64 = 1.0;
const SAME_AXIS_TOLERANCE: f64 = 0.01;
const AXIS_PRECISION: f64 = 1.0 / DEVICE_SCALE as f64;
// Optional source evidence must not create an unbounded second path walk on
// adversarial drawing files. Exhaustion leaves every raster edge unchanged.
const MAX_REFINEMENT_OBJECTS: usize = 50_000;
const MAX_REFINEMENT_SEGMENTS: usize = 100_000;

fn device_dimensions(width: u32, height: u32, scale: u32) -> Option<(i32, i32)> {
    if width == 0 || height == 0 {
        return None;
    }
    Some((
        i32::try_from(width.checked_mul(scale)?).ok()?,
        i32::try_from(height.checked_mul(scale)?).ok()?,
    ))
}

fn device_point(
    page: &PdfPage<'_>,
    point: (f32, f32),
    width: u32,
    height: u32,
    scale: u32,
) -> Option<(f64, f64)> {
    if !point.0.is_finite() || !point.1.is_finite() {
        return None;
    }
    let (virtual_width, virtual_height) = device_dimensions(width, height, scale)?;
    let config = PdfRenderConfig::new().set_fixed_size(virtual_width, virtual_height);
    let (x, y) = page
        .points_to_pixels(PdfPoints::new(point.0), PdfPoints::new(point.1), &config)
        .ok()?;
    Some((
        f64::from(x) / f64::from(scale),
        f64::from(y) / f64::from(scale),
    ))
}

fn project_bounds(
    page: &PdfPage<'_>,
    bounds: PdfWordBounds,
    width: u32,
    height: u32,
    scale: u32,
) -> Option<BoundingBox> {
    if !bounds.is_valid() {
        return None;
    }
    let mut bbox = BoundingBox::new(
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for corner in [
        (bounds.left, bounds.bottom),
        (bounds.left, bounds.top),
        (bounds.right, bounds.bottom),
        (bounds.right, bounds.top),
    ] {
        let (x, y) = device_point(page, corner, width, height, scale)?;
        bbox.x0 = bbox.x0.min(x);
        bbox.y0 = bbox.y0.min(y);
        bbox.x1 = bbox.x1.max(x);
        bbox.y1 = bbox.y1.max(y);
    }
    bbox.x0 = bbox.x0.clamp(0.0, f64::from(width));
    bbox.x1 = bbox.x1.clamp(0.0, f64::from(width));
    bbox.y0 = bbox.y0.clamp(0.0, f64::from(height));
    bbox.y1 = bbox.y1.clamp(0.0, f64::from(height));
    valid_bounds(&bbox).then_some(bbox)
}

/// Projects through PDFium's own page transform at subpixel precision. The
/// virtual dimensions affect coordinate conversion only, never rasterization.
/// If precision is unavailable, use the ordinary projection as a whole.
pub(super) fn precise_device_bounds(
    page: &PdfPage<'_>,
    bounds: PdfWordBounds,
    width: u32,
    height: u32,
) -> Option<BoundingBox> {
    project_bounds(page, bounds, width, height, DEVICE_SCALE)
        .or_else(|| project_bounds(page, bounds, width, height, 1))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug)]
struct AxisLine {
    axis: Axis,
    position: f64,
    start: f64,
    end: f64,
}

#[derive(Clone, Copy)]
enum VertexKind {
    Move,
    Line,
    Unsupported,
}

#[derive(Clone, Copy)]
struct Vertex {
    kind: VertexKind,
    point: (f64, f64),
    closes: bool,
}

fn axis_line(from: (f64, f64), to: (f64, f64)) -> Option<AxisLine> {
    if ![from.0, from.1, to.0, to.1].into_iter().all(f64::is_finite) {
        return None;
    }
    let dx = (to.0 - from.0).abs();
    let dy = (to.1 - from.1).abs();
    if dx <= AXIS_PRECISION && dy > AXIS_PRECISION {
        Some(AxisLine {
            axis: Axis::Vertical,
            position: (from.0 + to.0) / 2.0,
            start: from.1.min(to.1),
            end: from.1.max(to.1),
        })
    } else if dy <= AXIS_PRECISION && dx > AXIS_PRECISION {
        Some(AxisLine {
            axis: Axis::Horizontal,
            position: (from.1 + to.1) / 2.0,
            start: from.0.min(to.0),
            end: from.0.max(to.0),
        })
    } else {
        None
    }
}

fn straight_subpath_lines(vertices: impl IntoIterator<Item = Vertex>) -> Vec<AxisLine> {
    let mut lines = Vec::new();
    let mut current = None;
    let mut start = None;
    for vertex in vertices {
        match vertex.kind {
            VertexKind::Move => {
                current = Some(vertex.point);
                start = Some(vertex.point);
            }
            VertexKind::Line => {
                if let Some(previous) = current {
                    lines.extend(axis_line(previous, vertex.point));
                }
                current = Some(vertex.point);
            }
            VertexKind::Unsupported => {
                // Never turn Bezier control points into straight chords or
                // infer a closing line across a curved/unknown sequence.
                current = None;
                start = None;
            }
        }
        if vertex.closes {
            if let (Some(previous), Some(first)) = (current, start) {
                lines.extend(axis_line(previous, first));
            }
            current = None;
            start = None;
        }
    }
    lines
}

fn source_lines(page: &PdfPage<'_>, width: u32, height: u32) -> Vec<AxisLine> {
    let mut lines = Vec::new();
    let mut segment_count = 0usize;
    // Nested form objects are intentionally not traversed. Unsupported or
    // failed source evidence simply leaves the detected raster edge intact.
    for (object_index, object) in page.objects().iter().enumerate() {
        if object_index >= MAX_REFINEMENT_OBJECTS {
            return Vec::new();
        }
        let Some(path) = object.as_path_object() else {
            continue;
        };
        if !path.is_stroked().unwrap_or(false) {
            continue;
        }
        let Ok(matrix) = path.matrix() else {
            continue;
        };
        let segments = path.segments().transform(matrix);
        let mut vertices = Vec::new();
        let mut failed = false;
        for segment in segments.iter() {
            segment_count += 1;
            if segment_count > MAX_REFINEMENT_SEGMENTS {
                return Vec::new();
            }
            let (x, y) = segment.point();
            let Some(point) = device_point(page, (x.value, y.value), width, height, DEVICE_SCALE)
            else {
                failed = true;
                break;
            };
            let kind = match segment.segment_type() {
                PdfPathSegmentType::MoveTo => VertexKind::Move,
                PdfPathSegmentType::LineTo => VertexKind::Line,
                _ => VertexKind::Unsupported,
            };
            vertices.push(Vertex {
                kind,
                point,
                closes: segment.is_close(),
            });
        }
        if !failed {
            lines.extend(straight_subpath_lines(vertices));
        }
    }
    lines
}

fn snapped_axis(lines: &[AxisLine], axis: Axis, original: f64, start: f64, end: f64) -> f64 {
    let mut positions = lines
        .iter()
        .filter(|line| {
            line.axis == axis
                && (line.position - original).abs() <= SNAP_LIMIT
                && line.start <= start + SNAP_LIMIT
                && line.end >= end - SNAP_LIMIT
        })
        .map(|line| line.position)
        .collect::<Vec<_>>();
    positions.sort_by(f64::total_cmp);
    // Repeated collinear paths are the same source edge, not competing edges.
    positions.dedup_by(|a, b| (*a - *b).abs() <= SAME_AXIS_TOLERANCE);
    positions.sort_by(|a, b| (a - original).abs().total_cmp(&(b - original).abs()));
    let Some(&nearest) = positions.first() else {
        return original;
    };
    if positions.get(1).is_some_and(|other| {
        ((other - original).abs() - (nearest - original).abs()).abs() <= SAME_AXIS_TOLERANCE
    }) {
        original
    } else {
        nearest
    }
}

fn valid_bounds(bbox: &BoundingBox) -> bool {
    [bbox.x0, bbox.y0, bbox.x1, bbox.y1]
        .into_iter()
        .all(f64::is_finite)
        && bbox.x1 > bbox.x0
        && bbox.y1 > bbox.y0
}

fn refine_with_lines(cells: &[BoundingBox], lines: &[AxisLine]) -> Vec<BoundingBox> {
    cells
        .iter()
        .map(|cell| {
            if !valid_bounds(cell) {
                return cell.clone();
            }
            let candidate = BoundingBox::new(
                snapped_axis(lines, Axis::Vertical, cell.x0, cell.y0, cell.y1),
                snapped_axis(lines, Axis::Horizontal, cell.y0, cell.x0, cell.x1),
                snapped_axis(lines, Axis::Vertical, cell.x1, cell.y0, cell.y1),
                snapped_axis(lines, Axis::Horizontal, cell.y1, cell.x0, cell.x1),
            );
            if valid_bounds(&candidate) {
                candidate
            } else {
                cell.clone()
            }
        })
        .collect()
}

/// Refines only already detected cell edges by at most one raster pixel, and
/// only from a unique, locally covering, straight stroked source segment.
pub(super) fn refine_cell_bounds(
    page: &PdfPage<'_>,
    cells: &[BoundingBox],
    width: u32,
    height: u32,
) -> Vec<BoundingBox> {
    if cells.is_empty() || device_dimensions(width, height, DEVICE_SCALE).is_none() {
        return cells.to_vec();
    }
    refine_with_lines(cells, &source_lines(page, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertical(position: f64, start: f64, end: f64) -> AxisLine {
        AxisLine {
            axis: Axis::Vertical,
            position,
            start,
            end,
        }
    }

    fn center_owner(glyph: &BoundingBox, cells: &[BoundingBox]) -> Option<usize> {
        let x = (glyph.x0 + glyph.x1) / 2.0;
        let y = (glyph.y0 + glyph.y1) / 2.0;
        cells
            .iter()
            .position(|cell| x >= cell.x0 && x <= cell.x1 && y >= cell.y0 && y <= cell.y1)
    }

    #[test]
    fn source_edge_separates_two_subpixel_neighbor_font_centers() {
        let cells = [
            BoundingBox::new(78.5, 297.5, 188.5, 369.5),
            BoundingBox::new(188.5, 297.5, 696.5, 369.5),
        ];
        let refined = refine_with_lines(&cells, &[vertical(188.949416, 297.6, 369.6)]);
        let left_glyph = BoundingBox::new(185.717, 322.026, 191.960, 334.254);
        let right_glyph = BoundingBox::new(186.016, 322.026, 191.960, 334.254);
        assert_eq!(center_owner(&left_glyph, &cells), Some(1));
        assert_eq!(center_owner(&left_glyph, &refined), Some(0));
        assert_eq!(center_owner(&right_glyph, &refined), Some(1));
        assert_eq!(refined[0].x1, refined[1].x0);
        assert_eq!(refined.len(), cells.len());
    }

    #[test]
    fn source_edge_requires_local_whole_edge_coverage() {
        let cell = BoundingBox::new(20.0, 30.0, 80.0, 130.0);
        let lines = [
            vertical(80.4, 200.0, 400.0),
            vertical(79.8, 40.0, 120.0),
            vertical(81.01, 30.0, 130.0),
        ];
        assert_eq!(
            refine_with_lines(std::slice::from_ref(&cell), &lines),
            [cell.clone()]
        );
        assert_eq!(
            refine_with_lines(&[cell], &[vertical(80.4, 30.8, 129.2)])[0].x1,
            80.4
        );
    }

    #[test]
    fn competing_source_edges_fall_back_but_duplicate_paths_do_not() {
        let cell = BoundingBox::new(20.0, 30.0, 80.0, 130.0);
        let ambiguous = [vertical(79.6, 20.0, 140.0), vertical(80.405, 20.0, 140.0)];
        assert_eq!(
            refine_with_lines(std::slice::from_ref(&cell), &ambiguous),
            [cell.clone()]
        );
        let repeated = [
            vertical(80.4, 20.0, 140.0),
            vertical(80.405, 20.0, 140.0),
            vertical(80.4, 20.0, 140.0),
        ];
        let refined = refine_with_lines(&[cell], &repeated);
        assert!((refined[0].x1 - 80.4).abs() <= SAME_AXIS_TOLERANCE);
    }

    #[test]
    fn curves_and_diagonals_cannot_supply_a_straight_edge() {
        assert!(axis_line((0.0, 0.0), (10.0, 10.0)).is_none());
        let lines = straight_subpath_lines([
            Vertex {
                kind: VertexKind::Move,
                point: (10.0, 10.0),
                closes: false,
            },
            Vertex {
                kind: VertexKind::Unsupported,
                point: (10.0, 20.0),
                closes: false,
            },
            Vertex {
                kind: VertexKind::Unsupported,
                point: (10.0, 30.0),
                closes: true,
            },
        ]);
        assert!(lines.is_empty());
    }

    #[test]
    fn explicit_closed_subpath_retains_its_closing_edge() {
        let lines = straight_subpath_lines([
            Vertex {
                kind: VertexKind::Move,
                point: (10.0, 10.0),
                closes: false,
            },
            Vertex {
                kind: VertexKind::Line,
                point: (40.0, 10.0),
                closes: false,
            },
            Vertex {
                kind: VertexKind::Line,
                point: (40.0, 50.0),
                closes: false,
            },
            Vertex {
                kind: VertexKind::Line,
                point: (10.0, 50.0),
                closes: true,
            },
        ]);
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().any(|line| line.axis == Axis::Vertical
            && line.position == 10.0
            && line.start == 10.0
            && line.end == 50.0));
    }

    #[test]
    fn invalid_or_collapsing_refinement_keeps_original_bounds() {
        let cell = BoundingBox::new(20.0, 30.0, 20.5, 130.0);
        // The same unique source edge would collapse both narrow raster edges.
        let lines = [vertical(20.25, 30.0, 130.0)];
        let output = refine_with_lines(std::slice::from_ref(&cell), &lines);
        assert_eq!(output, [cell]);
        let invalid = BoundingBox::new(f64::NAN, 1.0, 2.0, 3.0);
        assert!(refine_with_lines(&[invalid], &lines)[0].x0.is_nan());
    }

    #[test]
    fn virtual_dimensions_are_checked_without_allocating_pixels() {
        assert_eq!(
            device_dimensions(1654, 1170, DEVICE_SCALE),
            Some((1_693_696, 1_198_080))
        );
        assert_eq!(device_dimensions(u32::MAX, 1170, DEVICE_SCALE), None);
        assert_eq!(device_dimensions(2_097_152, 1170, DEVICE_SCALE), None);
        assert_eq!(
            device_dimensions(2_097_152, 1170, 1),
            Some((2_097_152, 1170))
        );
        assert_eq!(device_dimensions(0, 1170, 1), None);
    }
}
