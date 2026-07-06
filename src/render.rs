//! The diff renderer — projects changed [`Cell`](crate::cell::Cell)s to
//! crossterm `Command`s.
//!
//! This is the Quadro P6 half of the M0 fix: instead of `Terminal::clear()` +
//! full repaint every frame, the runtime keeps a front buffer (what is on
//! screen) and a back buffer (what was just drawn), computes
//! [`Buffer::diff`](crate::buffer::Buffer::diff), and flushes **only the
//! changed cells** — a `MoveTo` when the run is non-contiguous, a style change
//! only when the style differs from the previous cell, then a `Print` of the
//! grapheme. The whole flush is wrapped in DEC-2026 synchronized output
//! (`BeginSynchronizedUpdate` / `EndSynchronizedUpdate`) so a terminal that
//! advertises it renders the frame atomically (no tearing on a VU meter).
//!
//! Every byte written here goes through a typed crossterm `Command`
//! (`MoveTo`, `SetForegroundColor`, `SetAttribute`, `Print`) — there is no
//! `format!()` and no hand-spelled escape anywhere in the render path
//! (★★ TYPED EMISSION / Quadro P5).

use std::io::Write;

use crossterm::QueueableCommand;
use crossterm::cursor::MoveTo;
use crossterm::style::{
    Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use unicode_width::UnicodeWidthStr;

use crate::buffer::Buffer;
use crate::cell::{Modifiers, Style};

/// Apply a whole [`Style`] to the output. `Attribute::Reset` clears attributes
/// *and* colors, so a `Color::Reset` slot is left as the terminal default
/// (never emitted explicitly — the live palette shows through).
fn apply_style<W: Write>(out: &mut W, style: Style) -> std::io::Result<()> {
    out.queue(SetAttribute(Attribute::Reset))?;
    if style.fg != Color::Reset {
        out.queue(SetForegroundColor(style.fg))?;
    }
    if style.bg != Color::Reset {
        out.queue(SetBackgroundColor(style.bg))?;
    }
    let m = style.modifiers;
    if m.contains(Modifiers::BOLD) {
        out.queue(SetAttribute(Attribute::Bold))?;
    }
    if m.contains(Modifiers::DIM) {
        out.queue(SetAttribute(Attribute::Dim))?;
    }
    if m.contains(Modifiers::ITALIC) {
        out.queue(SetAttribute(Attribute::Italic))?;
    }
    if m.contains(Modifiers::UNDERLINED) {
        out.queue(SetAttribute(Attribute::Underlined))?;
    }
    if m.contains(Modifiers::REVERSED) {
        out.queue(SetAttribute(Attribute::Reverse))?;
    }
    Ok(())
}

/// Flush the diff between `prev` (on screen) and `next` (just drawn) to `out`.
///
/// Writes nothing when the buffers are identical. When `sync` is true the
/// flush is wrapped in DEC-2026 synchronized-output brackets; terminals that
/// don't understand the private mode ignore the sequence, so it is safe to
/// leave on by default.
///
/// # Errors
///
/// Propagates any `io::Error` from writing to `out`.
pub fn render_diff<W: Write>(
    out: &mut W,
    prev: &Buffer,
    next: &Buffer,
    sync: bool,
) -> std::io::Result<()> {
    let updates = prev.diff(next);
    if updates.is_empty() {
        return Ok(());
    }

    if sync {
        out.queue(BeginSynchronizedUpdate)?;
    }

    // `expected` is where the terminal cursor sits after the previous Print;
    // when the next update starts there we skip the MoveTo.
    let mut expected: Option<(u16, u16)> = None;
    let mut last_style: Option<Style> = None;

    for (x, y, cell) in updates {
        if expected != Some((x, y)) {
            out.queue(MoveTo(x, y))?;
        }
        let style = cell.style();
        if last_style != Some(style) {
            apply_style(out, style)?;
            last_style = Some(style);
        }
        let symbol = cell.symbol();
        if symbol.is_empty() {
            // Continuation slot after a wide glyph — nothing to print.
            expected = Some((x, y));
            continue;
        }
        out.queue(Print(symbol))?;
        #[allow(clippy::cast_possible_truncation)]
        let w = UnicodeWidthStr::width(symbol).max(1) as u16;
        expected = Some((x.saturating_add(w), y));
    }

    out.queue(SetAttribute(Attribute::Reset))?;
    out.queue(ResetColor)?;
    if sync {
        out.queue(EndSynchronizedUpdate)?;
    }
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::Style;

    fn render_to_string(prev: &Buffer, next: &Buffer, sync: bool) -> String {
        let mut out: Vec<u8> = Vec::new();
        render_diff(&mut out, prev, next, sync).unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn identical_buffers_emit_nothing() {
        let front = Buffer::empty(4, 1);
        let back = front.clone();
        assert_eq!(render_to_string(&front, &back, true), "");
    }

    #[test]
    fn changed_cell_is_printed() {
        let front = Buffer::empty(4, 1);
        let mut back = Buffer::empty(4, 1);
        back.set_string(1, 0, "Z", Style::default());
        let s = render_to_string(&front, &back, false);
        assert!(s.contains('Z'), "expected the changed glyph in {s:?}");
    }

    #[test]
    fn sync_brackets_present_when_enabled() {
        let front = Buffer::empty(4, 1);
        let mut back = Buffer::empty(4, 1);
        back.set_string(0, 0, "A", Style::default());
        let s = render_to_string(&front, &back, true);
        // DEC-2026 begin/end synchronized update.
        assert!(s.contains("\x1b[?2026h"), "missing BSU in {s:?}");
        assert!(s.contains("\x1b[?2026l"), "missing ESU in {s:?}");
    }

    #[test]
    fn sync_brackets_absent_when_disabled() {
        let front = Buffer::empty(4, 1);
        let mut back = Buffer::empty(4, 1);
        back.set_string(0, 0, "A", Style::default());
        let s = render_to_string(&front, &back, false);
        assert!(!s.contains("\x1b[?2026h"));
    }

    #[test]
    fn contiguous_run_prints_glyphs_without_interleaved_moves() {
        let front = Buffer::empty(6, 1);
        let mut back = Buffer::empty(6, 1);
        back.set_string(0, 0, "abc", Style::default());
        let s = render_to_string(&front, &back, false);
        // A per-char MoveTo would split the run; a single contiguous run keeps
        // the three glyphs adjacent in the byte stream.
        assert!(s.contains("abc"), "expected a contiguous run in {s:?}");
    }

    #[test]
    fn styled_cell_emits_color() {
        let front = Buffer::empty(2, 1);
        let mut back = Buffer::empty(2, 1);
        back.set_string(0, 0, "R", Style::default().fg(Color::Red));
        let s = render_to_string(&front, &back, false);
        // crossterm 0.28 encodes `Color::Red` (the bright red at palette index
        // 9) as the 256-color SGR `38;5;9`.
        assert!(s.contains("38;5;9"), "expected red SGR in {s:?}");
    }
}
