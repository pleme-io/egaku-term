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
//! - [`event::from_crossterm`] — convert any `crossterm::event::Event` into an
//!   [`egaku::KeyCombo`]. Pairs with [`egaku::KeyMap`] for action dispatch.
//! - [`draw`] — drawers for every widget egaku ships (`ListView`, `TextInput`,
//!   `TabBar`, `Modal`, `SplitPane`, `ScrollView`). Each one takes a
//!   [`Rect`](egaku::Rect), the widget, the theme, and a focus flag.
//! - [`App`] / [`run`] — a generic event-loop runtime. Implement `App::update`
//!   + `App::draw` and `run(&mut app)` does the rest.
//! - [`keymap!`] / [`key!`] — declarative macros that make hand-authoring
//!   keybindings ergonomic.
//!
//! ## Minimal example
//!
//! ```no_run
//! use egaku::ListView;
//! use egaku_term::{App, Terminal, Result, key, keymap, draw};
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
//!     fn draw(&self, term: &mut Terminal) -> Result<()> {
//!         draw::list(term, egaku::Rect::new(0, 0, 40, 10), &self.list, true)
//!     }
//!     fn should_quit(&self) -> bool { self.done }
//! }
//! ```

pub mod app;
pub mod draw;
pub mod error;
pub mod event;
pub mod keymap;
pub mod terminal;
pub mod theme;

pub use app::{App, run};
pub use error::{Error, Result};
pub use event::from_crossterm;
pub use terminal::Terminal;

// Re-export crossterm so downstream crates don't have to track its version
// independently. They get the exact crossterm we render against.
pub use crossterm;

/// Re-exports used by the `key!` and `keymap!` macros so callers don't need
/// to import egaku types directly.
#[doc(hidden)]
pub mod __re {
    pub use egaku::{KeyCombo, KeyMap};
}
