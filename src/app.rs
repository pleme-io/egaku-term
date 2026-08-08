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

    /// Optional: dispatch through **typed** [`awase::Hotkey`] chords instead
    /// of the string [`KeyCombo`] path.
    ///
    /// Returning `Some` opts this app into the typed runtime; the default
    /// `None` keeps [`Self::keymap`] as the dispatch source, so every
    /// existing implementation is untouched.
    ///
    /// # Why an app would want this
    ///
    /// A `KeyCombo` is a key *name* plus modifier *names*. An app that
    /// authors its chords as typed values must therefore project them into
    /// this crate's spelling before anything can match, and that projection
    /// is where chords go to die: banken carried 199 lines of it, refusing
    /// `space` outright because the two vocabularies had no safe translation
    /// for it. On this path the authored chord and the delivered chord are
    /// the same type, so there is nothing to project and nothing to drift.
    ///
    /// # Modes
    ///
    /// Return a different [`awase::KeyMode`] as the app's state changes and
    /// the active bindings change with it — that is how a search prompt stops
    /// `j` from moving the cursor. Unclaimed printable characters arrive at
    /// [`Self::on_text`].
    fn hotkey_map(&self) -> Option<&awase::KeyMode<Self::Action>> {
        None
    }

    /// Optional: a printable character that no binding claimed.
    ///
    /// **Only fires on the typed path** (when [`Self::hotkey_map`] returns
    /// `Some`). Apps on the string path keep receiving character events
    /// through [`Self::on_unhandled`] exactly as before — routing them here
    /// too would change behaviour under existing implementations, which this
    /// addition must not do.
    ///
    /// This is the hook that makes a text field possible without the keymap
    /// swallowing its letters: a search mode binds only Escape/Return/
    /// Backspace, and every other key misses and lands here.
    fn on_text(&mut self, _c: char) {}
}

/// Resolve one event through an app's typed keymap.
///
/// Returns the action to apply, or `None` when nothing claimed the event —
/// in which case the caller routes to `on_text`/`on_unhandled`. Split out so
/// the sync and async runtimes cannot drift on dispatch semantics.
pub(crate) fn typed_dispatch<A>(mode: &awase::KeyMode<A>, evt: &Event) -> Option<A>
where
    A: Clone,
{
    let hk = crate::event::hotkey_from_crossterm(evt)?;
    let binding = mode.find_binding(&hk, &awase::MatchContext::default())?;
    Some(binding.action.clone())
}

/// The printable character an unclaimed key event carries, if any.
///
/// Deliberately excludes ctrl/alt/super-modified keys: those are chords an
/// app simply has not bound, not text the operator meant to type. Shift IS
/// allowed through, because a capital letter is text.
pub(crate) fn text_char(evt: &Event) -> Option<char> {
    use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
    let Event::Key(k) = evt else { return None };
    if k.kind != KeyEventKind::Press {
        return None;
    }
    if k.modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return None;
    }
    match k.code {
        KeyCode::Char(c) => Some(c),
        _ => None,
    }
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
        } else if let Some(action) = app.hotkey_map().and_then(|m| typed_dispatch(m, &evt)) {
            // Typed path. `typed_dispatch` clones the action out, which ends
            // the `hotkey_map()` borrow before `handle(&mut self)`.
            app.handle(&action);
        } else if app.hotkey_map().is_some() {
            // Typed path, nothing claimed it: a printable character is text,
            // anything else falls through as before.
            if let Some(c) = text_char(&evt) {
                app.on_text(c);
            } else {
                app.on_unhandled(&evt);
            }
        } else {
            // String path — unchanged.
            //
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

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

    // ── the typed dispatch path ─────────────────────────────────────────

    fn press(code: crossterm::event::KeyCode) -> Event {
        Event::Key(crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::NONE,
        ))
    }

    fn typed_mode() -> awase::KeyMode<Act> {
        let mut m: awase::KeyMode<Act> = awase::KeyMode::typed("default", false);
        m.try_bind(awase::Binding::new(
            awase::Hotkey::new(awase::Modifiers::NONE, awase::Key::Space),
            Act::Bump,
        ))
        .expect("free");
        m.try_bind(awase::Binding::new(
            awase::Hotkey::new(awase::Modifiers::NONE, awase::Key::Q),
            Act::Quit,
        ))
        .expect("free");
        m
    }

    #[test]
    fn typed_dispatch_resolves_a_real_keypress() {
        let m = typed_mode();
        assert_eq!(
            typed_dispatch(&m, &press(crossterm::event::KeyCode::Char('q'))),
            Some(Act::Quit)
        );
    }

    /// The fix this path exists for. The string fixture above binds
    /// `KeyCombo::key("space")`, which egaku-term can NEVER deliver — it
    /// names the spacebar `" "`. That binding is dead, and the test around it
    /// passes only because it looks the same wrong string back up.
    ///
    /// The spacebar fires on BOTH paths.
    ///
    /// This test used to be named `..._and_is_dead_on_the_string_path` and
    /// asserted the opposite of its second half: a real press delivered the
    /// key name `" "`, so the `space` binding anyone would actually write was
    /// unreachable, and that brokenness was pinned here rather than fixed.
    ///
    /// It is fixed upstream (egaku `c502920`): `KeyCombo` canonicalises an
    /// all-whitespace key to `space` before parsing, so the two spellings are
    /// one value. The characterisation is inverted rather than deleted —
    /// what was pinned as broken is now pinned as working.
    #[test]
    fn the_spacebar_dispatches_on_both_the_typed_and_string_paths() {
        let m = typed_mode();
        assert_eq!(
            typed_dispatch(&m, &press(crossterm::event::KeyCode::Char(' '))),
            Some(Act::Bump),
            "typed: the spacebar fires"
        );

        let c = Counter::new();
        let delivered = crate::event::from_crossterm(&press(crossterm::event::KeyCode::Char(' ')))
            .expect("the string path delivers something");
        assert_eq!(
            delivered.key, "space",
            "a real press is delivered under the name a binding can be written with"
        );
        assert!(
            c.keymap().lookup(&delivered).is_some(),
            "the `space` binding in the fixture is now REACHABLE from a real press"
        );
    }

    #[test]
    fn typed_dispatch_returns_none_for_an_unbound_key() {
        let m = typed_mode();
        assert_eq!(
            typed_dispatch(&m, &press(crossterm::event::KeyCode::Char('z'))),
            None
        );
    }

    #[test]
    fn an_unclaimed_printable_character_is_text() {
        // What makes a search prompt possible: the key missed the map, so it
        // is text rather than a lost event.
        assert_eq!(
            text_char(&press(crossterm::event::KeyCode::Char('z'))),
            Some('z')
        );
        assert_eq!(
            text_char(&press(crossterm::event::KeyCode::Char('Z'))),
            Some('Z'),
            "shift is text, not a chord"
        );
    }

    #[test]
    fn a_modified_key_is_not_text() {
        // ctrl+z is a chord the app has not bound, not something typed.
        let e = Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('z'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(text_char(&e), None);
        // …and neither is a non-character key.
        assert_eq!(text_char(&press(crossterm::event::KeyCode::Up)), None);
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
