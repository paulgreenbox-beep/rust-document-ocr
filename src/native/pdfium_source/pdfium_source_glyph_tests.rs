//! Synthetic glyph and grid regressions with invented text and geometry.

use super::*;
use crate::dto::TableCell;
use crate::table_grid::{GridLine, TableGrid};

fn glyph(value: char, x: f32, y: f32, width: f32) -> PdfTextGlyph {
    PdfTextGlyph {
        value,
        origin: Some((x, y + 2.0)),
        angle_radians: Some(0.0),
        bounds: Some(PdfWordBounds {
            left: x,
            bottom: y,
            right: x + width,
            top: y + 8.0,
        }),
        ink_bounds: None,
    }
}

fn separator(value: char) -> PdfTextGlyph {
    PdfTextGlyph {
        value,
        origin: None,
        angle_radians: None,
        bounds: None,
        ink_bounds: None,
    }
}

fn control_run(glyphs: &mut Vec<PdfTextGlyph>) {
    glyphs.extend([separator('\r'), separator('\n')]);
}

fn assert_unit_quantity_row(
    quantity: &str,
    unit_y: f32,
    pcs_y: f32,
    quantity_y: f32,
    quantity_glyphs: &[(f32, f32)],
    following_position: Option<(f32, f32)>,
) {
    // A page-level stacked-CAD context is necessary to reproduce the defect.
    // These overlapping/abutting baseline intervals must remain a single word.
    let mut glyphs = Vec::new();
    for index in 0..=8 {
        if index > 0 {
            control_run(&mut glyphs);
        }
        glyphs.push(glyph(
            char::from(b'A' + index),
            200.0 - f32::from(index) * 8.0,
            300.0 + f32::from(index) * 20.0,
            8.0,
        ));
    }
    glyphs.push(separator(' '));
    for (value, x, width) in [('ш', 252.0, 6.0), ('т', 258.0, 6.0), ('.', 264.0, 2.0)] {
        glyphs.push(glyph(value, x, unit_y, width));
    }
    control_run(&mut glyphs);
    for (value, x, width) in [
        ('p', 250.0, 5.0),
        ('c', 255.0, 4.0),
        ('s', 259.0, 5.0),
        ('.', 264.0, 2.0),
    ] {
        glyphs.push(glyph(value, x, pcs_y, width));
    }
    control_run(&mut glyphs);
    assert_eq!(quantity.chars().count(), quantity_glyphs.len());
    for (value, &(x, width)) in quantity.chars().zip(quantity_glyphs) {
        glyphs.push(glyph(value, x, quantity_y, width));
    }
    if let Some((x, y)) = following_position {
        control_run(&mut glyphs);
        glyphs.push(glyph('R', x, y, 5.0));
    }

    assert!(control_run_profile(&glyphs).stacked_backtracking);
    let canonical = format!(
        "ABCDEFGHI шт.pcs.{quantity}{}",
        if following_position.is_some() {
            "R"
        } else {
            ""
        }
    );
    assert_eq!(resolve_pdf_control_runs(&glyphs, false), canonical);
    let words = native_words_from_glyphs(&glyphs, false);
    // The bilingual unit occupies two real lines. Its positioned words may
    // split by geometry, but both must remain in the same unit cell.
    let mut expected = vec!["ABCDEFGHI", "шт.", "pcs.", quantity];
    if following_position.is_some() {
        expected.push("R");
    }
    assert_eq!(
        words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    // A geometry split may not discard, duplicate or reorder a native glyph.
    let source_values: String = glyphs
        .iter()
        .filter(|g| !g.value.is_whitespace())
        .map(|g| g.value)
        .collect();
    assert_eq!(
        words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<String>(),
        source_values
    );
    assert_eq!(resolve_pdf_control_runs(&glyphs, false), canonical);

    let lower_y = following_position.map_or(pcs_y - 20.0, |(_, y)| y - 20.0);
    let upper_y = unit_y + 20.0;
    // The source geometry is PDF bottom-up; assignment/reading order consume
    // rendered top-down coordinates, just as in the real page source.
    let pixel_box =
        |x0: f64, y0: f64, x1: f64, y1: f64| BoundingBox::new(x0, 480.0 - y1, x1, 480.0 - y0);
    let bbox = pixel_box(10.0, f64::from(lower_y), 365.0, f64::from(upper_y));
    let mut cells = vec![
        TableCell::grid(
            0,
            1,
            pixel_box(235.0, f64::from(pcs_y - 5.0), 290.0, f64::from(upper_y)),
        ),
        TableCell::grid(
            0,
            2,
            pixel_box(290.0, f64::from(pcs_y - 5.0), 365.0, f64::from(upper_y)),
        ),
    ];
    if let Some((_, y)) = following_position {
        cells.push(TableCell::grid(
            1,
            0,
            pixel_box(10.0, f64::from(y - 5.0), 235.0, f64::from(y + 15.0)),
        ));
    }
    let mut grid = TableGrid {
        bbox,
        horizontal_lines: [lower_y, upper_y]
            .into_iter()
            .rev()
            .map(|y| GridLine {
                position: 480.0 - f64::from(y),
                start: 10.0,
                end: 365.0,
            })
            .collect(),
        vertical_lines: [10.0, 235.0, 290.0, 365.0]
            .into_iter()
            .map(|x| GridLine {
                position: x,
                start: 480.0 - f64::from(upper_y),
                end: 480.0 - f64::from(lower_y),
            })
            .collect(),
        intersections: Vec::new(),
        cells,
        confidence: 1.0,
    };
    let positioned = words
        .iter()
        .filter_map(|word| {
            word.bounds.map(|b| Word {
                text: word.text.clone(),
                bbox: pixel_box(
                    f64::from(b.left),
                    f64::from(b.bottom),
                    f64::from(b.right),
                    f64::from(b.top),
                ),
                confidence: 1.0,
            })
        })
        .collect::<Vec<_>>();
    let metrics = grid.assign_words(&positioned);
    assert_eq!(
        grid.cells[0]
            .text
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>(),
        "шт.pcs."
    );
    assert_eq!(grid.cells[1].text, quantity);
    if following_position.is_some() {
        assert_eq!(grid.cells[2].text, "R");
    }
    assert_eq!(metrics.words_assigned as usize, expected.len() - 1);
    assert_eq!(metrics.words_unassigned, 0);
    assert_eq!(metrics.duplicate_assignments, 0);
}

#[test]
fn single_digit_is_separate_before_reverse_row_jump() {
    assert_unit_quantity_row(
        "7",
        220.0,
        206.0,
        213.0,
        &[(320.0, 5.0)],
        Some((30.0, 160.0)),
    );
}

#[test]
fn single_digit_is_separate_before_short_reverse_row_jump() {
    assert_unit_quantity_row(
        "8",
        145.0,
        131.0,
        138.0,
        &[(315.0, 5.0)],
        Some((210.0, 95.0)),
    );
}

#[test]
fn touching_digits_stay_together_across_column_transition() {
    assert_unit_quantity_row(
        "64",
        230.0,
        216.0,
        223.0,
        &[(322.0, 5.0), (327.0, 5.0)],
        Some((45.0, 170.0)),
    );
}

#[test]
fn next_row_marker_remains_at_left_edge() {
    assert_unit_quantity_row(
        "6",
        190.0,
        176.0,
        183.0,
        &[(325.0, 5.0)],
        Some((10.0, 100.0)),
    );
}

#[test]
fn terminal_digit_flushes_into_quantity_cell() {
    assert_unit_quantity_row("9", 90.0, 76.0, 83.0, &[(320.0, 5.0)], None);
}

#[test]
fn unequal_digit_widths_do_not_break_quantity() {
    assert_unit_quantity_row(
        "18",
        205.0,
        191.0,
        198.0,
        &[(318.0, 3.0), (321.0, 6.0)],
        Some((20.0, 125.0)),
    );
}

#[test]
fn wider_terminal_digit_stays_within_quantity_cell() {
    assert_unit_quantity_row("0", 120.0, 106.0, 113.0, &[(330.0, 6.0)], None);
}

fn rotate_glyph(mut glyph: PdfTextGlyph, angle: f32, scale: f32) -> PdfTextGlyph {
    let transform = |x: f32, y: f32| {
        (
            scale * (x * angle.cos() - y * angle.sin()),
            scale * (x * angle.sin() + y * angle.cos()),
        )
    };
    glyph.origin = glyph.origin.map(|(x, y)| transform(x, y));
    glyph.angle_radians = glyph.angle_radians.map(|current| current + angle);
    for target in [&mut glyph.bounds, &mut glyph.ink_bounds] {
        if let Some(bounds) = *target {
            let corners = [
                (bounds.left, bounds.bottom),
                (bounds.left, bounds.top),
                (bounds.right, bounds.bottom),
                (bounds.right, bounds.top),
            ]
            .map(|(x, y)| transform(x, y));
            *target = Some(PdfWordBounds {
                left: corners.iter().map(|p| p.0).fold(f32::INFINITY, f32::min),
                bottom: corners.iter().map(|p| p.1).fold(f32::INFINITY, f32::min),
                right: corners
                    .iter()
                    .map(|p| p.0)
                    .fold(f32::NEG_INFINITY, f32::max),
                top: corners
                    .iter()
                    .map(|p| p.1)
                    .fold(f32::NEG_INFINITY, f32::max),
            });
        }
    }
    glyph
}

#[test]
fn positioned_gap_guard_is_direction_and_scale_independent() {
    let previous = glyph('A', 10.0, 0.0, 8.0);
    for degrees in [0.0_f32, 90.0, 180.0, 270.0] {
        for scale in [0.5, 1.0, 4.0] {
            let angle = degrees.to_radians();
            let left = rotate_glyph(previous, angle, scale);
            for x in [-30.0, 50.0] {
                let right = rotate_glyph(glyph('B', x, -12.0, 8.0), angle, scale);
                assert!(
                    positioned_glyphs_are_disjoint(left, right),
                    "angle={degrees}, scale={scale}"
                );
                assert!(positioned_glyphs_are_disjoint(right, left));
            }
            // A large normal-axis jump with overlapping/abutting baseline
            // intervals is still valid stacked CAD, not evidence of a column.
            let stacked = rotate_glyph(glyph('B', 2.0, 100.0, 8.0), angle, scale);
            assert!(!positioned_glyphs_are_disjoint(left, stacked));
        }
    }
}

#[test]
fn positioned_gap_guard_keeps_threshold_and_unknown_geometry_conservative() {
    let mut previous = glyph('A', 0.0, 0.0, 10.0);
    previous.bounds.as_mut().unwrap().top = 10.0;
    for (gap, expected) in [(6.9, false), (7.0, false), (7.1, true)] {
        let mut next = previous;
        let bounds = next.bounds.as_mut().unwrap();
        bounds.left += 10.0 + gap;
        bounds.right += 10.0 + gap;
        assert_eq!(positioned_glyphs_are_disjoint(previous, next), expected);
        assert_eq!(positioned_glyphs_are_disjoint(next, previous), expected);
    }
    let mut next = glyph('B', 100.0, 100.0, 10.0);
    next.angle_radians = None;
    assert!(!positioned_glyphs_are_disjoint(previous, next));
    next.angle_radians = Some(f32::NAN);
    assert!(!positioned_glyphs_are_disjoint(previous, next));
    next.angle_radians = Some(std::f32::consts::FRAC_PI_2);
    assert!(!positioned_glyphs_are_disjoint(previous, next));
    next.angle_radians = Some(0.0);
    next.bounds = None;
    assert!(!positioned_glyphs_are_disjoint(previous, next));
}

#[test]
fn positioned_words_keep_stacked_cad_and_tail_profiles_intact() {
    for cross in [1.3, 1.5, 2.4] {
        let mut glyphs = Vec::new();
        for index in 0..=8 {
            if index > 0 {
                control_run(&mut glyphs);
            }
            let cross = if index >= 7 { 9.0 } else { cross };
            glyphs.push(glyph(
                char::from(b'A' + index),
                100.0 - f32::from(index) * 8.0,
                f32::from(index) * cross * 8.0,
                8.0,
            ));
        }
        assert!(control_run_profile(&glyphs).stacked_backtracking);
        let words = native_words_from_glyphs(&glyphs, false);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, "ABCDEFGHI");
        assert_eq!(resolve_pdf_control_runs(&glyphs, false), "ABCDEFGHI");
    }
}

#[test]
fn confirmed_grid_owners_split_same_column_row_jumps() {
    let mut glyphs = Vec::new();
    for index in 0..=8 {
        if index > 0 {
            control_run(&mut glyphs);
        }
        glyphs.push(glyph(
            char::from(b'A' + index),
            700.0 - f32::from(index) * 8.0,
            900.0 + f32::from(index) * 20.0,
            8.0,
        ));
    }
    glyphs.push(separator(' '));
    let first = glyphs.len();
    glyphs.push(glyph('P', 100.0, 500.0, 5.0));
    control_run(&mut glyphs);
    let second = glyphs.len();
    glyphs.push(glyph('S', 100.0, 10.0, 5.0));
    assert!(control_run_profile(&glyphs).stacked_backtracking);
    let canonical = resolve_pdf_control_runs(&glyphs, false);
    assert_eq!(canonical, "ABCDEFGHI PS");
    let legacy = native_words_from_glyphs(&glyphs, false);
    assert_eq!(legacy.last().unwrap().text, "PS");
    let mut owners = vec![GlyphGridOwner::Cell(2); glyphs.len()];
    owners[first] = GlyphGridOwner::Cell(0);
    owners[second] = GlyphGridOwner::Cell(1);
    let words = native_words_from_glyphs_with_owners(&glyphs, false, &owners);
    assert_eq!(words[words.len() - 2].text, "P");
    assert_eq!(words.last().unwrap().text, "S");
    assert_eq!(
        words.iter().map(|w| w.text.as_str()).collect::<String>(),
        "ABCDEFGHIPS"
    );
    assert_eq!(resolve_pdf_control_runs(&glyphs, false), canonical);
}

#[test]
fn confirmed_grid_owners_split_adjacent_glyphs_without_control_runs() {
    let glyphs = [glyph('A', 0.0, 0.0, 4.0), glyph('B', 4.0, 0.0, 4.0)];
    let cells = [
        BoundingBox::new(0.0, 0.0, 4.0, 10.0),
        BoundingBox::new(4.0, 0.0, 8.0, 10.0),
    ];
    let owners = glyphs
        .iter()
        .map(|g| {
            let b = g.bounds.unwrap();
            glyph_cell_owner(
                &BoundingBox::new(
                    f64::from(b.left),
                    f64::from(b.bottom),
                    f64::from(b.right),
                    f64::from(b.top),
                ),
                &cells,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(owners, [Some(0), Some(1)]);
    let owners = owners
        .into_iter()
        .map(|o| {
            o.map(GlyphGridOwner::Cell)
                .unwrap_or(GlyphGridOwner::Outside)
        })
        .collect::<Vec<_>>();
    let words = native_words_from_glyphs_with_owners(&glyphs, false, &owners);
    assert_eq!(
        words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
        ["A", "B"]
    );
    assert_eq!(resolve_pdf_control_runs(&glyphs, false), "AB");
}

#[test]
fn glyph_ownership_is_unique_and_prefers_specific_cells_over_enclosing_frames() {
    let cells = [
        BoundingBox::new(0.0, 0.0, 100.0, 100.0),
        BoundingBox::new(0.0, 0.0, 10.0, 10.0),
        BoundingBox::new(10.0, 0.0, 20.0, 10.0),
    ];
    assert_eq!(
        glyph_cell_owner(&BoundingBox::new(2.0, 2.0, 6.0, 8.0), &cells),
        Some(1)
    );
    assert_eq!(
        glyph_cell_owner(&BoundingBox::new(9.0, 2.0, 15.0, 8.0), &cells),
        Some(0)
    );
    assert_eq!(
        glyph_cell_owner(&BoundingBox::new(101.0, 2.0, 105.0, 8.0), &cells),
        None
    );
}

#[test]
fn unknown_glyph_geometry_preserves_existing_token_characters() {
    let glyphs = [
        glyph('A', 0.0, 0.0, 4.0),
        separator('?'),
        glyph('B', 4.0, 0.0, 4.0),
    ];
    let words = native_words_from_glyphs_with_owners(
        &glyphs,
        false,
        &[
            GlyphGridOwner::Cell(0),
            GlyphGridOwner::Unpositioned,
            GlyphGridOwner::Cell(0),
        ],
    );
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].text, "A?B");
    assert_eq!(resolve_pdf_control_runs(&glyphs, false), "A?B");
}

#[test]
fn unknown_prefix_inherits_first_positioned_neighbor_across_owner_changes() {
    for prefix in [
        vec![],
        vec![glyph('X', 50.0, 0.0, 4.0), separator(' ')],
        vec![glyph('X', 50.0, 0.0, 4.0), separator('\r')],
    ] {
        let mut glyphs = prefix;
        glyphs.push(separator('?'));
        glyphs.push(glyph('A', 0.0, 0.0, 4.0));
        let mut owners = vec![GlyphGridOwner::Cell(1); glyphs.len()];
        let len = owners.len();
        owners[len - 2] = GlyphGridOwner::Unpositioned;
        owners[len - 1] = GlyphGridOwner::Cell(0);
        let words = native_words_from_glyphs_with_owners(&glyphs, true, &owners);
        assert_eq!(words.last().unwrap().text, "?A");
        assert!(words.last().unwrap().bounds.is_some());
        assert_eq!(
            words.iter().map(|w| w.text.as_str()).collect::<String>(),
            glyphs
                .iter()
                .filter(|g| !g.value.is_whitespace())
                .map(|g| g.value)
                .collect::<String>()
        );
    }
}

#[test]
fn positioned_hyphens_require_pdfium_evidence_not_a_character_guess() {
    assert_eq!(positioned_character_value('\u{2}', true), '-');
    assert_eq!(positioned_character_value('\u{2}', false), '\u{2}');
    assert_eq!(positioned_character_value('\r', true), '\r');
    assert_eq!(positioned_character_value('2', true), '2');
    let canonical = [
        glyph('A', 0.0, 0.0, 4.0),
        glyph('\u{2}', 4.0, 0.0, 4.0),
        glyph('B', 8.0, 0.0, 4.0),
    ];
    assert_eq!(resolve_pdf_control_runs(&canonical, false), "AB");
    let mut positioned = canonical;
    positioned[1].value = positioned_character_value(positioned[1].value, true);
    assert_eq!(native_words_from_glyphs(&positioned, false)[0].text, "A-B");
    assert_eq!(resolve_pdf_control_runs(&canonical, false), "AB");
}

#[test]
fn subpixel_glyph_without_pixel_owner_keeps_its_neighbor_context() {
    let glyphs = [
        glyph('A', 0.0, 0.0, 4.0),
        glyph('.', 4.01, 0.0, 0.01),
        glyph('B', 4.02, 0.0, 4.0),
    ];
    let words = native_words_from_glyphs_with_owners(
        &glyphs,
        false,
        &[
            GlyphGridOwner::Cell(0),
            GlyphGridOwner::Unpositioned,
            GlyphGridOwner::Cell(0),
        ],
    );
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].text, "A.B");
    assert!(words[0].bounds.unwrap().right - words[0].bounds.unwrap().left > 8.0);
}

#[test]
fn grid_words_keep_visible_spaces_between_pdf_text_objects() {
    let spaces = vec![glyph(' ', 0.0, 0.0, 4.0); 3];
    let previous = glyph('т', 25.0, 40.0, 6.0);
    let next = glyph('-', 35.0, 40.0, 5.0);
    assert!(observed_word_space_boundary(previous, next, &spaces));
    assert!(!observed_word_space_boundary(previous, next, &spaces[..2]));
    let adjacent = glyph('-', 31.0, 40.0, 5.0);
    assert!(!observed_word_space_boundary(previous, adjacent, &spaces));
    let mut glyphs = spaces;
    glyphs.push(previous);
    control_run(&mut glyphs);
    glyphs.push(next);
    assert_eq!(resolve_pdf_control_runs(&glyphs, false).trim(), "т-");
    let words = native_words_from_glyphs_with_owners(
        &glyphs,
        false,
        &vec![GlyphGridOwner::Cell(0); glyphs.len()],
    );
    assert_eq!(
        words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
        ["т", "-"]
    );
    assert_eq!(resolve_pdf_control_runs(&glyphs, false).trim(), "т-");
}

#[test]
fn positioned_word_space_handles_reverse_font_advance_without_changing_page_text() {
    let spaces = vec![glyph(' ', 0.0, 0.0, 3.0); 3];
    let previous = glyph('A', 20.0, 0.0, 4.0);
    let next = glyph('B', 13.0, 0.0, 4.0);
    assert!(observed_word_space_boundary(previous, next, &spaces));
    let mut glyphs = spaces;
    glyphs.push(previous);
    control_run(&mut glyphs);
    glyphs.push(next);
    let canonical = resolve_pdf_control_runs(&glyphs, false);
    let words = native_words_from_glyphs_with_owners(
        &glyphs,
        false,
        &vec![GlyphGridOwner::Cell(0); glyphs.len()],
    );
    assert_eq!(
        words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );
    assert_eq!(resolve_pdf_control_runs(&glyphs, false), canonical);
}

#[test]
fn positioned_word_space_uses_regular_neighbor_after_a_superscript() {
    let mut previous = glyph('2', 20.0, 0.0, 3.0);
    previous.bounds.as_mut().unwrap().top = 5.0;
    let next = glyph('/', 26.0, 0.0, 3.0);
    // Repeated spaces belong to the regular text size, not the superscript.
    let spaces = vec![glyph(' ', 0.0, 0.0, 3.0); 3];
    assert!(observed_word_space_boundary(previous, next, &spaces));
    let adjacent = glyph('/', 23.01, 0.0, 3.0);
    assert!(!observed_word_space_boundary(previous, adjacent, &spaces));
    assert!(!observed_word_space_boundary(previous, next, &spaces[..2]));
}

#[test]
fn positioned_words_use_ink_geometry_but_preserve_font_metrics_and_canonical_text() {
    let mut a = glyph('A', 0.0, 0.0, 5.0);
    a.ink_bounds = Some(PdfWordBounds {
        left: 1.0,
        bottom: 2.0,
        right: 4.0,
        top: 7.0,
    });
    let mut b = glyph('B', 5.0, 0.0, 5.0);
    b.ink_bounds = Some(PdfWordBounds {
        left: 5.5,
        bottom: 2.0,
        right: 9.0,
        top: 7.0,
    });
    for degrees in [0.0_f32, 90.0, 180.0, 270.0] {
        let glyphs = [a, b].map(|glyph| rotate_glyph(glyph, degrees.to_radians(), 1.0));
        let canonical = resolve_pdf_control_runs(&glyphs, false);
        let words =
            native_words_from_glyphs_with_owners(&glyphs, false, &[GlyphGridOwner::Cell(0); 2]);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, "AB");
        let expected = glyphs[0]
            .ink_bounds
            .unwrap()
            .union(glyphs[1].ink_bounds.unwrap());
        let actual = words[0].bounds.unwrap();
        assert!((actual.left - expected.left).abs() < 0.001);
        assert!((actual.top - expected.top).abs() < 0.001);
        assert!((actual.right - expected.right).abs() < 0.001);
        assert!((actual.bottom - expected.bottom).abs() < 0.001);
        assert_eq!(resolve_pdf_control_runs(&glyphs, false), canonical);
    }
    // Missing tight geometry falls back to valid font geometry, not loss.
    b.ink_bounds = None;
    let words = native_words_from_glyphs_with_owners(&[a, b], false, &[GlyphGridOwner::Cell(0); 2]);
    assert_eq!(words[0].bounds.unwrap().right, 10.0);
    assert_eq!(words[0].text, "AB");
}

#[test]
fn isolated_subpixel_ink_falls_back_to_font_box_instead_of_losing_the_character() {
    let mut source = glyph('|', 0.0, 0.0, 3.0);
    source.ink_bounds = Some(PdfWordBounds {
        left: 1.01,
        bottom: 2.0,
        right: 1.05,
        top: 7.0,
    });
    let words = native_words_from_glyphs_with_owners(&[source], false, &[GlyphGridOwner::Cell(0)]);
    assert_eq!(words[0].text, "|");
    let projected = project_positioned_word_bounds(&words[0], |bounds| {
        let x0 = bounds.left.round();
        let x1 = bounds.right.round();
        Ok::<_, ()>((x1 > x0).then(|| BoundingBox::new(f64::from(x0), 0.0, f64::from(x1), 10.0)))
    })
    .unwrap()
    .expect("font bounds remain renderable");
    assert_eq!(projected.x0, 0.0);
    assert_eq!(projected.x1, 3.0);
    assert_eq!(resolve_pdf_control_runs(&[source], false), "|");
    let next = glyph('A', 3.0, 0.0, 4.0);
    let adjacent =
        native_words_from_glyphs_with_owners(&[source, next], false, &[GlyphGridOwner::Cell(0); 2]);
    assert_eq!(
        adjacent
            .iter()
            .map(|word| word.text.as_str())
            .collect::<String>(),
        "|A"
    );
}

#[test]
fn subpixel_prefix_is_not_flushed_as_an_unrenderable_singleton() {
    for prefix in [
        vec![],
        vec![glyph('X', 50.0, 0.0, 4.0), separator(' ')],
        vec![glyph('X', 50.0, 0.0, 4.0), separator('\r')],
    ] {
        let mut glyphs = prefix;
        glyphs.push(glyph('.', 4.01, 0.0, 0.01));
        glyphs.push(glyph('A', 4.02, 0.0, 4.0));
        let mut owners = vec![GlyphGridOwner::Cell(1); glyphs.len()];
        let n = owners.len();
        owners[n - 2] = GlyphGridOwner::Unpositioned;
        owners[n - 1] = GlyphGridOwner::Cell(0);
        let words = native_words_from_glyphs_with_owners(&glyphs, true, &owners);
        assert_eq!(words.last().unwrap().text, ".A");
        assert!(
            words.last().unwrap().bounds.unwrap().right
                - words.last().unwrap().bounds.unwrap().left
                > 4.0
        );
        assert_eq!(
            words.iter().map(|w| w.text.as_str()).collect::<String>(),
            glyphs
                .iter()
                .filter(|g| !g.value.is_whitespace())
                .map(|g| g.value)
                .collect::<String>()
        );
    }
}

#[test]
fn grid_physical_lines_keep_bilingual_units_separate_at_all_quarter_turns() {
    let mut glyphs = Vec::new();
    for index in 0..=8 {
        if index > 0 {
            control_run(&mut glyphs);
        }
        glyphs.push(glyph(
            char::from(b'A' + index),
            700.0 - f32::from(index) * 8.0,
            900.0 + f32::from(index) * 20.0,
            8.0,
        ));
    }
    glyphs.push(separator(' '));
    glyphs.push(glyph('м', 100.0, 100.0, 5.0));
    control_run(&mut glyphs);
    for (index, value) in "Meter".chars().enumerate() {
        glyphs.push(glyph(value, 100.0 + index as f32 * 4.0, 85.0, 4.0));
    }
    for degrees in [0.0_f32, 90.0, 180.0, 270.0] {
        let rotated = glyphs
            .iter()
            .map(|g| rotate_glyph(*g, degrees.to_radians(), 1.0))
            .collect::<Vec<_>>();
        assert!(control_run_profile(&rotated).stacked_backtracking);
        let canonical = resolve_pdf_control_runs(&rotated, false);
        let words = native_words_from_glyphs_with_owners(
            &rotated,
            false,
            &vec![GlyphGridOwner::Cell(0); rotated.len()],
        );
        assert_eq!(words[words.len() - 2].text, "м");
        assert_eq!(words.last().unwrap().text, "Meter");
        assert_eq!(canonical, "ABCDEFGHI мMeter");
        assert_eq!(resolve_pdf_control_runs(&rotated, false), canonical);
    }
}

#[test]
fn unowned_grid_material_cannot_make_a_union_through_distant_cells() {
    let mut glyphs = Vec::new();
    for index in 0..=8 {
        if index > 0 {
            control_run(&mut glyphs);
        }
        glyphs.push(glyph(
            char::from(b'A' + index),
            700.0 - f32::from(index) * 8.0,
            900.0 + f32::from(index) * 20.0,
            8.0,
        ));
    }
    glyphs.push(separator(' '));
    glyphs.push(glyph('.', 100.0, 500.0, 5.0));
    control_run(&mut glyphs);
    glyphs.push(glyph('H', 100.0, 10.0, 5.0));
    assert_eq!(
        native_words_from_glyphs(&glyphs, false)
            .last()
            .unwrap()
            .text,
        ".H"
    );
    let words = native_words_from_glyphs_with_owners(
        &glyphs,
        false,
        &vec![GlyphGridOwner::Outside; glyphs.len()],
    );
    assert_eq!(words[words.len() - 2].text, ".");
    assert_eq!(words.last().unwrap().text, "H");
    assert!(words.iter().all(|w| w.bounds.unwrap().height() < 10.0));
    assert_eq!(
        words.iter().map(|w| w.text.as_str()).collect::<String>(),
        "ABCDEFGHI.H"
    );
}
