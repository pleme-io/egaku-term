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

use egaku::{ListView, Modal, Rect, ScrollView, SplitPane, TabBar, TextInput};
use unicode_width::UnicodeWidthStr;

use crate::buffer::Buffer;
use crate::cell::Style;
use crate::theme::Palette;

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

/// Render a [`ListView`] inside `rect`. Selected row is rendered with the
/// palette's selection background; a `▶ ` gutter glyph marks it on focused
/// lists, and a 2-space gutter is reserved on unfocused lists for visual
/// stability.
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
