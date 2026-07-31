//! Drawers for [`egaku`] widgets onto a [`Buffer`].
//!
//! Every drawer takes the widget by reference, a [`Rect`](egaku::Rect)
//! describing where to render (in cell coordinates — `1.0 == one terminal
//! cell`), a [`Palette`](crate::theme::Palette) for colors, and a `focused`
//! flag so widgets can dim when inactive. Drawers write typed
//! [`Cell`](crate::cell::Cell)s into the back [`Buffer`] via the buffer's typed
//! ops ([`Buffer::set_stringn`], [`Buffer::set_char`], [`Buffer::blank`],
//! [`Buffer::hline`]) — no drawer builds a styled string by hand or spells an
//! escape sequence (★★ TYPED EMISSION / Quadro P5). The runtime diffs the
//! buffer against the previous frame and flushes only the changed cells.

// If this line fails with `E0432: no TableView in the root`, you are compiling
// against the egaku the lockfile pins, which predates `Selectable`/`TableView`.
// That is a KNOWN, RECORDED state, not a new breakage: read
// `pending-egaku-bump:` at the top of CLAUDE.md for the clearing chain, and see
// `tests/pending_egaku_bump.rs` for why the compiler — not a test — is the gate
// in this direction. (A signpost, not a check; the hard gate is this import.)
use egaku::{ListView, Modal, Rect, ScrollView, SplitPane, TabBar, TableRow, TableView, TextInput};
use unicode_width::UnicodeWidthStr;

use crate::buffer::Buffer;
use crate::cell::Style;
use crate::theme::Palette;

// ---------- The Draw trait ---------------------------------------------------

/// One widget, one way to render it: `widget.draw(buf, rect, palette)`.
///
/// # Why the trait lives HERE and not in egaku
///
/// A drawer needs a [`Buffer`] and a [`Palette`], and both are terminal
/// concepts. egaku depends on serde / tracing / thiserror / unicode-* and
/// **nothing else** — no crossterm, no backend — which is exactly what lets one
/// `ListView` value drive a GPU pane (via `garasu`) and a TTY pane (via this
/// crate) without either renderer knowing about the other. The dependency
/// arrow `egaku-term → egaku` is one-way and load-bearing; a `Draw` trait in
/// egaku would reverse it. So egaku owns the *state* vocabulary
/// ([`egaku::Selectable`]) and each renderer owns its own *rendering*
/// vocabulary. This is that vocabulary for terminals.
///
/// # Relationship to the free `draw::*` functions
///
/// Nothing is replaced. [`list`], [`tabs`], [`text_input`] and friends remain
/// exactly as they were, including their explicit `focused: bool` parameter —
/// callers that track focus out-of-band (from an [`egaku::FocusManager`], which
/// keys focus by widget *name*) keep passing it. `Draw` is the uniform surface
/// on top: it sources `focused` from the widget's own `is_focused()`, which is
/// why those widgets grew a self-owned focus flag. Pick per call site:
///
/// - a screen that renders a fixed set of named panes → the free functions
/// - a heterogeneous list of widgets rendered in a loop → `&dyn Draw`
///
/// `Draw` is object-safe, so `Vec<Box<dyn Draw>>` and `&[&dyn Draw]` work:
///
/// ```
/// use egaku::{ListView, Rect, TabBar};
/// use egaku_term::{Draw, TestBackend, theme::Palette};
///
/// let mut list = ListView::new(vec!["one".into()], 3);
/// list.set_focused(true);
/// let bar = TabBar::new(vec!["alpha".into()]);
/// let panes: Vec<&dyn Draw> = vec![&list, &bar];
///
/// let palette = Palette::default();
/// let mut backend = TestBackend::new(20, 2);
/// backend.draw(|buf| {
///     for (row, pane) in panes.iter().enumerate() {
///         pane.draw(buf, Rect::new(0.0, row as f32, 20.0, 1.0), &palette);
///     }
/// });
/// assert!(backend.to_lines()[0].contains("one"));
/// ```
///
/// # Implementors
///
/// [`ListView`], [`TextInput`], [`TabBar`], [`TableView`] (focus read from the
/// widget), [`ScrollView`] (as a scrollbar in the rect's right column), and
/// [`SplitPane`] (as its divider line).
///
/// # [`Modal`] is deliberately NOT an implementor
///
/// This is a documented carve-out, not an oversight. [`modal`]'s signature is
/// `(buf, bounds, modal, body: &[&str])`: the body is **content the `Modal`
/// value does not own**, and `Draw::draw` has nowhere to put it. Two ways to
/// force the fit were considered and both rejected:
///
/// - *Give `Modal` a body field.* `Modal` is by design a visibility state
///   machine — `show` / `hide` / `is_visible` and a title, nothing else.
///   Making it own its body turns it from an FSM into a content container, and
///   a modal body is usually derived per-frame from application state that the
///   widget has no business caching. Focus is genuinely widget state; a body is
///   not.
/// - *Implement `Draw` for `(&Modal, &[&str])`.* That satisfies the compiler
///   and teaches nobody anything: the tuple is not a widget, and it would make
///   the trait's meaning ("a thing that knows how to draw itself") false for
///   one implementor.
///
/// So: call [`modal`] / [`modal_with`] directly. If a future `Modal` really
/// does come to own its content, it can join the trait then — the honest gap
/// is cheaper than a bad fit.
pub trait Draw {
    /// Render this widget into `buf` within `rect`, using `palette` for color.
    fn draw(&self, buf: &mut Buffer, rect: Rect, palette: &Palette);
}

impl Draw for ListView {
    fn draw(&self, buf: &mut Buffer, rect: Rect, palette: &Palette) {
        list_with(buf, rect, self, self.is_focused(), palette);
    }
}

impl Draw for TextInput {
    fn draw(&self, buf: &mut Buffer, rect: Rect, palette: &Palette) {
        text_input_with(buf, rect, self, self.is_focused(), palette);
    }
}

impl Draw for TabBar {
    fn draw(&self, buf: &mut Buffer, rect: Rect, palette: &Palette) {
        tabs_with(buf, rect, self, self.is_focused(), palette);
    }
}

impl<R: TableRow> Draw for TableView<R> {
    fn draw(&self, buf: &mut Buffer, rect: Rect, palette: &Palette) {
        table_with(buf, rect, self, self.is_focused(), palette);
    }
}

/// A `ScrollView` draws as its scroll indicator — the one-column gutter on the
/// right edge of `rect`. The scrolled *content* is the caller's, so this is the
/// only part of a scroll view the widget itself can render.
impl Draw for ScrollView {
    fn draw(&self, buf: &mut Buffer, rect: Rect, palette: &Palette) {
        scrollbar_with(buf, rect, self, palette);
    }
}

/// A `SplitPane` draws as its divider. The children are the caller's; it hands
/// out their geometry via [`SplitPane::first_rect`] / [`SplitPane::second_rect`].
impl Draw for SplitPane {
    fn draw(&self, buf: &mut Buffer, rect: Rect, palette: &Palette) {
        split_with(buf, rect, self, palette);
    }
}

/// Convert egaku's `f32` rect into integer terminal coordinates.
/// Negative or wildly-out-of-range values clamp to zero.
fn to_cell_rect(rect: Rect) -> (u16, u16, u16, u16) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let to_u16 = |f: f32| f.max(0.0).round().min(f32::from(u16::MAX)) as u16;
    (
        to_u16(rect.x),
        to_u16(rect.y),
        to_u16(rect.width),
        to_u16(rect.height),
    )
}

/// Truncate a string so its display width fits in `max_cols`.
/// Uses [`unicode_width`] so CJK / emoji measure correctly.
fn truncate_to_width(s: &str, max_cols: u16) -> String {
    if max_cols == 0 {
        return String::new();
    }
    let max = usize::from(max_cols);
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > max {
            break;
        }
        out.push(ch);
        used += w;
    }
    out
}

// ---------- ListView ---------------------------------------------------------

/// Render a [`ListView`] inside `rect`. The selected row is rendered with the
/// palette's selection background and a `▶ ` gutter glyph; every other row
/// reserves a 2-space gutter so the text columns stay aligned.
///
/// **`focused` adds BOLD to the selected row, and nothing else.** (Corrected
/// 2026-07-31: this doc previously claimed the `▶ ` marker was the focus
/// signal and that an unfocused list drew a blank gutter. It never did — the
/// marker has always tracked *selection*, and `golden_list_marks_selected_row`
/// has always asserted BOLD as the focus signal. The prose was the thing that
/// was wrong; the behaviour is unchanged, since consumers render against it.)
pub fn list(buf: &mut Buffer, rect: Rect, list: &ListView, focused: bool) {
    list_with(buf, rect, list, focused, &Palette::default());
}

/// Like [`list`] but with an explicit palette.
pub fn list_with(buf: &mut Buffer, rect: Rect, list: &ListView, focused: bool, palette: &Palette) {
    let (x, y, w, h) = to_cell_rect(rect);
    if w == 0 || h == 0 {
        return;
    }

    let visible = list.visible_items();
    let offset = list.offset();
    for (idx, item) in visible.iter().enumerate() {
        let row_idx = u16::try_from(idx).unwrap_or(u16::MAX);
        if row_idx >= h {
            break;
        }
        let row = y + row_idx;
        let is_selected = offset + idx == list.selected_index();

        let (style, prefix) = if is_selected {
            let mut sel = Style::default()
                .fg(palette.foreground)
                .bg(palette.selection);
            if focused {
                sel = sel.bold();
            }
            (sel, "▶ ")
        } else {
            (Style::default(), "  ")
        };

        if is_selected {
            // Paint the full-width highlight bar first so the padding to the
            // right of the text carries the selection background.
            buf.blank(x, row, w, style);
        }
        let end = buf.set_stringn(x, row, prefix, w, style);
        let remaining = x.saturating_add(w).saturating_sub(end);
        buf.set_stringn(end, row, item, remaining, style);
    }
}

// ---------- TableView --------------------------------------------------------

/// Cells of gutter between two table columns.
const TABLE_COL_GAP: u16 = 2;

/// Render a [`TableView`] inside `rect`: a header row, a rule, then the data
/// rows. See [`table_with`] for the layout rules.
pub fn table<R: TableRow>(buf: &mut Buffer, rect: Rect, table: &TableView<R>, focused: bool) {
    table_with(buf, rect, table, focused, &Palette::default());
}

/// Like [`table`] but with an explicit palette.
///
/// Layout: row `y` is the header (accent, bold), `y + 1` is a rule, data rows
/// start at `y + 2`. Column widths are the max display width of the header and
/// every projected cell, measured with [`unicode_width`] so CJK and emoji
/// columns line up. The selected row is painted full-width with the palette's
/// selection background; on a focused table it is also bold.
///
/// # The viewport is derived here, not stored in the model
///
/// [`TableView`] carries no scroll offset — it is the ordered row set plus a
/// cursor. This drawer derives the visible window from the cursor and the rect
/// height, bottom-anchored, so **the selected row is always on screen** no
/// matter how far down a table taller than the terminal it sits. The source
/// model this was lifted from had no windowing at all: a table taller than the
/// terminal simply clipped, and moving the cursor past the last visible row
/// moved it somewhere the operator could not see. That class is gone, and it
/// cost no new state — the offset is a pure function of `(selected, height)`,
/// so there is no second place for it to be wrong.
pub fn table_with<R: TableRow>(
    buf: &mut Buffer,
    rect: Rect,
    table: &TableView<R>,
    focused: bool,
    palette: &Palette,
) {
    let (x, y, w, h) = to_cell_rect(rect);
    if w == 0 || h == 0 {
        return;
    }

    let widths = table_column_widths(table);

    // ── Header row ──
    let headers: Vec<&str> = table.columns().iter().map(|c| c.header.as_str()).collect();
    let header_style = Style::default().fg(palette.accent).bold();
    draw_table_cells(buf, x, y, w, &headers, &widths, header_style);

    // ── Rule under the header ──
    if h >= 2 {
        buf.hline(x, y + 1, w, '─', Style::default().fg(palette.border));
    }

    // ── Data rows ──
    let visible_rows = usize::from(h.saturating_sub(2));
    if visible_rows == 0 {
        return;
    }
    let first_visible = table
        .selected_index()
        .saturating_sub(visible_rows.saturating_sub(1));
    let first_data_row = y + 2;

    for (offset, row) in table
        .rows()
        .iter()
        .skip(first_visible)
        .take(visible_rows)
        .enumerate()
    {
        let Ok(row_i) = u16::try_from(offset) else {
            break;
        };
        let ry = first_data_row + row_i;
        let is_selected = first_visible + offset == table.selected_index();

        let style = if is_selected {
            let mut sel = Style::default()
                .fg(palette.foreground)
                .bg(palette.selection);
            if focused {
                sel = sel.bold();
            }
            // Paint the full-width bar first so the padding to the right of
            // the last column carries the selection background.
            buf.blank(x, ry, w, sel);
            sel
        } else {
            Style::default()
        };

        // Project the row through the columns FIRST, then draw. Header and
        // data reach `draw_table_cells` as the same `&[&str]`, so there is no
        // variant to branch on and no way for a row to accidentally render the
        // header text — the bug the source model hit with an `Option<&Row>`
        // that overloaded `None` to mean both "header" and "selected row".
        let values: Vec<&str> = table
            .columns()
            .iter()
            .map(|c| table.cell_value(row, c))
            .collect();
        draw_table_cells(buf, x, ry, w, &values, &widths, style);
    }
}

/// Column render widths: the max display width of the header and every
/// projected cell value, so the columns line up.
fn table_column_widths<R: TableRow>(table: &TableView<R>) -> Vec<u16> {
    table
        .columns()
        .iter()
        .map(|col| {
            let w = table
                .rows()
                .iter()
                .map(|r| table.cell_value(r, col).width())
                .fold(col.header.width(), usize::max);
            u16::try_from(w).unwrap_or(u16::MAX)
        })
        .collect()
}

/// Draw one row of already-projected values left-to-right, each padded to its
/// column width with [`TABLE_COL_GAP`] between. Every write is a typed
/// [`Buffer`] op — no `format!()` of VT (★★ TYPED EMISSION).
fn draw_table_cells(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    values: &[&str],
    widths: &[u16],
    style: Style,
) {
    let right_edge = x.saturating_add(width);
    let mut cx = x;
    for (value, &cw) in values.iter().zip(widths.iter()) {
        if cx >= right_edge {
            break;
        }
        let remaining = right_edge.saturating_sub(cx);
        buf.set_stringn(cx, y, value, cw.min(remaining), style);
        cx = cx.saturating_add(cw).saturating_add(TABLE_COL_GAP);
    }
}

// ---------- TextInput --------------------------------------------------------

/// Render a [`TextInput`] on a single row of `rect`. When `focused`, a
/// reverse-video block cursor is drawn at the input's cursor position;
/// otherwise the text is drawn dim.
pub fn text_input(buf: &mut Buffer, rect: Rect, input: &TextInput, focused: bool) {
    text_input_with(buf, rect, input, focused, &Palette::default());
}

/// Like [`text_input`] but with an explicit palette.
pub fn text_input_with(
    buf: &mut Buffer,
    rect: Rect,
    input: &TextInput,
    focused: bool,
    palette: &Palette,
) {
    let (x, y, w, _h) = to_cell_rect(rect);
    if w == 0 {
        return;
    }

    let text = input.text();
    let fg = if focused {
        palette.foreground
    } else {
        palette.muted
    };
    let style = Style::default().fg(fg);
    buf.set_stringn(x, y, text, w, style);

    if focused {
        // Block-style cursor: redraw the glyph at the cursor with reverse attr.
        let cursor_byte = input.cursor();
        let prefix_width = u16::try_from(text[..cursor_byte.min(text.len())].width()).unwrap_or(0);
        let cursor_col = x + prefix_width.min(w.saturating_sub(1));
        let cursor_glyph = text[cursor_byte..].chars().next().unwrap_or(' ');
        let cursor_style = Style::default().fg(fg).reversed();
        buf.set_char(cursor_col, y, cursor_glyph, cursor_style);
    }
}

// ---------- TabBar -----------------------------------------------------------

/// Render a [`TabBar`] as a single row of `[ tab ]  [ tab ]  ...`. The
/// active tab is reverse-video; a focused bar bolds it.
pub fn tabs(buf: &mut Buffer, rect: Rect, bar: &TabBar, focused: bool) {
    tabs_with(buf, rect, bar, focused, &Palette::default());
}

/// Like [`tabs`] but with an explicit palette.
pub fn tabs_with(buf: &mut Buffer, rect: Rect, bar: &TabBar, focused: bool, palette: &Palette) {
    let (x, y, w, _h) = to_cell_rect(rect);
    if w == 0 {
        return;
    }

    let mut col: u16 = 0;
    for (i, name) in bar.tabs().iter().enumerate() {
        // Label is " {name} " — one pad cell on each side.
        let name_w = u16::try_from(name.width()).unwrap_or(w);
        let label_w = name_w.saturating_add(2);
        if col + label_w + 1 > w {
            break;
        }

        let is_active = i == bar.active_index();
        let style = if is_active {
            let mut s = Style::default().bg(palette.accent).fg(palette.background);
            if focused {
                s = s.bold();
            }
            s
        } else {
            Style::default().fg(palette.muted)
        };

        let start = x + col;
        // Paint the pad cells + background, then the label glyphs over them.
        buf.blank(start, y, label_w, style);
        buf.set_stringn(start + 1, y, name, name_w, style);
        col += label_w + 1; // +1 spacer between tabs
    }
}

// ---------- Modal ------------------------------------------------------------

/// Render a [`Modal`] centered inside `bounds`. Skips entirely when the
/// modal is not visible, so callers can call this unconditionally each
/// frame. The body is supplied as a slice of pre-wrapped lines.
pub fn modal(buf: &mut Buffer, bounds: Rect, modal: &Modal, body: &[&str]) {
    modal_with(buf, bounds, modal, body, &Palette::default());
}

/// Like [`modal`] but with an explicit palette.
pub fn modal_with(buf: &mut Buffer, bounds: Rect, modal: &Modal, body: &[&str], palette: &Palette) {
    if !modal.is_visible() {
        return;
    }
    let (bx, by, bw, bh) = to_cell_rect(bounds);
    if bw < 6 || bh < 4 {
        return;
    }

    // Compute box size: at most 80% of bounds, at least enough for the
    // longest line + 4 cells of padding.
    let max_content_w = body.iter().map(|s| s.width()).max().unwrap_or(0);
    let title_w = modal.title().width();
    let want_w = max_content_w.max(title_w) + 4;
    let want_h = body.len() + 4;

    let box_w = u16::try_from(want_w).unwrap_or(bw).min(bw * 4 / 5);
    let box_h = u16::try_from(want_h).unwrap_or(bh).min(bh * 4 / 5);
    if box_w < 2 || box_h < 2 {
        return;
    }
    let box_x = bx + (bw.saturating_sub(box_w)) / 2;
    let box_y = by + (bh.saturating_sub(box_h)) / 2;
    let right = box_x + box_w - 1;
    let inner = box_w - 2; // cells between the two corner columns

    let style = Style::default().fg(palette.border).bg(palette.background);

    // Fill the whole box with the modal background first.
    for r in 0..box_h {
        buf.blank(box_x, box_y + r, box_w, style);
    }

    // Top border: ┌─ title ─...─┐
    buf.hline(box_x + 1, box_y, inner, '─', style);
    buf.set_char(box_x, box_y, '┌', style);
    buf.set_char(right, box_y, '┐', style);
    if inner >= 3 {
        let mut cx = box_x + 1;
        cx = buf.set_stringn(cx, box_y, "─ ", inner, style);
        let title_budget = right.saturating_sub(cx).saturating_sub(1);
        cx = buf.set_stringn(cx, box_y, modal.title(), title_budget, style);
        if cx < right {
            buf.set_char(cx, box_y, ' ', style);
        }
    }

    // Side borders on every interior row.
    for r in 1..(box_h - 1) {
        buf.set_char(box_x, box_y + r, '│', style);
        buf.set_char(right, box_y + r, '│', style);
    }

    // Body rows: 1-cell left pad, clipped to the interior width.
    let content_budget = inner.saturating_sub(2);
    for (i, line) in body.iter().enumerate() {
        let row_idx = u16::try_from(i + 1).unwrap_or(u16::MAX);
        if row_idx >= box_h - 1 {
            break;
        }
        buf.set_stringn(box_x + 2, box_y + row_idx, line, content_budget, style);
    }

    // Bottom border.
    let bottom = box_y + box_h - 1;
    buf.hline(box_x + 1, bottom, inner, '─', style);
    buf.set_char(box_x, bottom, '└', style);
    buf.set_char(right, bottom, '┘', style);
}

// ---------- ScrollView indicator --------------------------------------------

/// Render a one-column scroll indicator on the right edge of `rect`. The
/// thumb's relative position reflects [`ScrollView::scroll_fraction`]; the
/// thumb size reflects the viewport-to-content ratio.
pub fn scrollbar(buf: &mut Buffer, rect: Rect, scroll: &ScrollView) {
    scrollbar_with(buf, rect, scroll, &Palette::default());
}

/// Like [`scrollbar`] but with an explicit palette.
pub fn scrollbar_with(buf: &mut Buffer, rect: Rect, scroll: &ScrollView, palette: &Palette) {
    let (x, y, w, h) = to_cell_rect(rect);
    if w == 0 || h == 0 {
        return;
    }
    let col = x + w - 1;
    let style = Style::default().fg(palette.muted);

    if scroll.max_scroll() <= 0.0 {
        // No scrolling needed — draw the gutter dim for visual consistency.
        for r in 0..h {
            buf.set_char(col, y + r, '│', style);
        }
        return;
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let thumb_size = ((scroll.viewport_height / scroll.content_height) * f32::from(h))
        .max(1.0)
        .min(f32::from(h)) as u16;

    let scrollable_h = h - thumb_size;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let thumb_top = (scroll.scroll_fraction() * f32::from(scrollable_h)).round() as u16;

    for r in 0..h {
        let glyph = if r >= thumb_top && r < thumb_top + thumb_size {
            '█'
        } else {
            '│'
        };
        buf.set_char(col, y + r, glyph, style);
    }
}

// ---------- SplitPane border -------------------------------------------------

/// Render the divider line between a [`SplitPane`]'s two children inside
/// `bounds`. Children themselves render via the panes' [`SplitPane::first_rect`]
/// / [`SplitPane::second_rect`] coordinates.
pub fn split(buf: &mut Buffer, bounds: Rect, split: &SplitPane) {
    split_with(buf, bounds, split, &Palette::default());
}

/// Like [`split`] but with an explicit palette.
pub fn split_with(buf: &mut Buffer, bounds: Rect, split: &SplitPane, palette: &Palette) {
    let first = split.first_rect(&bounds);
    let (fx, fy, fw, fh) = to_cell_rect(first);
    if fw == 0 || fh == 0 {
        return;
    }
    let style = Style::default().fg(palette.border);
    match split.orientation() {
        egaku::Orientation::Horizontal => {
            // Vertical line at the right edge of `first`.
            let col = fx + fw;
            for r in 0..fh {
                buf.set_char(col, fy + r, '│', style);
            }
        }
        egaku::Orientation::Vertical => {
            // Horizontal line at the bottom edge of `first`.
            let row = fy + fh;
            buf.hline(fx, row, fw, '─', style);
        }
    }
}

// ---------- Header / banner --------------------------------------------------

/// Render a single-line bold header at `(rect.x, rect.y)` truncated to
/// `rect.width`.
pub fn header(buf: &mut Buffer, rect: Rect, text: &str) {
    header_with(buf, rect, text, &Palette::default());
}

/// Like [`header`] but with an explicit palette.
pub fn header_with(buf: &mut Buffer, rect: Rect, text: &str, palette: &Palette) {
    let (x, y, w, _h) = to_cell_rect(rect);
    if w == 0 {
        return;
    }
    let style = Style::default().fg(palette.accent).bold();
    buf.set_stringn(x, y, text, w, style);
}

// ---------- Paragraph (multiline text with word wrap) -----------------------

/// Word-wrap `text` to fit `width` columns. Lines that contain a word
/// longer than `width` are broken at the column boundary. Empty input
/// returns an empty Vec; explicit `\n`s are honored as paragraph breaks.
#[must_use]
pub fn wrap_text(text: &str, width: u16) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let max = usize::from(width);
    let mut out = Vec::new();
    for raw_line in text.split('\n') {
        if raw_line.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut cur_w = 0usize;
        for word in raw_line.split_whitespace() {
            let w = word.width();
            if w > max {
                // Word longer than the line — flush current, then break the
                // word at column boundaries.
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                    cur_w = 0;
                }
                let mut chunk = String::new();
                let mut chunk_w = 0usize;
                for ch in word.chars() {
                    let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if chunk_w + cw > max && !chunk.is_empty() {
                        out.push(std::mem::take(&mut chunk));
                        chunk_w = 0;
                    }
                    chunk.push(ch);
                    chunk_w += cw;
                }
                if !chunk.is_empty() {
                    out.push(chunk);
                }
                continue;
            }
            let needed = if current.is_empty() { w } else { cur_w + 1 + w };
            if needed > max {
                out.push(std::mem::take(&mut current));
                cur_w = 0;
            }
            if !current.is_empty() {
                current.push(' ');
                cur_w += 1;
            }
            current.push_str(word);
            cur_w += w;
        }
        if !current.is_empty() || raw_line.chars().all(char::is_whitespace) {
            out.push(current);
        }
    }
    out
}

/// Render a multi-line paragraph inside `rect`, word-wrapped to the rect
/// width and truncated to the rect height. Lines longer than the height
/// are dropped — callers that want scrolling pair this with an
/// [`egaku::ScrollView`] and offset their input.
pub fn paragraph(buf: &mut Buffer, rect: Rect, text: &str) {
    paragraph_with(buf, rect, text, &Palette::default());
}

/// Like [`paragraph`] but with an explicit palette.
pub fn paragraph_with(buf: &mut Buffer, rect: Rect, text: &str, palette: &Palette) {
    let (x, y, w, h) = to_cell_rect(rect);
    if w == 0 || h == 0 {
        return;
    }
    let style = Style::default().fg(palette.foreground);
    for (i, line) in wrap_text(text, w).iter().enumerate().take(usize::from(h)) {
        let row_idx = u16::try_from(i).unwrap_or(u16::MAX);
        buf.set_stringn(x, y + row_idx, line, w, style);
    }
}

// ---------- BorderedBlock (titled box wrapping a child rect) ----------------

/// Rectangle inset where a [`bordered_block`]'s child content should
/// render. Returns the inner rect (border subtracted) for downstream
/// drawers to use as their target.
///
/// The border occupies the outermost ring of `rect`; the inner rect is
/// `rect` with `(x+1, y+1, w-2, h-2)`.
#[must_use]
pub fn block_inner(rect: Rect) -> Rect {
    let inner_w = (rect.width - 2.0).max(0.0);
    let inner_h = (rect.height - 2.0).max(0.0);
    Rect::new(rect.x + 1.0, rect.y + 1.0, inner_w, inner_h)
}

/// Render a single-line border around `rect` with `title` embedded in the
/// top edge. The interior is left untouched — call this BEFORE drawing
/// child widgets, then draw children into [`block_inner(rect)`].
///
/// `focused` toggles the border color between accent (focused) and border
/// (unfocused).
pub fn bordered_block(buf: &mut Buffer, rect: Rect, title: &str, focused: bool) {
    bordered_block_with(buf, rect, title, focused, &Palette::default());
}

/// Like [`bordered_block`] but with an explicit palette.
pub fn bordered_block_with(
    buf: &mut Buffer,
    rect: Rect,
    title: &str,
    focused: bool,
    palette: &Palette,
) {
    let (x, y, w, h) = to_cell_rect(rect);
    if w < 2 || h < 2 {
        return;
    }
    let color = if focused {
        palette.accent
    } else {
        palette.border
    };
    let style = Style::default().fg(color);
    let right = x + w - 1;
    let inner = w - 2;

    // Top: ┌─...─┐ with the title embedded near the left over the dashes.
    buf.hline(x + 1, y, inner, '─', style);
    buf.set_char(x, y, '┌', style);
    buf.set_char(right, y, '┐', style);
    if !title.is_empty() && inner > 0 {
        let mut cx = buf.set_stringn(x + 1, y, " ", inner, style);
        let title_budget = right.saturating_sub(cx);
        cx = buf.set_stringn(cx, y, title, title_budget, style);
        if cx < right {
            buf.set_char(cx, y, ' ', style);
        }
    }

    // Middle rows: │   ...   │  (don't paint the interior — caller does).
    for r in 1..(h - 1) {
        buf.set_char(x, y + r, '│', style);
        buf.set_char(right, y + r, '│', style);
    }

    // Bottom: └────...────┘
    let bottom = y + h - 1;
    buf.hline(x + 1, bottom, inner, '─', style);
    buf.set_char(x, bottom, '└', style);
    buf.set_char(right, bottom, '┘', style);
}

// ---------- StatusLine (left + spacer + right tri-section) ------------------

/// Render a single-row status bar: `left` text flush-left, `right` text
/// flush-right, padding between them. Uses the palette's `selection`
/// background for the bar so it visually separates from the content above
/// and below.
///
/// If both segments together exceed the rect width, the right segment is
/// truncated first (left is the "current state" usually authored by the
/// app and more important to keep readable).
pub fn status_line(buf: &mut Buffer, rect: Rect, left: &str, right: &str) {
    status_line_with(buf, rect, left, right, &Palette::default());
}

/// Like [`status_line`] but with an explicit palette.
pub fn status_line_with(buf: &mut Buffer, rect: Rect, left: &str, right: &str, palette: &Palette) {
    let (x, y, w, _h) = to_cell_rect(rect);
    if w == 0 {
        return;
    }
    let style = Style::default()
        .fg(palette.foreground)
        .bg(palette.selection);

    // Paint the whole bar first so the gap between the segments carries the
    // status background.
    buf.blank(x, y, w, style);

    let left_str = truncate_to_width(left, w);
    let left_w = u16::try_from(left_str.width()).unwrap_or(w).min(w);
    buf.set_stringn(x, y, &left_str, w, style);

    let right_budget = w - left_w;
    let right_str = truncate_to_width(right, right_budget);
    let right_w = u16::try_from(right_str.width()).unwrap_or(0);
    let right_x = x + w - right_w;
    buf.set_stringn(right_x, y, &right_str, right_w, style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestBackend;
    use egaku::{Column, IDENTITY_FIELD, SortKey, SortOrder};

    // ---- table fixtures ----------------------------------------------------

    struct TestRow {
        name: String,
        cells: Vec<(String, String)>,
    }

    impl TableRow for TestRow {
        fn identity(&self) -> &str {
            &self.name
        }

        fn cell(&self, field: &str) -> Option<&str> {
            self.cells
                .iter()
                .find(|(k, _)| k == field)
                .map(|(_, v)| v.as_str())
        }
    }

    fn trow(name: &str, status: &str) -> TestRow {
        TestRow {
            name: name.into(),
            cells: vec![("phase".into(), status.into())],
        }
    }

    /// Sorted by NAME ascending, so row order in every table test is exactly
    /// the order the fixture names them.
    fn ttable(rows: Vec<TestRow>) -> TableView<TestRow> {
        TableView::new(
            vec![
                Column::new("NAME", IDENTITY_FIELD),
                Column::new("STATUS", "phase"),
            ],
            rows,
            SortKey::new("NAME", SortOrder::Asc),
        )
        .expect("NAME is declared")
    }

    // ---- table drawer ------------------------------------------------------

    #[test]
    fn table_header_row_carries_the_column_headers_in_order() {
        let t = ttable(vec![trow("catch-0", "Running")]);
        let mut backend = TestBackend::new(40, 5);
        backend.draw(|buf| table(buf, Rect::new(0.0, 0.0, 40.0, 5.0), &t, true));
        let lines = backend.to_lines();
        assert_eq!(lines[0], "NAME     STATUS");
        assert!(
            lines[1].starts_with('─'),
            "row 1 is the rule, got {:?}",
            lines[1]
        );
        assert_eq!(lines[2], "catch-0  Running");
    }

    #[test]
    fn table_columns_are_padded_to_the_widest_value_not_the_header() {
        // "a-very-long-name" is wider than "NAME", so STATUS must shift right
        // on BOTH the header row and the data row by the same amount.
        let t = ttable(vec![trow("a-very-long-name", "Running")]);
        let mut backend = TestBackend::new(40, 5);
        backend.draw(|buf| table(buf, Rect::new(0.0, 0.0, 40.0, 5.0), &t, true));
        let lines = backend.to_lines();
        let header_col = lines[0].find("STATUS").expect("header present");
        let data_col = lines[2].find("Running").expect("value present");
        assert_eq!(
            header_col, data_col,
            "header and cell share a column origin"
        );
    }

    #[test]
    fn table_column_widths_measure_display_width_not_char_count() {
        // Each CJK glyph occupies two terminal cells. Measuring `chars().count()`
        // instead of display width under-counts by half and slides every column
        // to its right out of alignment.
        let t = ttable(vec![trow("日本語", "Running")]);
        let widths = table_column_widths(&t);
        assert_eq!(widths[0], 6, "three double-width glyphs = 6 cells");
    }

    #[test]
    fn table_selected_row_carries_the_selection_background() {
        let t = ttable(vec![trow("a", "Running"), trow("b", "Running")]);
        let palette = Palette::default();
        let mut backend = TestBackend::new(40, 6);
        backend.draw(|buf| table_with(buf, Rect::new(0.0, 0.0, 40.0, 6.0), &t, true, &palette));
        // Row 0 header, row 1 rule, row 2 = first (selected) data row.
        let selected = backend.cell(0, 2).expect("selected cell");
        let other = backend.cell(0, 3).expect("unselected cell");
        assert_eq!(selected.bg, palette.selection);
        assert_ne!(other.bg, palette.selection);
    }

    #[test]
    fn table_selection_bar_spans_the_full_width_past_the_last_column() {
        let t = ttable(vec![trow("a", "Running")]);
        let palette = Palette::default();
        let mut backend = TestBackend::new(40, 6);
        backend.draw(|buf| table_with(buf, Rect::new(0.0, 0.0, 40.0, 6.0), &t, true, &palette));
        let far_right = backend.cell(39, 2).expect("rightmost cell of the row");
        assert_eq!(
            far_right.bg, palette.selection,
            "the bar must reach the right edge, not stop at the last column"
        );
    }

    #[test]
    fn table_selected_row_shows_the_row_never_the_header() {
        // Regression class from the model this drawer was lifted from: a
        // single `Option<&Row>` meant both "header row" and "selected data row"
        // there, so the selected row drew the column headers.
        let t = ttable(vec![trow("catch-0", "Running")]);
        let mut backend = TestBackend::new(40, 6);
        backend.draw(|buf| table(buf, Rect::new(0.0, 0.0, 40.0, 6.0), &t, true));
        let data_row = &backend.to_lines()[2];
        assert!(data_row.starts_with("catch-0"), "got {data_row:?}");
    }

    #[test]
    fn table_viewport_scrolls_to_keep_the_selection_visible() {
        // 6 rows into 3 data rows of screen. With the cursor on the last row
        // the window must be bottom-anchored on it — the model has no offset,
        // so this is entirely the drawer's derivation.
        let mut t = ttable((0..6).map(|i| trow(&format!("r{i}"), "x")).collect());
        for _ in 0..5 {
            t.select_next();
        }
        assert_eq!(t.selected_index(), 5);

        let mut backend = TestBackend::new(40, 5); // 5 - header - rule = 3 rows
        backend.draw(|buf| table(buf, Rect::new(0.0, 0.0, 40.0, 5.0), &t, true));
        let lines = backend.to_lines();
        assert!(lines[2].starts_with("r3"), "got {:?}", lines[2]);
        assert!(lines[3].starts_with("r4"), "got {:?}", lines[3]);
        assert!(
            lines[4].starts_with("r5"),
            "the selected row must be on screen, got {:?}",
            lines[4]
        );
    }

    #[test]
    fn table_viewport_stays_at_the_top_while_the_selection_fits() {
        let mut t = ttable((0..6).map(|i| trow(&format!("r{i}"), "x")).collect());
        t.select_next(); // index 1, well inside a 3-row window
        let mut backend = TestBackend::new(40, 5);
        backend.draw(|buf| table(buf, Rect::new(0.0, 0.0, 40.0, 5.0), &t, true));
        let lines = backend.to_lines();
        assert!(
            lines[2].starts_with("r0"),
            "no premature scroll, got {:?}",
            lines[2]
        );
    }

    #[test]
    fn table_with_no_room_for_data_rows_still_draws_the_header() {
        let t = ttable(vec![trow("a", "Running")]);
        let mut backend = TestBackend::new(40, 2);
        backend.draw(|buf| table(buf, Rect::new(0.0, 0.0, 40.0, 2.0), &t, true));
        let lines = backend.to_lines();
        assert_eq!(lines[0], "NAME  STATUS");
        assert!(lines[1].starts_with('─'));
    }

    #[test]
    fn table_zero_sized_rect_is_a_noop() {
        let t = ttable(vec![trow("a", "Running")]);
        let mut backend = TestBackend::new(10, 3);
        backend.draw(|buf| table(buf, Rect::new(0.0, 0.0, 0.0, 0.0), &t, true));
        assert!(
            backend.to_lines().iter().all(String::is_empty),
            "nothing drawn"
        );
    }

    #[test]
    fn table_of_no_rows_draws_only_the_header_and_rule() {
        let t = ttable(vec![]);
        let mut backend = TestBackend::new(20, 4);
        backend.draw(|buf| table(buf, Rect::new(0.0, 0.0, 20.0, 4.0), &t, true));
        let lines = backend.to_lines();
        assert_eq!(lines[0], "NAME  STATUS");
        assert!(lines[2].is_empty() && lines[3].is_empty());
    }

    // ---- the Draw trait ----------------------------------------------------

    #[test]
    fn draw_trait_is_object_safe_and_renders_a_heterogeneous_set() {
        let mut list = ListView::new(vec!["one".into()], 3);
        list.set_focused(true);
        let mut bar = TabBar::new(vec!["alpha".into()]);
        bar.set_focused(true);
        let widgets: Vec<&dyn Draw> = vec![&list, &bar];

        let palette = Palette::default();
        let mut backend = TestBackend::new(20, 2);
        backend.draw(|buf| {
            for (i, w) in widgets.iter().enumerate() {
                #[allow(clippy::cast_precision_loss)]
                w.draw(buf, Rect::new(0.0, i as f32, 20.0, 1.0), &palette);
            }
        });
        let lines = backend.to_lines();
        assert!(
            lines[0].contains("one"),
            "the list drew, got {:?}",
            lines[0]
        );
        assert!(
            lines[1].contains("alpha"),
            "the tab bar drew, got {:?}",
            lines[1]
        );
    }

    #[test]
    fn draw_sources_focus_from_the_widget_not_the_call_site() {
        // The whole reason ListView grew a self-owned focus flag: `draw` has no
        // focus argument, so the rendered frame must still differ by focus.
        // The focus signal in `list_with` is BOLD on the selected row (the `▶ `
        // marker tracks selection, not focus — see `list`'s docs).
        let palette = Palette::default();

        let render = |list: &ListView| {
            let mut backend = TestBackend::new(20, 1);
            backend.draw(|buf| list.draw(buf, Rect::new(0.0, 0.0, 20.0, 1.0), &palette));
            backend
        };

        let unfocused = ListView::new(vec!["one".into()], 3);
        let mut focused = ListView::new(vec!["one".into()], 3);
        focused.set_focused(true);

        let dim = render(&unfocused);
        let bright = render(&focused);

        assert!(
            !dim.cell(0, 0)
                .unwrap()
                .modifiers
                .contains(crate::Modifiers::BOLD),
            "an unfocused list must not render bold"
        );
        assert!(
            bright
                .cell(0, 0)
                .unwrap()
                .modifiers
                .contains(crate::Modifiers::BOLD),
            "a focused list renders its selected row bold — sourced from the \
             widget's own flag, with no focus argument at the call site"
        );
    }

    #[test]
    fn draw_for_table_view_matches_the_free_function() {
        // The trait impl must be a pure re-dispatch — no second layout.
        let mut t = ttable(vec![trow("a", "Running"), trow("b", "Pending")]);
        t.set_focused(true);
        let palette = Palette::default();
        let rect = Rect::new(0.0, 0.0, 30.0, 5.0);

        let mut via_trait = TestBackend::new(30, 5);
        via_trait.draw(|buf| Draw::draw(&t, buf, rect, &palette));

        let mut via_fn = TestBackend::new(30, 5);
        via_fn.draw(|buf| table_with(buf, rect, &t, true, &palette));

        assert_eq!(via_trait.buffer().cells(), via_fn.buffer().cells());
    }

    #[test]
    fn draw_for_split_pane_draws_the_divider() {
        let sp = SplitPane::horizontal();
        let palette = Palette::default();
        let mut backend = TestBackend::new(10, 2);
        backend.draw(|buf| sp.draw(buf, Rect::new(0.0, 0.0, 10.0, 2.0), &palette));
        assert!(
            backend.to_lines().iter().any(|l| l.contains('│')),
            "divider drawn"
        );
    }

    #[test]
    fn draw_for_scroll_view_draws_the_gutter() {
        let sv = ScrollView::new(4.0, 20.0);
        let palette = Palette::default();
        let mut backend = TestBackend::new(6, 4);
        backend.draw(|buf| sv.draw(buf, Rect::new(0.0, 0.0, 6.0, 4.0), &palette));
        let col = (0..4)
            .filter_map(|y| backend.cell(5, y))
            .filter(|c| c.symbol() != " ")
            .count();
        assert_eq!(col, 4, "the right column carries the indicator");
    }

    // ---- pre-existing helpers ---------------------------------------------

    #[test]
    fn truncate_to_width_basic() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
        assert_eq!(truncate_to_width("hello world", 5), "hello");
    }

    #[test]
    fn truncate_to_width_zero() {
        assert_eq!(truncate_to_width("anything", 0), "");
    }

    #[test]
    fn truncate_to_width_cjk() {
        // Each CJK char is width 2
        assert_eq!(truncate_to_width("日本語", 4), "日本");
        assert_eq!(truncate_to_width("日本語", 6), "日本語");
    }

    #[test]
    fn to_cell_rect_rounds() {
        let (x, y, w, h) = to_cell_rect(Rect::new(1.4, 2.6, 10.5, 4.0));
        assert_eq!((x, y, w, h), (1, 3, 11, 4));
    }

    #[test]
    fn to_cell_rect_clamps_negative() {
        let (x, y, _, _) = to_cell_rect(Rect::new(-5.0, -1.0, 10.0, 4.0));
        assert_eq!((x, y), (0, 0));
    }

    #[test]
    fn wrap_text_simple() {
        let lines = wrap_text("the quick brown fox jumps", 10);
        assert_eq!(lines, vec!["the quick", "brown fox", "jumps"]);
    }

    #[test]
    fn wrap_text_zero_width() {
        assert!(wrap_text("anything", 0).is_empty());
    }

    #[test]
    fn wrap_text_preserves_paragraph_breaks() {
        let lines = wrap_text("first line\n\nsecond line", 20);
        assert_eq!(
            lines,
            vec![
                "first line".to_string(),
                String::new(),
                "second line".to_string()
            ]
        );
    }

    #[test]
    fn wrap_text_breaks_oversized_word() {
        let lines = wrap_text("aaaaaaaa word", 4);
        // "aaaaaaaa" is 8 wide; gets broken into "aaaa","aaaa", then "word"
        assert_eq!(lines, vec!["aaaa", "aaaa", "word"]);
    }

    #[test]
    fn block_inner_subtracts_border() {
        let inner = block_inner(Rect::new(0.0, 0.0, 10.0, 5.0));
        assert_eq!(inner, Rect::new(1.0, 1.0, 8.0, 3.0));
    }

    #[test]
    #[allow(clippy::float_cmp)] // `.max(0.0)` yields exactly 0.0 — exact compare is correct.
    fn block_inner_zero_floor() {
        let inner = block_inner(Rect::new(0.0, 0.0, 1.0, 1.0));
        assert_eq!(inner.width, 0.0);
        assert_eq!(inner.height, 0.0);
    }
}
