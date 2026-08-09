//! Async runtime — `AsyncApp` trait + `run_async()`.
//!
//! Available behind the `tokio` feature flag. Apps that don't need async
//! should use the sync [`crate::App`] / [`crate::run`] pair instead — it
//! has no tokio dependency.
//!
//! This runtime exists because the bulk of pleme-io's TUI consumers are
//! tokio-based (arnes-tui, kura-tui, hikyaku, tanken, …). They need an
//! event loop that:
//!
//! 1. Drives a `crossterm::event::EventStream` (tokio-aware) instead of
//!    blocking on `crossterm::event::read()`.
//! 2. Lets the app `await` between events (provider streams, MCP calls,
//!    file watch, etc.) without dropping events.
//! 3. Composes via `tokio::select!` so events and external futures share
//!    the same loop.
//!
//! `run_async` does (1)+(2). For (3) — apps that need to multiplex events
//! with their own streams. **`AsyncApp::tick` DOES NOT EXIST** — this line
//! promised it, and a `tick_interval`, from at least 0.3.1 onward. The trait
//! below has four methods and none of them is a tick; `run_async`'s loop is a
//! single `events.next().await` with no `tokio::select!`, so **a consumer
//! cannot be woken by anything except input**. Corrected 2026-08-03 after the
//! promise cost a downstream diagnosis: banken wrote `refresh()` against this
//! contract and it has had zero callers ever since.
//!
//! Until a tick lands, the shipped way to get a periodic wakeup is to skip
//! `run_async` and hand-roll the loop — `hibiki/src/main.rs:209` does exactly
//! that with an `EventStream` arm plus a `tokio::time::interval` arm, using
//! only public API from this crate. Or skip the runtime and
//! drive [`Terminal`] + drawers directly from your own `select!` loop —
//! both are first-class.

use crossterm::event::{Event, EventStream};
use egaku::KeyMap;
use futures_util::StreamExt;

use crate::app::Unclaimed;
use crate::buffer::Buffer;
use crate::error::Result;
use crate::event::from_crossterm;
use crate::render::render_diff;
use crate::terminal::Terminal;

/// Async cousin of [`crate::App`]. Same shape, but `handle` and `draw` are
/// async, and [`AsyncApp::wake`] lets a non-input source request a redraw.
pub trait AsyncApp: Send {
    /// The action enum the app's [`KeyMap`] resolves keys to.
    type Action;

    /// Borrow the keymap. Called once per event; small and cheap.
    fn keymap(&self) -> &KeyMap<Self::Action>;

    /// Apply a resolved action. Async so the app can await downstream work
    /// (provider call, MCP tool dispatch, …).
    fn handle(
        &mut self,
        action: &Self::Action,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Paint the current state into the frame buffer. Async because some apps
    /// want to fetch preview content lazily; callers that don't can `async {}`.
    /// The buffer is freshly reset before this is called; the runtime diffs it
    /// against the previous frame and flushes only the changed cells.
    fn draw(&self, frame: &mut Buffer) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Return true to exit the loop.
    fn should_quit(&self) -> bool;

    /// Resolve when something OTHER than input should redraw the view.
    ///
    /// The default is [`std::future::pending`] — it never resolves, so an app
    /// that does not override this behaves exactly as before: input-only.
    /// That default is what makes this addition non-breaking.
    ///
    /// Override it to await a `watch::Receiver::changed()`, an interval tick,
    /// or a channel recv. `run_async` selects over this and the event stream,
    /// so resolving here causes exactly one redraw and nothing else — it is a
    /// *wakeup*, not a callback, which is why it takes `&self` and returns
    /// nothing. Mutating state belongs in whatever produced the signal.
    ///
    /// Cancellation-safety is the caller's obligation: the returned future is
    /// dropped un-polled whenever an input event wins the select. A
    /// `watch::Receiver::changed()` or a `tokio::time::sleep` is safe; a future
    /// that consumes from a queue and buffers internally is not, and will drop
    /// items on every keystroke.
    ///
    /// It takes `&self`, not `&mut self`, and that is deliberate: a signal
    /// future is the one thing here that gets destroyed part-way through, so
    /// it must not be able to leave half-written state behind. Do the writing
    /// in [`AsyncApp::on_wake`], which runs after the select has resolved and
    /// therefore always runs to completion.
    fn wake(&self) -> impl std::future::Future<Output = ()> + Send {
        std::future::pending()
    }

    /// Apply whatever [`AsyncApp::wake`] signalled, then the runtime redraws.
    ///
    /// This is the `&mut self` half of the wakeup: re-read a source, drain a
    /// buffer, advance an animation. It is called only after `wake` resolved,
    /// outside the `select!`, so unlike the signal future it is never dropped
    /// part-way — which is what makes a torn update unrepresentable rather
    /// than merely unlikely.
    fn on_wake(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }

    /// Optional fall-through for events the keymap didn't resolve.
    /// Optional: dispatch through **typed** [`awase::Hotkey`] chords instead
    /// of the string [`KeyMap`] path.
    ///
    /// Async peer of [`crate::App::hotkey_map`] — see its doc for why an app
    /// wants this. Returning `Some` opts in; the default `None` keeps
    /// [`Self::keymap`] as the dispatch source, so existing implementations
    /// are untouched.
    fn hotkey_map(&self) -> Option<&awase::KeyMode<Self::Action>> {
        None
    }

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

    /// Optional: a printable character no binding claimed.
    ///
    /// Async peer of [`crate::App::on_text`], and likewise **only fires on
    /// the typed path**.
    fn on_text(&mut self, _c: char) -> impl std::future::Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }

    fn on_unhandled(
        &mut self,
        _event: &Event,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }
}

/// Run an [`AsyncApp`] on the current tokio runtime.
///
/// Owns the terminal for the duration of the call. The event source is a
/// [`crossterm::event::EventStream`] so `await`-ing it doesn't block other
/// tokio tasks.
///
/// `Action: Clone + Send` because the runtime detaches the resolved action
/// from the keymap borrow before invoking [`AsyncApp::handle`] — same
/// rationale as the sync runtime, plus `Send` because we're in tokio.
pub async fn run_async<A>(app: &mut A) -> Result<()>
where
    A: AsyncApp,
    A::Action: Clone + Send,
{
    let mut term = Terminal::enter()?;
    let mut events = EventStream::new();
    let (mut cols, mut rows) = term.size()?;
    let mut prev = Buffer::empty(cols, rows);
    let mut back = Buffer::empty(cols, rows);
    term.clear()?;
    term.flush()?;

    while !app.should_quit() {
        back.reset();
        app.draw(&mut back).await?;
        let sync = term.sync_output();
        render_diff(term.out(), &prev, &back, sync)?;
        std::mem::swap(&mut prev, &mut back);

        // The one non-input wakeup path. Before this existed the loop had a
        // single parking await on `events.next()`, so a view could not update
        // without a keystroke — an embedded refresher would spin invisibly.
        // `wake()` defaults to `pending()`, so an app that ignores it lowers
        // to exactly the previous behaviour.
        // `Some(_)` = the event stream won, `None` = the wakeup won. The
        // immutable borrow taken by `app.wake()` ends with the `select!`
        // expression, which is what lets `on_wake` take `&mut app` below.
        let outcome = tokio::select! {
            biased;
            e = events.next() => Some(e),
            () = app.wake() => None,
        };
        let Some(evt) = outcome else {
            app.on_wake().await?;
            // Redraw only: back to the top, which re-draws and re-selects.
            // No event to dispatch.
            continue;
        };
        match evt {
            Some(Ok(evt)) => {
                if let Event::Resize(w, h) = evt {
                    cols = w;
                    rows = h;
                    prev.resize(cols, rows);
                    back.resize(cols, rows);
                    term.clear()?;
                    term.flush()?;
                    app.on_unhandled(&evt).await?;
                } else if let Some(action) = app
                    .hotkey_map()
                    .and_then(|m| crate::app::typed_dispatch(m, &evt))
                {
                    // Typed path. The action is cloned out inside
                    // `typed_dispatch`, ending the borrow before `handle`.
                    app.handle(&action).await?;
                } else if app.hotkey_map().is_some() {
                    // Typed path, unclaimed. The decision is made in ONE
                    // place, shared with the sync runtime and with its test.
                    match crate::app::route_unclaimed(app.unclaimed(), &evt) {
                        crate::app::UnclaimedRoute::Text(c) => app.on_text(c).await?,
                        crate::app::UnclaimedRoute::Unhandled => app.on_unhandled(&evt).await?,
                    }
                } else {
                    // String path — unchanged.
                    if let Some(combo) = from_crossterm(&evt)
                        && let Some(action) = app.keymap().lookup(&combo).cloned()
                    {
                        app.handle(&action).await?;
                        continue;
                    }
                    app.on_unhandled(&evt).await?;
                }
            }
            Some(Err(e)) => return Err(e.into()),
            None => return Ok(()), // event stream exhausted (terminal closed)
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use egaku::KeyCombo;

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

    impl AsyncApp for Counter {
        type Action = Act;

        fn keymap(&self) -> &KeyMap<Act> {
            &self.keys
        }

        async fn handle(&mut self, a: &Act) -> Result<()> {
            match a {
                Act::Bump => self.count += 1,
                Act::Quit => self.done = true,
            }
            Ok(())
        }

        async fn draw(&self, _frame: &mut Buffer) -> Result<()> {
            Ok(())
        }

        fn should_quit(&self) -> bool {
            self.done
        }
    }

    // Tokio runtime smoke test — exercises trait wiring without taking a
    // real terminal (we never call run_async in CI). The point is that
    // the trait compiles, the action type traverses the borrow correctly,
    // and tokio's executor is happy with the async fns.
    #[tokio::test]
    async fn keymap_dispatch_through_async_app() {
        let mut c = Counter::new();
        let bump = c
            .keymap()
            .lookup(&KeyCombo::key("space"))
            .copied()
            .expect("bump bound");
        c.handle(&bump).await.unwrap();
        assert_eq!(c.count, 1);
        assert!(!c.should_quit());
        c.handle(&Act::Quit).await.unwrap();
        assert!(c.should_quit());
    }

    #[tokio::test]
    async fn unhandled_default_returns_ok() {
        let mut c = Counter::new();
        c.on_unhandled(&Event::Resize(80, 24)).await.unwrap();
        assert_eq!(c.count, 0);
    }
}
