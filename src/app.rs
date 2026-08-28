//! `App` trait + `run()` runtime.
//!
//! The runtime owns the terminal lifecycle and event loop so apps don't
//! have to. Implement [`App`] on a state struct, then call [`run`].

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use egaku::KeyMap;

use crate::buffer::Buffer;
use crate::error::Result;
use crate::event::from_crossterm;
use crate::render::render_diff;
use crate::terminal::Terminal;

/// How long the loop waits for a terminal event when an [`EventPump`] is
/// attached but the app declared no `tick_interval`.
///
/// ~60 Hz. A driven app wants an injected keystroke acted on promptly, and a
/// poll this cheap is invisible next to the redraw it precedes. An app that
/// wants a different tradeoff says so with `tick_interval`, which wins.
const PUMP_POLL: std::time::Duration = std::time::Duration::from_millis(16);

/// A handle another thread can use to feed events into a running [`App`].
///
/// ── ★ WHAT THIS IS FOR ───────────────────────────────────────────────────
/// A TUI's input comes from a terminal, and a terminal is the one thing an
/// agent, a test harness, or a replay tool does not have. Every such consumer
/// otherwise grows its own side channel into the app's state — and then the
/// driven path and the human path are two different programs that happen to
/// share a name. Bugs live in the gap, and they are invisible exactly when
/// you are driving the thing to look for them.
///
/// So a pump does not reach into app state at all. It queues real [`Event`]s,
/// and the run loop takes them **before** it polls the terminal and hands
/// them to the same dispatch chain a keypress goes through — the typed
/// hotkey map, then the string keymap, then `on_text`, then `on_unhandled`.
/// There is one copy of that chain and injected events cannot miss it.
///
/// ── ★ BOUNDED, AND IT SAYS SO ────────────────────────────────────────────
/// The queue has a cap and `inject` returns whether the event was taken. A
/// silently-dropped keystroke is the worst failure a driven TUI can have: the
/// producer believes it typed a password and the app received half of one,
/// and nothing anywhere says so. `dropped()` counts refusals so a consumer
/// can assert on it rather than hope.
#[derive(Clone, Debug)]
pub struct EventPump {
    inner: std::sync::Arc<PumpInner>,
}

#[derive(Debug)]
struct PumpInner {
    queue: std::sync::Mutex<std::collections::VecDeque<Event>>,
    dropped: std::sync::atomic::AtomicU64,
    cap: usize,
}

impl Default for EventPump {
    fn default() -> Self {
        Self::new()
    }
}

impl EventPump {
    /// A pump holding at most 1024 pending events.
    ///
    /// Generous for typing — a password is tens of events — and small enough
    /// that a runaway producer is refused rather than growing until the
    /// machine notices.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(1024)
    }

    #[must_use]
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            inner: std::sync::Arc::new(PumpInner {
                queue: std::sync::Mutex::new(std::collections::VecDeque::new()),
                dropped: std::sync::atomic::AtomicU64::new(0),
                cap: cap.max(1),
            }),
        }
    }

    /// Queue one event. Returns `false` when the queue is full and the event
    /// was NOT taken — never silently.
    ///
    /// ★ Refuses the NEWEST rather than evicting the oldest. A half-delivered
    /// sequence that is missing its tail can be retried; one missing an event
    /// from its middle has been reordered, and for input that is a different
    /// and worse failure.
    pub fn inject(&self, event: Event) -> bool {
        let mut q = self
            .inner
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if q.len() >= self.inner.cap {
            self.inner
                .dropped
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return false;
        }
        q.push_back(event);
        true
    }

    /// Queue a key press. The ergonomic form of [`EventPump::inject`].
    pub fn inject_key(&self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        self.inject(Event::Key(KeyEvent {
            code,
            modifiers,
            // ★ `Press`, explicitly. `egaku_term`'s own key routing drops
            // anything that is not a press, so a synthetic event built with
            // the default kind would be accepted here and discarded three
            // frames later with nothing logged.
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }))
    }

    /// Queue each character of `text` as a key press.
    ///
    /// Returns the number of characters actually queued, which is `text.chars().count()`
    /// unless the queue filled — so a caller can compare and notice.
    pub fn inject_text(&self, text: &str) -> usize {
        text.chars()
            .take_while(|c| self.inject_key(KeyCode::Char(*c), KeyModifiers::NONE))
            .count()
    }

    /// Events waiting to be dispatched.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.inner
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Events refused because the queue was full, since construction.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.inner
            .dropped
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn take(&self) -> Option<Event> {
        self.inner
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
    }
}


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

    /// Optional: how long to wait for an event before calling [`App::tick`].
    ///
    /// ── ★ WHY THIS EXISTS ────────────────────────────────────────────────
    /// The loop below reads events with `event::read()`, which BLOCKS. That
    /// makes an egaku-term app unable to react to anything except a
    /// keypress: not a clock, not a file change, not a message from another
    /// thread. Any app that needs one has had to grow a second thread and a
    /// way to fake a keystroke, which is how a TUI ends up with two input
    /// paths that disagree.
    ///
    /// Returning `None` — the default — keeps `event::read()` exactly as it
    /// was, so every existing app is unchanged byte for byte and nothing
    /// opts in by accident.
    ///
    /// The concrete case that forced it: mukae's greeter publishes its login
    /// flow over kanshou so an agent can OBSERVE it, and could never be
    /// answered, because a queued synthetic keystroke had no moment to be
    /// drained in. The observation surface existed and the loop had no way
    /// to reach it.
    fn tick_interval(&self) -> Option<std::time::Duration> {
        None
    }

    /// Optional: called when [`App::tick_interval`] elapsed with no event.
    ///
    /// Runs on the SAME thread as `handle` and `draw`, so an implementation
    /// may touch app state directly and needs no lock of its own beyond
    /// whatever it shares with other threads. A tick that changes state is
    /// followed by a redraw on the next pass, exactly as an event is.
    fn tick(&mut self) {}

    /// Optional: a queue this app accepts injected events from.
    ///
    /// Returning `Some` opts the loop into polling (see [`PUMP_POLL`]) so
    /// queued events are acted on promptly. The default is `None`, which
    /// leaves `event::read()` blocking exactly as before.
    ///
    /// The app owns the pump and hands clones to whatever drives it — an MCP
    /// sidecar, a test, a replay. See [`EventPump`] for why injection goes
    /// through the queue rather than through app state.
    fn event_pump(&self) -> Option<&EventPump> {
        None
    }

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

    /// What to do with a key **no binding claimed**, on the typed path.
    ///
    /// Defaulted to [`Unclaimed::Text`], which is exactly the behaviour every
    /// app had before this existed — so adding it moved nobody. Override it,
    /// per stance, when unbound must mean *undefined* rather than *typed*:
    /// in vim's Normal mode `d` is a verb, not a character.
    ///
    /// It takes `&self` and is read once per event, so a modal app answers
    /// from whatever it currently is and the runtime holds no mode state.
    fn unclaimed(&self) -> Unclaimed {
        Unclaimed::Text
    }

    /// This is the hook that makes a text field possible without the keymap
    /// swallowing its letters: a search mode binds only Escape/Return/
    /// Backspace, and every other key misses and lands here.
    fn on_text(&mut self, _c: char) {}
}

/// What the runtime does with a key **no binding claimed**, on the typed
/// (`hotkey_map`) dispatch path.
///
/// # Why this is egaku-term's own axis and not `awase::KeyMode::passthrough`
///
/// `awase::KeyMode` already carries a `passthrough` flag, and honouring it
/// looks like free money. It would be a fleet-wide regression delivered
/// through a semver-*compatible* minor, which no version number protects
/// against.
///
/// Measured 2026-08-09: **every** typed-path `KeyMode` in the fleet is
/// constructed `passthrough: false` — pauta, the alicerce wizard, acervo-ui's
/// `normal` *and* `search` modes, banken's picker and app, and this crate's own
/// test app. acervo-ui's `search` mode binds only Escape/Return/Backspace
/// *precisely so* every other key misses into `on_text`. Reading the flag would
/// have silently killed text entry in five applications the next time they ran
/// `cargo update`.
///
/// So the axis is named here, defaults to today's behaviour, and an app opts in
/// deliberately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Unclaimed {
    /// A printable goes to `on_text`; anything else goes to `on_unhandled`.
    /// **The default, and exactly what every existing app already gets.**
    #[default]
    Text,
    /// Everything goes to `on_unhandled` — nothing is treated as text.
    ///
    /// What a modal app wants in a command stance: in vim's Normal mode `d` is
    /// a verb, not a character, and an unbound key must not silently land in
    /// the buffer. An app that returns this and does not override
    /// `on_unhandled` swallows the key, which is the correct default for a
    /// stance where unbound means undefined.
    Consume,
}

/// Where an **unclaimed** key goes. Returned by [`route_unclaimed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnclaimedRoute {
    /// Deliver this character to `on_text`.
    Text(char),
    /// Hand the whole event to `on_unhandled`.
    Unhandled,
}

/// The one place the unclaimed-key decision is made.
///
/// **Both runtimes call this, and so does its test.** An earlier version of
/// that test re-implemented the branch and asserted against its own copy —
/// which passes whatever the runtime does, and is the same vacuity that let a
/// table render UIDs for weeks behind three green tests. One function, one
/// caller shape, no mirror.
pub(crate) fn route_unclaimed(axis: Unclaimed, evt: &Event) -> UnclaimedRoute {
    match axis {
        Unclaimed::Consume => UnclaimedRoute::Unhandled,
        Unclaimed::Text => match text_char(evt) {
            Some(c) => UnclaimedRoute::Text(c),
            None => UnclaimedRoute::Unhandled,
        },
    }
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

        // ★ POLL-THEN-READ, ONLY WHEN THE APP ASKED FOR IT. With no
        // `tick_interval` this is `event::read()` and nothing else — the
        // original blocking call, so an app that never opts in cannot pay a
        // wakeup it did not request. `continue` rather than falling through:
        // a tick is not an event, and handing one to the keymap would make
        // every app's `on_unhandled` fire on a timer.
        // ★ ONLY THE *SOURCE* OF THE EVENT CHANGES HERE — never its routing.
        // The dispatch chain below is left exactly as it was, so an injected
        // event provably takes the same path as a keystroke: there is one
        // copy of that chain and nothing can route around it. Factoring the
        // chain instead would have created the very divergence an injection
        // API exists to avoid.
        //
        // Injected events are drained BEFORE the terminal is polled. They
        // have already arrived; making them wait behind a poll would add a
        // frame of latency to every driven keystroke for no reason.
        let injected = app.event_pump().and_then(EventPump::take);
        let evt = if let Some(e) = injected {
            e
        } else {
            // An attached pump implies polling even with no `tick_interval`,
            // or an injected event would sit unseen behind a blocking read
            // until the operator happened to touch a key — which is the
            // failure this whole mechanism exists to remove.
            let wait = app
                .tick_interval()
                .or_else(|| app.event_pump().map(|_| PUMP_POLL));
            match wait {
                None => event::read()?,
                Some(interval) => {
                    if event::poll(interval)? {
                        event::read()?
                    } else {
                        app.tick();
                        continue;
                    }
                }
            }
        };
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
            // Typed path, unclaimed. WHICH of the two readings is the app's
            // to declare; the default is `Text`, i.e. unchanged.
            if app.unclaimed() == Unclaimed::Consume {
                app.on_unhandled(&evt);
            } else
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

#[cfg(test)]
mod unclaimed_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Act {
        Quit,
    }

    /// An app that records where unclaimed keys landed, and lets a test pick
    /// the axis.
    struct Probe {
        mode: awase::KeyMode<Act>,
        unclaimed: Unclaimed,
        text: Vec<char>,
        unhandled: usize,
    }

    impl Probe {
        fn new(unclaimed: Unclaimed) -> Self {
            // Exactly the shape acervo-ui's `search` mode has: one binding,
            // `passthrough: false`, everything else expected to miss.
            let mut mode: awase::KeyMode<Act> = awase::KeyMode::typed("probe", false);
            let _ = mode.add_binding(awase::Binding::new(
                awase::Hotkey::new(awase::Modifiers::NONE, awase::Key::Escape),
                Act::Quit,
            ));
            Self {
                mode,
                unclaimed,
                text: Vec::new(),
                unhandled: 0,
            }
        }
    }

    impl App for Probe {
        type Action = Act;
        fn keymap(&self) -> &KeyMap<Act> {
            static EMPTY: std::sync::OnceLock<KeyMap<Act>> = std::sync::OnceLock::new();
            EMPTY.get_or_init(KeyMap::new)
        }
        fn hotkey_map(&self) -> Option<&awase::KeyMode<Act>> {
            Some(&self.mode)
        }
        fn unclaimed(&self) -> Unclaimed {
            self.unclaimed
        }
        fn handle(&mut self, _a: &Act) {}
        fn draw(&self, _f: &mut Buffer) -> Result<()> {
            Ok(())
        }
        fn should_quit(&self) -> bool {
            false
        }
        fn on_text(&mut self, c: char) {
            self.text.push(c);
        }
        fn on_unhandled(&mut self, _e: &Event) {
            self.unhandled += 1;
        }
    }

    /// Drive one unclaimed printable through **the function `run` itself
    /// calls** — never a re-implementation of it, which would assert against
    /// this test's own copy of the branch and pass whatever the runtime does.
    fn feed(app: &mut Probe, c: char) {
        let evt = Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        assert!(
            typed_dispatch(app.hotkey_map().unwrap(), &evt).is_none(),
            "the probe key must be UNCLAIMED or this test proves nothing",
        );
        match route_unclaimed(app.unclaimed(), &evt) {
            UnclaimedRoute::Text(ch) => app.on_text(ch),
            UnclaimedRoute::Unhandled => app.on_unhandled(&evt),
        }
    }

    /// **THE REGRESSION GATE, and it is the reason this axis exists at all.**
    /// Every typed-path `KeyMode` in the fleet is built `passthrough: false`,
    /// and five apps rely on unclaimed printables reaching `on_text` —
    /// acervo-ui's search mode binds only Escape/Return/Backspace precisely so
    /// other keys miss into it. The DEFAULT must be that behaviour, or a
    /// `cargo update` silently kills text entry in all of them.
    #[test]
    fn the_default_still_delivers_unclaimed_printables_as_text() {
        assert_eq!(Unclaimed::default(), Unclaimed::Text, "the default itself");
        let mut p = Probe::new(Unclaimed::default());
        feed(&mut p, 'q');
        feed(&mut p, 'j');
        assert_eq!(p.text, vec!['q', 'j'], "typed, exactly as before");
        assert_eq!(p.unhandled, 0);
    }

    /// The opt-in: in a command stance nothing unclaimed is text, so `d` is a
    /// verb rather than a character landing in the buffer.
    #[test]
    fn consume_routes_every_unclaimed_key_away_from_text() {
        let mut p = Probe::new(Unclaimed::Consume);
        feed(&mut p, 'd');
        feed(&mut p, 'w');
        assert!(p.text.is_empty(), "nothing may reach the buffer");
        assert_eq!(p.unhandled, 2, "the app still sees them, and may swallow");
    }

    /// A CLAIMED key is unaffected by the axis — it dispatches either way.
    /// Without this, `Consume` could be "swallow everything" and pass.
    #[test]
    fn a_bound_chord_still_dispatches_under_either_axis() {
        for axis in [Unclaimed::Text, Unclaimed::Consume] {
            let p = Probe::new(axis);
            let evt = Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert_eq!(
                typed_dispatch(p.hotkey_map().unwrap(), &evt),
                Some(Act::Quit),
                "{axis:?}",
            );
        }
    }
}

#[cfg(test)]
mod tick_tests {
    use super::*;

    struct Bare;
    impl App for Bare {
        type Action = ();
        fn keymap(&self) -> &KeyMap<()> {
            unreachable!("not driven in this test")
        }
        fn handle(&mut self, _: &()) {}
        fn draw(&self, _: &mut Buffer) -> Result<()> {
            Ok(())
        }
        fn should_quit(&self) -> bool {
            true
        }
    }

    /// ★ THE LOAD-BEARING PROPERTY. `run` calls `event::read()` — a blocking
    /// read — unless the app asks otherwise, so an app that has not opted in
    /// must report `None` and keep exactly its old behaviour. A default of
    /// `Some(..)` would put a wakeup on every TUI in the fleet, and the cost
    /// would show up as battery drain nobody could attribute.
    #[test]
    fn an_app_that_did_not_opt_in_still_blocks() {
        assert_eq!(
            Bare.tick_interval(),
            None,
            "the default must leave event::read() untouched"
        );
    }

    /// `tick` must be safe to call on an app that never implements it —
    /// otherwise the default would be a trap rather than a no-op.
    #[test]
    fn the_default_tick_is_a_no_op() {
        let mut a = Bare;
        a.tick();
        a.tick();
    }

    struct Ticker(u32);
    impl App for Ticker {
        type Action = ();
        fn keymap(&self) -> &KeyMap<()> {
            unreachable!()
        }
        fn handle(&mut self, _: &()) {}
        fn draw(&self, _: &mut Buffer) -> Result<()> {
            Ok(())
        }
        fn should_quit(&self) -> bool {
            self.0 >= 3
        }
        fn tick_interval(&self) -> Option<std::time::Duration> {
            Some(std::time::Duration::from_millis(5))
        }
        fn tick(&mut self) {
            self.0 += 1;
        }
    }

    #[test]
    fn an_opted_in_app_reports_its_interval_and_ticks_its_own_state() {
        let mut t = Ticker(0);
        assert_eq!(t.tick_interval(), Some(std::time::Duration::from_millis(5)));
        assert!(!t.should_quit());
        t.tick();
        t.tick();
        t.tick();
        assert!(t.should_quit(), "tick must be able to end the loop");
    }
}

#[cfg(test)]
mod pump_tests {
    use super::*;

    #[test]
    fn an_injected_key_is_a_press_because_the_router_drops_anything_else() {
        let p = EventPump::new();
        assert!(p.inject_key(KeyCode::Char('x'), KeyModifiers::NONE));
        match p.take() {
            Some(Event::Key(k)) => assert_eq!(
                k.kind,
                KeyEventKind::Press,
                "a non-Press synthetic key is accepted here and silently \
                 discarded by the router later — the worst kind of bug"
            ),
            other => panic!("expected a key event, got {other:?}"),
        }
    }

    #[test]
    fn text_is_queued_in_order() {
        let p = EventPump::new();
        assert_eq!(p.inject_text("abc"), 3);
        let mut got = String::new();
        while let Some(Event::Key(k)) = p.take() {
            if let KeyCode::Char(c) = k.code {
                got.push(c);
            }
        }
        assert_eq!(got, "abc", "order is the whole contract for typed input");
    }

    /// ★ THE PROPERTY THAT MATTERS MOST. A dropped keystroke must be
    /// REPORTED, never silent: the producer believing it typed a password
    /// while the app received half of one is unrecoverable and undiagnosable.
    #[test]
    fn a_full_queue_refuses_and_counts_rather_than_dropping_quietly() {
        let p = EventPump::with_capacity(2);
        assert!(p.inject_key(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(p.inject_key(KeyCode::Char('b'), KeyModifiers::NONE));
        assert!(
            !p.inject_key(KeyCode::Char('c'), KeyModifiers::NONE),
            "the third must be refused, not silently dropped"
        );
        assert_eq!(p.dropped(), 1, "refusals are counted so a caller can assert");
        assert_eq!(p.pending(), 2);
    }

    #[test]
    fn inject_text_reports_the_short_count_when_the_queue_fills() {
        let p = EventPump::with_capacity(3);
        assert_eq!(
            p.inject_text("hello"),
            3,
            "the caller must be able to see that only part of the text landed"
        );
    }

    #[test]
    fn a_clone_shares_the_queue_so_another_thread_can_drive() {
        let p = EventPump::new();
        let q = p.clone();
        std::thread::spawn(move || {
            q.inject_text("hi");
        })
        .join()
        .unwrap();
        assert_eq!(p.pending(), 2, "a pump must be usable from the thread that drives it");
    }

    struct Bare2;
    impl App for Bare2 {
        type Action = ();
        fn keymap(&self) -> &KeyMap<()> {
            unreachable!()
        }
        fn handle(&mut self, _: &()) {}
        fn draw(&self, _: &mut Buffer) -> Result<()> {
            Ok(())
        }
        fn should_quit(&self) -> bool {
            true
        }
    }

    #[test]
    fn an_app_without_a_pump_keeps_the_blocking_read() {
        assert!(
            Bare2.event_pump().is_none(),
            "no pump and no tick_interval must leave event::read() untouched"
        );
        assert_eq!(Bare2.tick_interval(), None);
    }
}
