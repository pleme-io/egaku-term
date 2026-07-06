//! `App` trait + `run()` runtime.
//!
//! The runtime owns the terminal lifecycle and event loop so apps don't
//! have to. Implement [`App`] on a state struct, then call [`run`].

use crossterm::event::{self, Event};
use egaku::KeyMap;

use crate::buffer::Buffer;
use crate::error::Result;
use crate::event::from_crossterm;
use crate::render::render_diff;
use crate::terminal::Terminal;

/// Drives a terminal application via egaku state machines.
///
/// Apps are built around three callbacks: [`App::handle`] turns a resolved
/// keymap action into a state transition, [`App::draw`] paints a frame,
/// [`App::should_quit`] tells the runtime when to tear down.
///
/// Typical implementations also keep a [`KeyMap`] and a `done: bool` field;
/// the runtime never inspects either directly except via the trait methods.
pub trait App {
    /// The action enum the app's [`KeyMap`] resolves keys to.
    type Action;

    /// Borrow the keymap. Called once per event; small and cheap.
    fn keymap(&self) -> &KeyMap<Self::Action>;

    /// Apply a resolved action to the app's state.
    fn handle(&mut self, action: &Self::Action);

    /// Paint the current state into the frame buffer. The buffer is freshly
    /// reset (all blank cells) before this is called; the runtime diffs it
    /// against the previous frame and flushes only the changed cells.
    fn draw(&self, frame: &mut Buffer) -> Result<()>;

    /// Return true to exit the loop. Polled after every event.
    fn should_quit(&self) -> bool;

    /// Optional: fall-through hook for events the keymap didn't resolve
    /// (text input characters, mouse events, resize, etc.). The default
    /// is a no-op.
    fn on_unhandled(&mut self, _event: &Event) {}
}

/// Run an [`App`] to completion.
///
/// Owns the terminal for the duration of the call and restores it on the
/// way out (including on panic, via [`Terminal`]'s `Drop`).
///
/// `Action: Clone` is required so the runtime can detach the resolved
/// action from the keymap borrow before invoking [`App::handle`]. Most
/// app actions are tiny enums that derive `Copy + Clone`, so this is free.
pub fn run<A>(app: &mut A) -> Result<()>
where
    A: App,
    A::Action: Clone,
{
    let mut term = Terminal::enter()?;
    let (mut cols, mut rows) = term.size()?;
    // Two buffers: `prev` mirrors what is on screen, `back` is drawn into each
    // frame. After clearing the terminal the screen matches a blank `prev`, so
    // the first diff paints the whole first frame.
    let mut prev = Buffer::empty(cols, rows);
    let mut back = Buffer::empty(cols, rows);
    term.clear()?;
    term.flush()?;

    while !app.should_quit() {
        back.reset();
        app.draw(&mut back)?;
        let sync = term.sync_output();
        render_diff(term.out(), &prev, &back, sync)?;
        std::mem::swap(&mut prev, &mut back);

        let evt = event::read()?;
        if let Event::Resize(w, h) = evt {
            cols = w;
            rows = h;
            // Resize both buffers (which clears them) and clear the screen,
            // so the next frame is a clean full repaint.
            prev.resize(cols, rows);
            back.resize(cols, rows);
            term.clear()?;
            term.flush()?;
            app.on_unhandled(&evt);
        } else {
            // Have to clone the action out: the borrow of `app` via `keymap()`
            // would otherwise overlap with `handle(&mut self)`. Most app
            // actions are small (Copy/Clone enums), so this is free in practice.
            if let Some(combo) = from_crossterm(&evt)
                && let Some(action) = app.keymap().lookup(&combo)
            {
                let action = clone_via_ref(action);
                app.handle(&action);
                continue;
            }
            app.on_unhandled(&evt);
        }
    }

    Ok(())
}

/// Workaround for borrow conflict: clone an `&A` into an owned `A` via the
/// [`Clone`] trait. The runtime requires `Action: Clone`, which is the only
/// constraint not stated in the [`App`] trait directly (it shows up here so
/// users can have `App` impls that don't use the runtime).
fn clone_via_ref<T: Clone>(t: &T) -> T {
    t.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use egaku::KeyCombo;

    // We can't run a real terminal in tests, but we can exercise the
    // trait wiring + keymap dispatch logic by calling the methods directly.

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Act {
        Bump,
        Quit,
    }

    struct Counter {
        count: u32,
        keys: KeyMap<Act>,
        done: bool,
    }

    impl Counter {
        fn new() -> Self {
            let mut keys = KeyMap::new();
            keys.bind(KeyCombo::key("space"), Act::Bump);
            keys.bind(KeyCombo::key("q"), Act::Quit);
            Self {
                count: 0,
                keys,
                done: false,
            }
        }
    }

    impl App for Counter {
        type Action = Act;
        fn keymap(&self) -> &KeyMap<Act> {
            &self.keys
        }
        fn handle(&mut self, a: &Act) {
            match a {
                Act::Bump => self.count += 1,
                Act::Quit => self.done = true,
            }
        }
        fn draw(&self, _frame: &mut Buffer) -> Result<()> {
            Ok(())
        }
        fn should_quit(&self) -> bool {
            self.done
        }
    }

    #[test]
    fn keymap_lookup_through_app() {
        let mut c = Counter::new();
        let bump = c.keymap().lookup(&KeyCombo::key("space")).copied().unwrap();
        c.handle(&bump);
        assert_eq!(c.count, 1);
        assert!(!c.should_quit());
        c.handle(&Act::Quit);
        assert!(c.should_quit());
    }

    #[test]
    fn unhandled_default_is_noop() {
        let mut c = Counter::new();
        c.on_unhandled(&Event::Resize(80, 24));
        assert_eq!(c.count, 0);
    }
}
