//! Egaku-Term — terminal renderer + runtime for [`egaku`] widget state machines.
//!
//! `egaku` is a pure-logic widget toolkit: state only, no rendering, no event
//! loop. Every consumer that wants a terminal UI today re-implements the
//! same boilerplate:
//!
//! 1. Enable raw mode + alternate screen + hide cursor on entry.
//! 2. Restore those on exit (and on panic, ideally).
//! 3. Pump `crossterm::event::read()` in a loop.
//! 4. Translate `KeyEvent` into something the app's state machine understands.
//! 5. Hand-style each widget render — selected line reverse video, padding,
//!    column wrapping, etc.
//!
//! `egaku-term` lifts those five steps into one library:
//!
//! - [`Terminal`] — Drop-safe lifecycle wrapper. Constructing one enables raw
//!   mode + alt screen; dropping (or panicking with one alive) restores the
//!   terminal.
//! - [`Buffer`] — the typed back buffer of [`Cell`]s. Drawers write cells into
//!   it; the runtime [`diff`](Buffer::diff)s it against the previous frame and
//!   flushes only the changed cells via [`render::render_diff`] — no
//!   full-repaint, no `format!()` (★★ TYPED EMISSION).
//! - [`event::from_crossterm`] — convert any `crossterm::event::Event` into an
//!   [`egaku::KeyCombo`]. Pairs with [`egaku::KeyMap`] for action dispatch.
//! - [`draw`] — drawers for every widget egaku ships (`ListView`, `TextInput`,
//!   `TabBar`, `Modal`, `SplitPane`, `ScrollView`). Each one takes a
//!   [`Buffer`], a [`Rect`](egaku::Rect), the widget, the theme, and a focus
//!   flag.
//! - [`TestBackend`] — headless [`Buffer`] capture for golden-frame tests.
//! - [`App`] / [`run`] — a generic event-loop runtime. Implement `App::handle`
//!   + `App::draw` and `run(&mut app)` does the rest.
//! - [`keymap!`] / [`key!`] — declarative macros that make hand-authoring
//!   keybindings ergonomic.
//!
//! ## Minimal example
//!
//! ```no_run
//! use egaku::ListView;
//! use egaku_term::{App, Buffer, Result, draw};
//!
//! #[derive(Clone, Copy, PartialEq, Eq)]
//! enum Action { Next, Prev, Quit }
//!
//! struct Wizard {
//!     list: ListView,
//!     keys: egaku::KeyMap<Action>,
//!     done: bool,
//! }
//!
//! impl App for Wizard {
//!     type Action = Action;
//!     fn keymap(&self) -> &egaku::KeyMap<Action> { &self.keys }
//!     fn handle(&mut self, action: &Action) {
//!         match action {
//!             Action::Next => self.list.select_next(),
//!             Action::Prev => self.list.select_prev(),
//!             Action::Quit => self.done = true,
//!         }
//!     }
//!     fn draw(&self, frame: &mut Buffer) -> Result<()> {
//!         draw::list(frame, egaku::Rect::new(0.0, 0.0, 40.0, 10.0), &self.list, true);
//!         Ok(())
//!     }
//!     fn should_quit(&self) -> bool { self.done }
//! }
//! ```

mod backend;
mod buffer;
mod cell;

pub mod app;
pub mod draw;
pub mod error;
pub mod event;
pub mod keymap;
pub mod render;
pub mod terminal;
pub mod theme;

#[cfg(feature = "tokio")]
pub mod app_async;

pub use app::{App, run};
pub use backend::TestBackend;
pub use buffer::Buffer;
pub use cell::{Cell, Modifiers, Style};
pub use error::{Error, Result};
pub use event::from_crossterm;
pub use render::render_diff;
pub use terminal::Terminal;

/// The back buffer a frame is drawn into. Alias of [`Buffer`] — a drawer's
/// canvas for one frame, diffed against the previous frame by the runtime.
pub type Frame = Buffer;

#[cfg(feature = "tokio")]
pub use app_async::{AsyncApp, run_async};

// Re-export crossterm so downstream crates don't have to track its version
// independently. They get the exact crossterm we render against.
pub use crossterm;

/// Re-exports used by the `key!` and `keymap!` macros so callers don't need
/// to import egaku types directly.
#[doc(hidden)]
pub mod __re {
    pub use egaku::{KeyCombo, KeyMap};
}
