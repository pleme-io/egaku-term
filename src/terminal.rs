//! Drop-safe terminal lifecycle wrapper + panic-safe restore (Quadro P7).
//!
//! A [`Terminal`] restores raw-mode / alt-screen / cursor on `Drop` — even on
//! an unwinding panic, because Drop runs during unwind. On top of that, a
//! process-wide **panic hook** (installed by [`install_panic_hook`], called
//! automatically by [`Terminal::enter`]) restores the terminal *before* the
//! default hook prints its message, so a panic in a draw/handle callback never
//! wedges the operator's terminal or leaks DA/OSC replies into the shell.

use std::io::{IsTerminal, Stdout, Write, stdout};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crossterm::{
    ExecutableCommand, QueueableCommand, cursor,
    event::PopKeyboardEnhancementFlags,
    terminal::{self, ClearType, LeaveAlternateScreen},
};

use crate::error::Result;

/// Test-observable count of how many times [`restore_terminal`] has run. Lets
/// the panic-injection test assert that a panic actually triggered a restore
/// (the terminal restore itself is a global syscall we can't observe directly
/// in a headless test).
pub(crate) static RESTORE_CALLS: AtomicUsize = AtomicUsize::new(0);

/// Whether [`Terminal::enter`] has already installed the process-wide panic
/// hook. Guards against stacking multiple restore layers when several
/// terminals are entered in sequence.
static PANIC_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Write the terminal-restore escape sequences to `out`: pop any pushed
/// keyboard-enhancement flags, leave the alternate screen, show the cursor.
/// Split out from the global [`restore_terminal`] so it is unit-testable
/// against an in-memory writer.
///
/// # Errors
///
/// Propagates any `io::Error` from writing to `out`.
pub(crate) fn restore_sequences<W: Write>(out: &mut W) -> std::io::Result<()> {
    out.queue(PopKeyboardEnhancementFlags)?;
    out.queue(LeaveAlternateScreen)?;
    out.queue(cursor::Show)?;
    out.flush()?;
    Ok(())
}

/// Best-effort restore of the shared terminal: leave the alt screen, show the
/// cursor, pop keyboard flags, and disable raw mode. Every step swallows its
/// own error — a partially-restored terminal is worse than a fully-restored
/// one. Safe to call more than once (each step is idempotent). No-ops the
/// escape writes when stdout is not a TTY (e.g. under `cargo test`).
pub(crate) fn restore_terminal() {
    RESTORE_CALLS.fetch_add(1, Ordering::SeqCst);
    if stdout().is_terminal() {
        let mut out = stdout();
        let _ = restore_sequences(&mut out);
        let _ = terminal::disable_raw_mode();
    }
}

/// Install a process-wide panic hook that restores the terminal before the
/// previously-installed hook runs (so the default hook's message prints on a
/// clean terminal). Chains — the previous hook is still invoked.
///
/// Called automatically (once) by [`Terminal::enter`]; exposed so an app that
/// drives [`Terminal`] pieces directly can install it explicitly.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
}

fn install_panic_hook_once() {
    if PANIC_HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    install_panic_hook();
}

/// Owns the terminal for the duration of an interactive session.
///
/// Constructing a `Terminal` enables raw mode, switches to the alternate
/// screen, hides the cursor, and installs the panic hook. Dropping it reverses
/// every lifecycle step in the opposite order — including on panic — so a
/// wedged terminal is impossible as long as a `Terminal` exists.
#[allow(clippy::struct_excessive_bools)] // four independent lifecycle flags, each rolled back on Drop.
pub struct Terminal {
    out: Stdout,
    raw_enabled: bool,
    alt_screen: bool,
    cursor_hidden: bool,
    sync_output: bool,
}

impl Terminal {
    /// Initialise a terminal session: raw mode + alternate screen + hide
    /// cursor, and install the panic-restore hook. Each lifecycle step is
    /// recorded so [`Drop`] rolls back exactly what this constructor changed.
    ///
    /// Synchronized output (DEC 2026) is enabled by default: the wrapping
    /// sequence is ignored by terminals that don't advertise it, so leaving it
    /// on is safe everywhere.
    pub fn enter() -> Result<Self> {
        install_panic_hook_once();
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
            sync_output: true,
        })
    }

    /// Borrow stdout WITHOUT taking ownership of terminal lifecycle.
    ///
    /// Use this when something else (a `kura_ghostty::TerminalRestoreGuard`,
    /// a hand-rolled `enable_raw_mode`+`EnterAlternateScreen` pair, an
    /// embedded TUI inside a larger app, etc.) already owns raw-mode +
    /// alt-screen lifecycle. The returned `Terminal` is a thin wrapper —
    /// drop is a no-op (it neither disables raw mode nor leaves the
    /// alternate screen). Does not install the panic hook (the host owns
    /// lifecycle and its own restore).
    #[must_use]
    pub fn borrow_stdout() -> Self {
        Self {
            out: stdout(),
            raw_enabled: false,
            alt_screen: false,
            cursor_hidden: false,
            sync_output: true,
        }
    }

    /// Whether frames are wrapped in DEC-2026 synchronized output.
    #[must_use]
    pub fn sync_output(&self) -> bool {
        self.sync_output
    }

    /// Enable or disable DEC-2026 synchronized-output wrapping.
    pub fn set_sync_output(&mut self, enabled: bool) {
        self.sync_output = enabled;
    }

    /// Borrow the underlying `Stdout` mutably. The runtime uses this to flush
    /// the diff; drawers write into a [`Buffer`](crate::buffer::Buffer)
    /// instead and never touch stdout.
    pub fn out(&mut self) -> &mut Stdout {
        &mut self.out
    }

    /// Clear the screen and move the cursor to `(0, 0)`. The diff runtime
    /// calls this once at startup and after a resize so the on-screen state
    /// matches a blank front buffer.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Mutex;

    // Serialize the tests that mutate the process-global panic hook.
    static HOOK_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn restore_sequences_emits_leave_alt_and_show_cursor() {
        let mut out: Vec<u8> = Vec::new();
        restore_sequences(&mut out).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(
            s.contains("\x1b[?1049l"),
            "missing LeaveAlternateScreen in {s:?}"
        );
        assert!(s.contains("\x1b[?25h"), "missing Show cursor in {s:?}");
    }

    #[test]
    fn restore_terminal_increments_counter() {
        let before = RESTORE_CALLS.load(Ordering::SeqCst);
        restore_terminal();
        assert!(RESTORE_CALLS.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn panic_hook_restores_terminal_before_default() {
        let _guard = HOOK_TEST_LOCK.lock().unwrap();
        let saved = std::panic::take_hook();
        // Silent baseline so the injected panic doesn't spam test output.
        std::panic::set_hook(Box::new(|_| {}));
        // Chains our restore in front of the silent baseline.
        install_panic_hook();

        let before = RESTORE_CALLS.load(Ordering::SeqCst);
        let result = catch_unwind(AssertUnwindSafe(|| {
            panic!("injected panic — terminal must be restored");
        }));

        // Restore the original hook before asserting so a failure here doesn't
        // leave the test binary with our hook installed.
        std::panic::set_hook(saved);

        assert!(
            result.is_err(),
            "the injected panic should have been caught"
        );
        assert!(
            RESTORE_CALLS.load(Ordering::SeqCst) > before,
            "the panic hook must have run the terminal restore"
        );
    }
}
