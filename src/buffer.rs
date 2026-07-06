//! The typed back buffer — a `width × height` grid of [`Cell`]s.
//!
//! `Buffer` is the load-bearing M0 primitive (Quadro §0 / P5 / P6): it is
//! simultaneously
//!
//! 1. the **typed emission surface** — drawers write [`Cell`]s via typed ops
//!    ([`Buffer::set_stringn`], [`Buffer::set_char`], [`Buffer::blank`],
//!    [`Buffer::hline`]); no drawer ever `format!()`s a styled string or
//!    hand-spells an escape;
//! 2. the input to the **diff renderer** — [`Buffer::diff`] returns only the
//!    cells that changed between a front and a back buffer, so the runtime
//!    flushes changed cells instead of full-clearing every frame; and
//! 3. the **headless capture surface** — a [`TestBackend`](crate::TestBackend)
//!    renders a frame into a `Buffer` and reads it back for golden-frame tests.
//!
//! Coordinates are cells: `(0, 0)` is the top-left. Wide (CJK / emoji) glyphs
//! occupy one cell holding the glyph plus a continuation cell with an empty
//! symbol; [`Buffer::diff`] and [`crate::render`] both honour display width.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::cell::{Cell, Style};

/// A `width × height` grid of attributed [`Cell`]s, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Buffer {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
}

impl Buffer {
    /// A buffer of blank default cells sized `width × height`.
    #[must_use]
    pub fn empty(width: u16, height: u16) -> Self {
        let len = usize::from(width) * usize::from(height);
        Self {
            width,
            height,
            cells: vec![Cell::default(); len],
        }
    }

    /// Buffer width in cells.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Buffer height in cells.
    #[must_use]
    pub fn height(&self) -> u16 {
        self.height
    }

    /// The whole cell slice, row-major.
    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Reset every cell to a blank default cell (reusing allocations). Called
    /// once per frame before drawing the back buffer.
    pub fn reset(&mut self) {
        for cell in &mut self.cells {
            cell.reset();
        }
    }

    /// Resize to `width × height`, clearing all cells. A no-op when the size
    /// is unchanged.
    pub fn resize(&mut self, width: u16, height: u16) {
        if width == self.width && height == self.height {
            self.reset();
            return;
        }
        let len = usize::from(width) * usize::from(height);
        self.cells.clear();
        self.cells.resize(len, Cell::default());
        self.width = width;
        self.height = height;
    }

    #[inline]
    fn index_of(&self, x: u16, y: u16) -> Option<usize> {
        if x < self.width && y < self.height {
            Some(usize::from(y) * usize::from(self.width) + usize::from(x))
        } else {
            None
        }
    }

    #[inline]
    #[allow(clippy::cast_possible_truncation)]
    fn pos_of(&self, i: usize) -> (u16, u16) {
        debug_assert!(self.width > 0, "pos_of called on a zero-width buffer");
        let w = usize::from(self.width);
        ((i % w) as u16, (i / w) as u16)
    }

    /// Borrow the cell at `(x, y)`, if in bounds.
    #[must_use]
    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        self.index_of(x, y).map(|i| &self.cells[i])
    }

    /// Mutably borrow the cell at `(x, y)`, if in bounds.
    pub fn get_mut(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        match self.index_of(x, y) {
            Some(i) => Some(&mut self.cells[i]),
            None => None,
        }
    }

    /// Write `s` starting at `(x, y)`, styled, clipped to `max_cols` columns
    /// and to the buffer's right edge. Returns the x column *after* the last
    /// glyph written (i.e. where the next write would begin).
    ///
    /// Zero-width graphemes (control chars, standalone combining marks) are
    /// skipped. Wide graphemes write the glyph plus a continuation cell.
    #[allow(clippy::cast_possible_truncation)]
    pub fn set_stringn(&mut self, x: u16, y: u16, s: &str, max_cols: u16, style: Style) -> u16 {
        if y >= self.height {
            return x;
        }
        let budget_end = x.saturating_add(max_cols).min(self.width);
        let mut cur_x = x;
        for grapheme in s.graphemes(true) {
            let gw = UnicodeWidthStr::width(grapheme);
            if gw == 0 {
                continue;
            }
            let gw = gw as u16;
            if cur_x >= budget_end || cur_x.saturating_add(gw) > budget_end {
                break;
            }
            if let Some(i) = self.index_of(cur_x, y) {
                self.cells[i].set_symbol(grapheme);
                self.cells[i].set_style(style);
            }
            for k in 1..gw {
                if let Some(i) = self.index_of(cur_x + k, y) {
                    self.cells[i].set_symbol("");
                    self.cells[i].set_style(style);
                }
            }
            cur_x += gw;
        }
        cur_x
    }

    /// Write `s` starting at `(x, y)`, styled, clipped to the buffer edge.
    /// Returns the x column after the last glyph written.
    pub fn set_string(&mut self, x: u16, y: u16, s: &str, style: Style) -> u16 {
        self.set_stringn(x, y, s, self.width, style)
    }

    /// Write a single `char` at `(x, y)`, styled. Returns the x column after
    /// the glyph.
    pub fn set_char(&mut self, x: u16, y: u16, ch: char, style: Style) -> u16 {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w == 0 {
            return x;
        }
        if let Some(i) = self.index_of(x, y) {
            self.cells[i].set_char(ch);
            self.cells[i].set_style(style);
        }
        #[allow(clippy::cast_possible_truncation)]
        let w = w as u16;
        for k in 1..w {
            if let Some(i) = self.index_of(x + k, y) {
                self.cells[i].set_symbol("");
                self.cells[i].set_style(style);
            }
        }
        x.saturating_add(w)
    }

    /// Fill `width` cells starting at `(x, y)` with spaces carrying `style`.
    /// Used for highlight bars and padded backgrounds.
    pub fn blank(&mut self, x: u16, y: u16, width: u16, style: Style) {
        let end = x.saturating_add(width).min(self.width);
        let mut cx = x;
        while cx < end {
            if let Some(i) = self.index_of(cx, y) {
                self.cells[i].set_symbol(" ");
                self.cells[i].set_style(style);
            }
            cx += 1;
        }
    }

    /// Fill `width` cells starting at `(x, y)` with a repeated (width-1)
    /// character `ch` (box-drawing lines, rules, gutters).
    pub fn hline(&mut self, x: u16, y: u16, width: u16, ch: char, style: Style) {
        let end = x.saturating_add(width).min(self.width);
        let mut cx = x;
        while cx < end {
            if let Some(i) = self.index_of(cx, y) {
                self.cells[i].set_char(ch);
                self.cells[i].set_style(style);
            }
            cx += 1;
        }
    }

    /// Compute the cells that changed from `self` (the front buffer, i.e.
    /// what is on screen) to `next` (the freshly-drawn back buffer). Returns
    /// `(x, y, &Cell)` tuples borrowing from `next`, in row-major order.
    ///
    /// The algorithm is width-aware: after a changed wide glyph, the following
    /// continuation cell is skipped, and an invalidation window is propagated
    /// so overlapping wide-glyph edits repaint cleanly. When the two buffers
    /// differ in size, `next` is returned in full (a forced repaint).
    #[must_use]
    pub fn diff<'a>(&self, next: &'a Buffer) -> Vec<(u16, u16, &'a Cell)> {
        if self.width != next.width || self.height != next.height {
            let mut out = Vec::with_capacity(next.cells.len());
            for (i, cell) in next.cells.iter().enumerate() {
                let (x, y) = next.pos_of(i);
                out.push((x, y, cell));
            }
            return out;
        }

        let mut updates = Vec::new();
        let mut invalidated: usize = 0;
        let mut to_skip: usize = 0;
        for (i, (current, previous)) in next.cells.iter().zip(self.cells.iter()).enumerate() {
            if (current != previous || invalidated > 0) && to_skip == 0 {
                let (x, y) = next.pos_of(i);
                updates.push((x, y, current));
            }
            let current_width = UnicodeWidthStr::width(current.symbol());
            to_skip = current_width.saturating_sub(1);
            let affected = current_width.max(UnicodeWidthStr::width(previous.symbol()));
            invalidated = affected.max(invalidated).saturating_sub(1);
        }
        updates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::Modifiers;
    use crossterm::style::Color;

    fn row_text(buf: &Buffer, y: u16) -> String {
        (0..buf.width())
            .filter_map(|x| buf.get(x, y))
            .map(Cell::symbol)
            .collect::<String>()
    }

    #[test]
    fn empty_is_all_blank() {
        let buf = Buffer::empty(4, 2);
        assert_eq!(buf.width(), 4);
        assert_eq!(buf.height(), 2);
        assert_eq!(row_text(&buf, 0), "    ");
        assert_eq!(row_text(&buf, 1), "    ");
    }

    #[test]
    fn set_string_writes_and_returns_end() {
        let mut buf = Buffer::empty(10, 1);
        let end = buf.set_string(0, 0, "hi", Style::default());
        assert_eq!(end, 2);
        assert_eq!(row_text(&buf, 0), "hi        ");
    }

    #[test]
    fn set_stringn_clips_to_budget() {
        let mut buf = Buffer::empty(10, 1);
        let end = buf.set_stringn(0, 0, "hello world", 5, Style::default());
        assert_eq!(end, 5);
        assert_eq!(row_text(&buf, 0), "hello     ");
    }

    #[test]
    fn set_string_clips_to_buffer_edge() {
        let mut buf = Buffer::empty(3, 1);
        let end = buf.set_string(0, 0, "abcdef", Style::default());
        assert_eq!(end, 3);
        assert_eq!(row_text(&buf, 0), "abc");
    }

    #[test]
    fn wide_glyph_takes_two_cells_with_continuation() {
        let mut buf = Buffer::empty(4, 1);
        let end = buf.set_string(0, 0, "日本", Style::default());
        assert_eq!(end, 4);
        assert_eq!(buf.get(0, 0).unwrap().symbol(), "日");
        assert_eq!(buf.get(1, 0).unwrap().symbol(), "");
        assert_eq!(buf.get(2, 0).unwrap().symbol(), "本");
        assert_eq!(buf.get(3, 0).unwrap().symbol(), "");
    }

    #[test]
    fn wide_glyph_not_written_when_it_would_overflow_budget() {
        let mut buf = Buffer::empty(4, 1);
        // width-2 glyph at x=3 would overflow the width-4 buffer.
        let end = buf.set_stringn(3, 0, "日", 2, Style::default());
        assert_eq!(end, 3);
        assert_eq!(buf.get(3, 0).unwrap().symbol(), " ");
    }

    #[test]
    fn style_applied_to_written_cells() {
        let mut buf = Buffer::empty(4, 1);
        let style = Style::default().fg(Color::Red).bold();
        buf.set_string(0, 0, "ab", style);
        assert_eq!(buf.get(0, 0).unwrap().fg, Color::Red);
        assert!(buf.get(0, 0).unwrap().modifiers.contains(Modifiers::BOLD));
        // untouched cells keep the default style
        assert_eq!(buf.get(3, 0).unwrap().fg, Color::Reset);
    }

    #[test]
    fn blank_and_hline() {
        let mut buf = Buffer::empty(6, 1);
        let style = Style::default().bg(Color::Blue);
        buf.blank(0, 0, 3, style);
        buf.hline(3, 0, 3, '─', style);
        assert_eq!(row_text(&buf, 0), "   ───");
        assert_eq!(buf.get(0, 0).unwrap().bg, Color::Blue);
        assert_eq!(buf.get(5, 0).unwrap().bg, Color::Blue);
    }

    #[test]
    fn diff_reports_only_changed_cells() {
        let front = Buffer::empty(5, 1);
        let mut back = Buffer::empty(5, 1);
        back.set_string(2, 0, "X", Style::default());
        let updates = front.diff(&back);
        assert_eq!(updates.len(), 1);
        assert_eq!((updates[0].0, updates[0].1), (2, 0));
        assert_eq!(updates[0].2.symbol(), "X");
    }

    #[test]
    fn diff_empty_when_identical() {
        let front = Buffer::empty(5, 2);
        let back = front.clone();
        assert!(front.diff(&back).is_empty());
    }

    #[test]
    fn diff_size_mismatch_is_full_repaint() {
        let front = Buffer::empty(2, 1);
        let back = Buffer::empty(3, 1);
        assert_eq!(front.diff(&back).len(), 3);
    }

    #[test]
    fn diff_skips_continuation_after_wide_change() {
        let front = Buffer::empty(4, 1);
        let mut back = Buffer::empty(4, 1);
        back.set_string(0, 0, "日", Style::default());
        let updates = front.diff(&back);
        // Only the wide glyph cell is an update; the continuation is skipped.
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].2.symbol(), "日");
    }

    #[test]
    fn resize_clears() {
        let mut buf = Buffer::empty(2, 1);
        buf.set_string(0, 0, "ab", Style::default());
        buf.resize(3, 2);
        assert_eq!(buf.width(), 3);
        assert_eq!(buf.height(), 2);
        assert_eq!(row_text(&buf, 0), "   ");
    }
}
