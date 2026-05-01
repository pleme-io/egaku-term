//! Drawers for [`egaku`] widgets onto a terminal.
//!
//! Every drawer takes the widget by reference, a [`Rect`](egaku::Rect)
//! describing where to render (in cell coordinates — `1.0 == one terminal
//! cell`), a [`Palette`](crate::theme::Palette) for colors, and a `focused`
//! flag so widgets can dim when inactive. Drawers queue commands onto the
//! terminal but do not flush — the caller flushes once per frame.

use crossterm::{
    QueueableCommand,
    cursor::MoveTo,
    style::{
        Attribute, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
};
use egaku::{ListView, Modal, Rect, ScrollView, SplitPane, TabBar, TextInput};
use unicode_width::UnicodeWidthStr;

use crate::error::Result;
use crate::terminal::Terminal;
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

/// Print a single line at `(col, row)` truncated to `max_cols`. The cursor
/// is left at the end of the printed run; callers shouldn't depend on that.
fn print_at(term: &mut Terminal, col: u16, row: u16, max_cols: u16, text: &str) -> Result<()> {
    let line = truncate_to_width(text, max_cols);
    term.out().queue(MoveTo(col, row))?.queue(Print(line))?;
    Ok(())
}

// ---------- ListView ---------------------------------------------------------

/// Render a [`ListView`] inside `rect`. Selected row is rendered with the
/// palette's selection background; a `▶ ` gutter glyph marks it on focused
/// lists, and a 2-space gutter is reserved on unfocused lists for visual
/// stability.
pub fn list(term: &mut Terminal, rect: Rect, list: &ListView, focused: bool) -> Result<()> {
    list_with(term, rect, list, focused, &Palette::default())
}

/// Like [`list`] but with an explicit palette.
pub fn list_with(
    term: &mut Terminal,
    rect: Rect,
    list: &ListView,
    focused: bool,
    palette: &Palette,
) -> Result<()> {
    let (x, y, w, h) = to_cell_rect(rect);
    if w == 0 || h == 0 {
        return Ok(());
    }

    let visible = list.visible_items();
    let offset = list.offset();
    for (i, item) in visible.iter().enumerate() {
        let row_idx = u16::try_from(i).unwrap_or(u16::MAX);
        if row_idx >= h {
            break;
        }
        let absolute_idx = offset + i;
        let is_selected = absolute_idx == list.selected_index();

        let prefix = if is_selected { "▶ " } else { "  " };
        let line = format!("{prefix}{item}");

        if is_selected {
            term.out()
                .queue(SetBackgroundColor(palette.selection))?
                .queue(SetForegroundColor(palette.foreground))?;
            if focused {
                term.out().queue(SetAttribute(Attribute::Bold))?;
            }
        }

        print_at(term, x, y + row_idx, w, &line)?;

        // Pad selected row to full width for a clean highlight bar.
        if is_selected {
            let used = u16::try_from(line.width()).unwrap_or(w).min(w);
            if used < w {
                term.out().queue(Print(" ".repeat(usize::from(w - used))))?;
            }
            term.out().queue(ResetColor)?.queue(SetAttribute(Attribute::Reset))?;
        }
    }

    Ok(())
}

// ---------- TextInput --------------------------------------------------------

/// Render a [`TextInput`] on a single row of `rect`. When `focused`, the
/// cursor block is drawn at the input's cursor position; otherwise a dim
/// underline is drawn below the visible text.
pub fn text_input(term: &mut Terminal, rect: Rect, input: &TextInput, focused: bool) -> Result<()> {
    text_input_with(term, rect, input, focused, &Palette::default())
}

/// Like [`text_input`] but with an explicit palette.
pub fn text_input_with(
    term: &mut Terminal,
    rect: Rect,
    input: &TextInput,
    focused: bool,
    palette: &Palette,
) -> Result<()> {
    let (x, y, w, _h) = to_cell_rect(rect);
    if w == 0 {
        return Ok(());
    }

    let text = input.text();
    let visible = truncate_to_width(text, w);

    if focused {
        term.out().queue(SetForegroundColor(palette.foreground))?;
    } else {
        term.out().queue(SetForegroundColor(palette.muted))?;
    }
    term.out().queue(MoveTo(x, y))?.queue(Print(&visible))?;

    if focused {
        // Block-style cursor: redraw the byte at cursor with reverse attr.
        let cursor_byte = input.cursor();
        let prefix_width = u16::try_from(text[..cursor_byte.min(text.len())].width()).unwrap_or(0);
        let cursor_col = x + prefix_width.min(w.saturating_sub(1));
        let cursor_glyph = text[cursor_byte..]
            .chars()
            .next()
            .map_or(' ', |c| c);
        term.out()
            .queue(MoveTo(cursor_col, y))?
            .queue(SetAttribute(Attribute::Reverse))?
            .queue(Print(cursor_glyph))?
            .queue(SetAttribute(Attribute::Reset))?;
    }
    term.out().queue(ResetColor)?;
    Ok(())
}

// ---------- TabBar -----------------------------------------------------------

/// Render a [`TabBar`] as a single row of `[ tab ]  [ tab ]  ...`. The
/// active tab is reverse-video; a focused bar bolds it.
pub fn tabs(term: &mut Terminal, rect: Rect, bar: &TabBar, focused: bool) -> Result<()> {
    tabs_with(term, rect, bar, focused, &Palette::default())
}

/// Like [`tabs`] but with an explicit palette.
pub fn tabs_with(
    term: &mut Terminal,
    rect: Rect,
    bar: &TabBar,
    focused: bool,
    palette: &Palette,
) -> Result<()> {
    let (x, y, w, _h) = to_cell_rect(rect);
    if w == 0 {
        return Ok(());
    }

    let mut col: u16 = 0;
    for (i, name) in bar.tabs().iter().enumerate() {
        let label = format!(" {name} ");
        let label_w = u16::try_from(label.width()).unwrap_or(w);
        if col + label_w + 1 > w {
            break;
        }

        let is_active = i == bar.active_index();
        if is_active {
            term.out()
                .queue(SetBackgroundColor(palette.accent))?
                .queue(SetForegroundColor(palette.background))?;
            if focused {
                term.out().queue(SetAttribute(Attribute::Bold))?;
            }
        } else {
            term.out().queue(SetForegroundColor(palette.muted))?;
        }
        term.out()
            .queue(MoveTo(x + col, y))?
            .queue(Print(&label))?
            .queue(ResetColor)?
            .queue(SetAttribute(Attribute::Reset))?;
        col += label_w + 1; // +1 spacer
    }
    Ok(())
}

// ---------- Modal ------------------------------------------------------------

/// Render a [`Modal`] centered inside `bounds`. Skips entirely when the
/// modal is not visible, so callers can call this unconditionally each
/// frame. The body is supplied as a slice of pre-wrapped lines.
pub fn modal(
    term: &mut Terminal,
    bounds: Rect,
    modal: &Modal,
    body: &[&str],
) -> Result<()> {
    modal_with(term, bounds, modal, body, &Palette::default())
}

/// Like [`modal`] but with an explicit palette.
pub fn modal_with(
    term: &mut Terminal,
    bounds: Rect,
    modal: &Modal,
    body: &[&str],
    palette: &Palette,
) -> Result<()> {
    if !modal.is_visible() {
        return Ok(());
    }
    let (bx, by, bw, bh) = to_cell_rect(bounds);
    if bw < 6 || bh < 4 {
        return Ok(());
    }

    // Compute box size: at most 80% of bounds, at least enough for the
    // longest line + 4 cells of padding.
    let max_content_w = body.iter().map(|s| s.width()).max().unwrap_or(0);
    let title_w = modal.title().width();
    let want_w = max_content_w.max(title_w) + 4;
    let want_h = body.len() + 4;

    let box_w = u16::try_from(want_w).unwrap_or(bw).min(bw * 4 / 5);
    let box_h = u16::try_from(want_h).unwrap_or(bh).min(bh * 4 / 5);
    let box_x = bx + (bw.saturating_sub(box_w)) / 2;
    let box_y = by + (bh.saturating_sub(box_h)) / 2;

    term.out()
        .queue(SetForegroundColor(palette.border))?
        .queue(SetBackgroundColor(palette.background))?;

    // Top border with title
    let title = format!("─ {} ", modal.title());
    let title_w_u = u16::try_from(title.width()).unwrap_or(box_w);
    let pad = box_w
        .saturating_sub(2)
        .saturating_sub(title_w_u);
    let top = format!("┌{title}{}┐", "─".repeat(usize::from(pad)));
    print_at(term, box_x, box_y, box_w, &top)?;

    // Body rows
    for (i, line) in body.iter().enumerate() {
        let row_idx = u16::try_from(i + 1).unwrap_or(u16::MAX);
        if row_idx >= box_h - 1 {
            break;
        }
        let inner_w = box_w.saturating_sub(2);
        let truncated = truncate_to_width(line, inner_w.saturating_sub(2));
        let used = u16::try_from(truncated.width()).unwrap_or(0);
        let pad_right = inner_w.saturating_sub(used + 2);
        let row = format!("│ {truncated}{} │", " ".repeat(usize::from(pad_right)));
        print_at(term, box_x, box_y + row_idx, box_w, &row)?;
    }

    // Fill remaining body rows with empty interior.
    for r in (body.len() + 1)..usize::from(box_h - 1) {
        let row = format!("│{}│", " ".repeat(usize::from(box_w - 2)));
        let row_idx = u16::try_from(r).unwrap_or(u16::MAX);
        print_at(term, box_x, box_y + row_idx, box_w, &row)?;
    }

    // Bottom border
    let bottom = format!("└{}┘", "─".repeat(usize::from(box_w - 2)));
    print_at(term, box_x, box_y + box_h - 1, box_w, &bottom)?;

    term.out().queue(ResetColor)?;
    Ok(())
}

// ---------- ScrollView indicator --------------------------------------------

/// Render a one-column scroll indicator on the right edge of `rect`. The
/// thumb's relative position reflects [`ScrollView::scroll_fraction`]; the
/// thumb size reflects the viewport-to-content ratio.
pub fn scrollbar(term: &mut Terminal, rect: Rect, scroll: &ScrollView) -> Result<()> {
    scrollbar_with(term, rect, scroll, &Palette::default())
}

/// Like [`scrollbar`] but with an explicit palette.
pub fn scrollbar_with(
    term: &mut Terminal,
    rect: Rect,
    scroll: &ScrollView,
    palette: &Palette,
) -> Result<()> {
    let (x, y, w, h) = to_cell_rect(rect);
    if w == 0 || h == 0 {
        return Ok(());
    }
    let col = x + w - 1;

    if scroll.max_scroll() <= 0.0 {
        // No scrolling needed — draw the gutter dim for visual consistency.
        term.out().queue(SetForegroundColor(palette.muted))?;
        for r in 0..h {
            print_at(term, col, y + r, 1, "│")?;
        }
        term.out().queue(ResetColor)?;
        return Ok(());
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

    term.out().queue(SetForegroundColor(palette.muted))?;
    for r in 0..h {
        let glyph = if r >= thumb_top && r < thumb_top + thumb_size {
            "█"
        } else {
            "│"
        };
        print_at(term, col, y + r, 1, glyph)?;
    }
    term.out().queue(ResetColor)?;
    Ok(())
}

// ---------- SplitPane border -------------------------------------------------

/// Render the divider line between a [`SplitPane`]'s two children inside
/// `bounds`. Children themselves render via the panes' [`SplitPane::first_rect`]
/// / [`SplitPane::second_rect`] coordinates.
pub fn split(term: &mut Terminal, bounds: Rect, split: &SplitPane) -> Result<()> {
    split_with(term, bounds, split, &Palette::default())
}

/// Like [`split`] but with an explicit palette.
pub fn split_with(
    term: &mut Terminal,
    bounds: Rect,
    split: &SplitPane,
    palette: &Palette,
) -> Result<()> {
    let first = split.first_rect(&bounds);
    let (fx, fy, fw, fh) = to_cell_rect(first);
    if fw == 0 || fh == 0 {
        return Ok(());
    }
    term.out().queue(SetForegroundColor(palette.border))?;
    match split.orientation() {
        egaku::Orientation::Horizontal => {
            // Vertical line at the right edge of `first`
            let col = fx + fw;
            for r in 0..fh {
                print_at(term, col, fy + r, 1, "│")?;
            }
        }
        egaku::Orientation::Vertical => {
            // Horizontal line at the bottom edge of `first`
            let row = fy + fh;
            print_at(term, fx, row, fw, &"─".repeat(usize::from(fw)))?;
        }
    }
    term.out().queue(ResetColor)?;
    Ok(())
}

// ---------- Header / banner --------------------------------------------------

/// Render a single-line bold header at `(rect.x, rect.y)` truncated to
/// `rect.width`.
pub fn header(term: &mut Terminal, rect: Rect, text: &str) -> Result<()> {
    header_with(term, rect, text, &Palette::default())
}

/// Like [`header`] but with an explicit palette.
pub fn header_with(
    term: &mut Terminal,
    rect: Rect,
    text: &str,
    palette: &Palette,
) -> Result<()> {
    let (x, y, w, _h) = to_cell_rect(rect);
    if w == 0 {
        return Ok(());
    }
    term.out()
        .queue(SetAttribute(Attribute::Bold))?
        .queue(SetForegroundColor(palette.accent))?;
    print_at(term, x, y, w, text)?;
    term.out()
        .queue(SetAttribute(Attribute::Reset))?
        .queue(ResetColor)?;
    Ok(())
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
}
