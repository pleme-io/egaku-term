//! Drop-safe terminal lifecycle wrapper.

use crossterm::{
    cursor,
    terminal::{self, ClearType},
    ExecutableCommand, QueueableCommand,
};
use std::io::{Stdout, Write, stdout};

use crate::error::Result;

/// Owns the terminal for the duration of an interactive session.
///
/// Constructing a `Terminal` enables raw mode, switches to the alternate
/// screen, and hides the cursor. Dropping it reverses every step in the
/// opposite order — including on panic, so a wedged terminal is impossible
/// as long as a `Terminal` exists.
///
/// `Terminal` derefs as `Stdout` for direct crossterm usage, but the
/// drawers in [`crate::draw`] take `&mut Terminal` so apps rarely need the
/// raw handle.
pub struct Terminal {
    out: Stdout,
    raw_enabled: bool,
    alt_screen: bool,
    cursor_hidden: bool,
}

impl Terminal {
    /// Initialise a terminal session: raw mode + alternate screen + hide
    /// cursor. Each step is recorded so [`Drop`] can roll back exactly the
    /// state this constructor changed (and nothing else).
    pub fn enter() -> Result<Self> {
        let mut out = stdout();

        terminal::enable_raw_mode()?;
        let raw_enabled = true;

        // If the next step fails, we still want raw mode disabled on drop —
        // hence why we set `raw_enabled = true` before this point.
        out.execute(terminal::EnterAlternateScreen)?;
        let alt_screen = true;

        out.execute(cursor::Hide)?;
        let cursor_hidden = true;

        Ok(Self {
            out,
            raw_enabled,
            alt_screen,
            cursor_hidden,
        })
    }

    /// Borrow stdout WITHOUT taking ownership of terminal lifecycle.
    ///
    /// Use this when something else (a `kura_ghostty::TerminalRestoreGuard`,
    /// a hand-rolled `enable_raw_mode`+`EnterAlternateScreen` pair, an
    /// embedded TUI inside a larger app, etc.) already owns raw-mode +
    /// alt-screen lifecycle. The returned `Terminal` is a thin wrapper —
    /// drop is a no-op (it neither disables raw mode nor leaves the
    /// alternate screen).
    ///
    /// Call this when you need [`crate::draw`] drawers but the caller is
    /// the lifecycle authority. If you want egaku-term to own lifecycle,
    /// use [`Terminal::enter`] instead.
    pub fn borrow_stdout() -> Self {
        Self {
            out: stdout(),
            raw_enabled: false,
            alt_screen: false,
            cursor_hidden: false,
        }
    }

    /// Borrow the underlying `Stdout` mutably. Drawers in [`crate::draw`]
    /// use this to queue crossterm commands.
    pub fn out(&mut self) -> &mut Stdout {
        &mut self.out
    }

    /// Clear the screen and move cursor to (0,0). Drawers usually call this
    /// at the start of a frame.
    pub fn clear(&mut self) -> Result<()> {
        self.out
            .queue(terminal::Clear(ClearType::All))?
            .queue(cursor::MoveTo(0, 0))?;
        Ok(())
    }

    /// Flush queued commands to the screen.
    pub fn flush(&mut self) -> Result<()> {
        self.out.flush()?;
        Ok(())
    }

    /// Returns the current terminal size as `(cols, rows)`.
    pub fn size(&self) -> Result<(u16, u16)> {
        Ok(terminal::size()?)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Reverse order of `enter()`. Each step swallows its own error
        // because Drop can't return one — a partially-restored terminal is
        // worse than a fully-restored one.
        if self.cursor_hidden {
            let _ = self.out.execute(cursor::Show);
        }
        if self.alt_screen {
            let _ = self.out.execute(terminal::LeaveAlternateScreen);
        }
        if self.raw_enabled {
            let _ = terminal::disable_raw_mode();
        }
    }
}
