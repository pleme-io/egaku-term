//! The headless backend — golden-frame capture over a [`Buffer`].
//!
//! [`TestBackend`] renders a frame into a [`Buffer`] with no terminal
//! attached, then exposes it for structured assertions. This is the Quadro T11
//! capture surface: a test draws widgets into the backend and asserts the
//! resulting cells / lines, instead of taking a real TTY.
//!
//! Assert against structured accessors — [`TestBackend::to_lines`] for text,
//! [`TestBackend::cell`] for per-cell style — **not** `.contains()` on a
//! formatted blob (★★ TOKEN-STABILITY).
//!
//! ```
//! use egaku_term::{TestBackend, Style};
//! use egaku_term::crossterm::style::Color;
//!
//! let mut backend = TestBackend::new(8, 1);
//! backend.draw(|buf| {
//!     buf.set_string(0, 0, "hi", Style::default().fg(Color::Red));
//! });
//! assert_eq!(backend.to_lines(), vec!["hi".to_string()]);
//! assert_eq!(backend.cell(0, 0).unwrap().fg, Color::Red);
//! ```

use std::fmt;

use crate::buffer::Buffer;
use crate::cell::Cell;

/// A headless renderer that draws into an in-memory [`Buffer`].
#[derive(Debug, Clone)]
pub struct TestBackend {
    buffer: Buffer,
}

impl TestBackend {
    /// A blank backend sized `width × height` cells.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            buffer: Buffer::empty(width, height),
        }
    }

    /// Clear the buffer and run `f` to draw one frame into it. This mirrors
    /// what the runtime does each frame: reset the back buffer, then draw.
    pub fn draw<F: FnOnce(&mut Buffer)>(&mut self, f: F) {
        self.buffer.reset();
        f(&mut self.buffer);
    }

    /// Borrow the captured buffer.
    #[must_use]
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Mutably borrow the captured buffer (draw into it directly).
    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffer
    }

    /// The cell at `(x, y)`, if in bounds — for per-cell style assertions.
    #[must_use]
    pub fn cell(&self, x: u16, y: u16) -> Option<&Cell> {
        self.buffer.get(x, y)
    }

    /// Resize the backend, clearing it.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.buffer.resize(width, height);
    }

    /// Backend width in cells.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.buffer.width()
    }

    /// Backend height in cells.
    #[must_use]
    pub fn height(&self) -> u16 {
        self.buffer.height()
    }

    /// The captured frame as one `String` per row, with trailing blank cells
    /// trimmed. Continuation cells (after a wide glyph) contribute their empty
    /// symbol, so a row reads back as the visible text.
    #[must_use]
    pub fn to_lines(&self) -> Vec<String> {
        (0..self.buffer.height())
            .map(|y| {
                let line: String = (0..self.buffer.width())
                    .filter_map(|x| self.buffer.get(x, y))
                    .map(Cell::symbol)
                    .collect();
                line.trim_end().to_string()
            })
            .collect()
    }
}

impl fmt::Display for TestBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines = self.to_lines();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{line}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::Style;
    use crossterm::style::Color;

    #[test]
    fn draw_and_read_lines() {
        let mut backend = TestBackend::new(10, 2);
        backend.draw(|buf| {
            buf.set_string(0, 0, "top", Style::default());
            buf.set_string(0, 1, "bottom", Style::default());
        });
        assert_eq!(
            backend.to_lines(),
            vec!["top".to_string(), "bottom".to_string()]
        );
    }

    #[test]
    fn draw_resets_between_frames() {
        let mut backend = TestBackend::new(6, 1);
        backend.draw(|buf| {
            buf.set_string(0, 0, "aaaaaa", Style::default());
        });
        backend.draw(|buf| {
            buf.set_string(0, 0, "b", Style::default());
        });
        assert_eq!(backend.to_lines(), vec!["b".to_string()]);
    }

    #[test]
    fn cell_exposes_style() {
        let mut backend = TestBackend::new(4, 1);
        backend.draw(|buf| {
            buf.set_string(0, 0, "x", Style::default().fg(Color::Green));
        });
        assert_eq!(backend.cell(0, 0).unwrap().fg, Color::Green);
        assert!(backend.cell(9, 9).is_none());
    }

    #[test]
    fn display_joins_rows() {
        let mut backend = TestBackend::new(4, 2);
        backend.draw(|buf| {
            buf.set_string(0, 0, "ab", Style::default());
            buf.set_string(0, 1, "cd", Style::default());
        });
        assert_eq!(backend.to_string(), "ab\ncd");
    }
}
