//! Golden-frame tests — render widgets into a headless [`TestBackend`] and
//! assert the resulting cells / lines with structured accessors, not
//! `.contains()` on a formatted blob (★★ TOKEN-STABILITY).

use egaku::{
    Column, IDENTITY_FIELD, ListView, Rect, SortKey, SortOrder, TabBar, TableRow, TableView,
};
use egaku_term::crossterm::style::Color;
use egaku_term::theme::Palette;
use egaku_term::{Draw, Modifiers, TestBackend, draw};

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

// ---------------------------------------------------------------------------
// TableView — through the public API only, exactly as a consumer sees it.
// ---------------------------------------------------------------------------

/// The whole consumer-side obligation for a table: an identity and a cell
/// lookup. Everything else (widths, order, cursor, viewport) is the library's.
struct Pod {
    name: String,
    phase: String,
}

impl TableRow for Pod {
    fn identity(&self) -> &str {
        &self.name
    }

    fn cell(&self, field: &str) -> Option<&str> {
        match field {
            "phase" => Some(&self.phase),
            _ => None,
        }
    }
}

fn pod(name: &str, phase: &str) -> Pod {
    Pod {
        name: name.to_string(),
        phase: phase.to_string(),
    }
}

fn pod_table(rows: Vec<Pod>) -> TableView<Pod> {
    TableView::new(
        vec![
            Column::new("NAME", IDENTITY_FIELD),
            Column::new("STATUS", "phase"),
        ],
        rows,
        SortKey::new("NAME", SortOrder::Asc),
    )
    .expect("NAME is a declared column")
}

#[test]
fn golden_table_lays_out_header_rule_and_aligned_rows() {
    let table = pod_table(vec![pod("catch-0", "Running"), pod("gateway-7", "Pending")]);
    let mut backend = TestBackend::new(24, 4);
    backend.draw(|buf| {
        draw::table(buf, Rect::new(0.0, 0.0, 24.0, 4.0), &table, true);
    });

    assert_eq!(
        backend.to_lines(),
        vec![
            "NAME       STATUS".to_string(),
            "────────────────────────".to_string(),
            "catch-0    Running".to_string(),
            "gateway-7  Pending".to_string(),
        ]
    );

    let palette = Palette::default();
    // Header: accent + bold.
    let header = backend.cell(0, 0).expect("header cell");
    assert_eq!(header.fg, palette.accent);
    assert!(header.modifiers.contains(Modifiers::BOLD));
    // Selected (first) data row carries the selection bar; the next does not.
    assert_eq!(backend.cell(0, 2).unwrap().bg, palette.selection);
    assert_eq!(backend.cell(0, 3).unwrap().bg, Color::Reset);
}

#[test]
fn golden_table_scrolls_the_selection_into_view() {
    // Four rows, two rows of screen. The model holds no offset — the drawer
    // derives the window, bottom-anchored on the cursor.
    let mut table = pod_table(vec![
        pod("r0", "Running"),
        pod("r1", "Running"),
        pod("r2", "Running"),
        pod("r3", "Running"),
    ]);
    table.select_next();
    table.select_next();
    table.select_next();

    let mut backend = TestBackend::new(20, 4);
    backend.draw(|buf| {
        draw::table(buf, Rect::new(0.0, 0.0, 20.0, 4.0), &table, true);
    });

    let lines = backend.to_lines();
    assert!(lines[2].starts_with("r2"), "got {:?}", lines[2]);
    assert!(
        lines[3].starts_with("r3"),
        "the selected row must be visible, got {:?}",
        lines[3]
    );
}

#[test]
fn golden_draw_trait_renders_a_table_with_widget_owned_focus() {
    // Same frame, reached through `Draw` with no focus argument at the call
    // site — the focus flag rides on the widget.
    let mut table = pod_table(vec![pod("catch-0", "Running")]);
    table.set_focused(true);

    let palette = Palette::default();
    let mut backend = TestBackend::new(24, 3);
    backend.draw(|buf| {
        table.draw(buf, Rect::new(0.0, 0.0, 24.0, 3.0), &palette);
    });

    let selected = backend.cell(0, 2).expect("selected data cell");
    assert_eq!(selected.bg, palette.selection);
    assert!(
        selected.modifiers.contains(Modifiers::BOLD),
        "a focused table bolds its selected row"
    );
}
