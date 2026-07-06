//! Golden-frame tests — render widgets into a headless [`TestBackend`] and
//! assert the resulting cells / lines with structured accessors, not
//! `.contains()` on a formatted blob (★★ TOKEN-STABILITY).

use egaku::{ListView, Rect, TabBar};
use egaku_term::crossterm::style::Color;
use egaku_term::theme::Palette;
use egaku_term::{Modifiers, TestBackend, draw};

#[test]
fn golden_list_marks_selected_row() {
    let list = ListView::new(
        vec!["one".to_string(), "two".to_string(), "three".to_string()],
        3,
    );
    let mut backend = TestBackend::new(20, 3);
    backend.draw(|buf| {
        draw::list(buf, Rect::new(0.0, 0.0, 20.0, 3.0), &list, true);
    });

    assert_eq!(
        backend.to_lines(),
        vec![
            "▶ one".to_string(),
            "  two".to_string(),
            "  three".to_string(),
        ]
    );

    let palette = Palette::default();
    let selected = backend.cell(0, 0).expect("selected gutter cell");
    assert_eq!(selected.symbol(), "▶");
    assert_eq!(selected.bg, palette.selection);
    assert!(selected.modifiers.contains(Modifiers::BOLD));

    // Unselected rows keep the terminal-default background.
    assert_eq!(backend.cell(0, 1).unwrap().bg, Color::Reset);
}

#[test]
fn golden_header_is_bold_accent() {
    let mut backend = TestBackend::new(10, 1);
    backend.draw(|buf| {
        draw::header(buf, Rect::new(0.0, 0.0, 10.0, 1.0), "Title");
    });

    assert_eq!(backend.to_lines(), vec!["Title".to_string()]);

    let palette = Palette::default();
    let first = backend.cell(0, 0).expect("header first cell");
    assert_eq!(first.symbol(), "T");
    assert_eq!(first.fg, palette.accent);
    assert!(first.modifiers.contains(Modifiers::BOLD));
}

#[test]
fn golden_tabs_render_active_label() {
    let bar = TabBar::new(vec!["alpha".to_string(), "beta".to_string()]);
    let mut backend = TestBackend::new(24, 1);
    backend.draw(|buf| {
        draw::tabs(buf, Rect::new(0.0, 0.0, 24.0, 1.0), &bar, true);
    });

    let line = &backend.to_lines()[0];
    assert!(line.contains("alpha"), "tabs line was {line:?}");
    assert!(line.contains("beta"), "tabs line was {line:?}");

    // The active tab (index 0) carries the accent background — cell(1,0) is
    // the 'a' of " alpha " (cell 0 is the leading pad space).
    let palette = Palette::default();
    let active = backend.cell(1, 0).expect("active tab glyph");
    assert_eq!(active.symbol(), "a");
    assert_eq!(active.bg, palette.accent);
}

#[test]
fn golden_bordered_block_draws_corners() {
    let mut backend = TestBackend::new(6, 3);
    backend.draw(|buf| {
        draw::bordered_block(buf, Rect::new(0.0, 0.0, 6.0, 3.0), "", true);
    });

    assert_eq!(backend.cell(0, 0).unwrap().symbol(), "┌");
    assert_eq!(backend.cell(5, 0).unwrap().symbol(), "┐");
    assert_eq!(backend.cell(0, 2).unwrap().symbol(), "└");
    assert_eq!(backend.cell(5, 2).unwrap().symbol(), "┘");
    // Top edge between corners is a horizontal rule.
    assert_eq!(backend.cell(2, 0).unwrap().symbol(), "─");
    // Left edge middle row is a vertical rule.
    assert_eq!(backend.cell(0, 1).unwrap().symbol(), "│");

    // Focused block uses the accent border color.
    let palette = Palette::default();
    assert_eq!(backend.cell(0, 0).unwrap().fg, palette.accent);
}
