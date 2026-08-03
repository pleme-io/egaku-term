//! The wake arm: a view that updates with no keystroke.
//!
//! Before `AsyncApp::wake`, `run_async` had a single parking await on
//! `events.next()`. Every consumer was input-only, which is why banken's
//! `refresh()` sat with zero callers and why an embedded live-source engine
//! would spin invisibly. These tests pin both halves of the fix: that a
//! non-input signal actually redraws, and that an app which ignores `wake`
//! still behaves exactly as it did before.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use egaku::KeyMap;
use egaku_term::{AsyncApp, Buffer, Result};

/// Counts its own draws and quits after N, driven only by a watch channel.
/// `label` is the state `on_wake` writes and `draw` renders — the round trip
/// that proves the wakeup carried data, not merely a repaint.
struct WakeApp {
    keymap: KeyMap<()>,
    draws: Arc<AtomicUsize>,
    rx: tokio::sync::watch::Receiver<u64>,
    limit: usize,
    label: String,
}

impl AsyncApp for WakeApp {
    type Action = ();

    fn keymap(&self) -> &KeyMap<Self::Action> {
        &self.keymap
    }

    fn handle(&mut self, _a: &()) -> impl std::future::Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }

    fn draw(&self, f: &mut Buffer) -> impl std::future::Future<Output = Result<()>> + Send {
        self.draws.fetch_add(1, Ordering::SeqCst);
        f.set_string(0, 0, &self.label, egaku_term::Style::default());
        async { Ok(()) }
    }

    fn wake(&self) -> impl std::future::Future<Output = ()> + Send {
        let mut rx = self.rx.clone();
        async move {
            // Cancellation-safe: dropped un-polled on every input event.
            let _ = rx.changed().await;
        }
    }

    fn on_wake(&mut self) -> impl std::future::Future<Output = Result<()>> + Send {
        // The `&mut self` half — reads what the signal delivered and writes it
        // into the state `draw` renders.
        self.label = format!("tick {}", *self.rx.borrow_and_update());
        async { Ok(()) }
    }

    fn should_quit(&self) -> bool {
        self.draws.load(Ordering::SeqCst) >= self.limit
    }
}

/// The test that could not pass before this change: a background task bumps a
/// `watch::Sender`, the app applies it and the RENDERED FRAME CHANGES, with
/// ZERO key events injected.
///
/// It cannot call `run_async` (that seizes the real terminal), so it drives
/// the same shape: draw, park on `wake()`, apply via `on_wake()`. The
/// assertion is on the buffer contents, not on a counter — a repaint that
/// renders the same pixels would not be a live view.
#[tokio::test]
async fn a_watch_bump_changes_the_rendered_frame_with_no_key_events() {
    let (tx, rx) = tokio::sync::watch::channel(0u64);
    let draws = Arc::new(AtomicUsize::new(0));
    let mut app = WakeApp {
        keymap: KeyMap::new(),
        draws: Arc::clone(&draws),
        rx,
        limit: 3,
        label: "tick 0".to_owned(),
    };

    tokio::spawn(async move {
        for i in 1..=3u64 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let _ = tx.send(i);
        }
    });

    let mut buf = Buffer::empty(10, 3);
    let mut frames: Vec<String> = Vec::new();
    for _ in 0..3 {
        buf.reset();
        app.draw(&mut buf).await.unwrap();
        frames.push(row0(&buf));
        tokio::time::timeout(std::time::Duration::from_secs(2), app.wake())
            .await
            .expect("wake() must resolve from a non-input signal");
        app.on_wake().await.unwrap();
    }

    assert_eq!(
        draws.load(Ordering::SeqCst),
        3,
        "three draws, no keys pressed"
    );
    assert_eq!(
        frames,
        vec![
            "tick 0".to_owned(),
            "tick 1".to_owned(),
            "tick 2".to_owned()
        ],
        "each wakeup must carry data into the next frame, not just repaint it"
    );
}

/// Row 0 of a buffer as a trimmed string — the frame assertion reads content
/// by position rather than searching a blob.
fn row0(buf: &Buffer) -> String {
    (0..buf.width())
        .filter_map(|x| buf.get(x, 0).map(egaku_term::Cell::symbol))
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// An app that does NOT override `wake` keeps the old behaviour exactly:
/// the default is `pending()`, so it never resolves and the loop parks on
/// input alone. This is what makes the trait addition non-breaking for every
/// existing consumer.
struct InputOnlyApp {
    keymap: KeyMap<()>,
}

impl AsyncApp for InputOnlyApp {
    type Action = ();
    fn keymap(&self) -> &KeyMap<Self::Action> {
        &self.keymap
    }
    fn handle(&mut self, _a: &()) -> impl std::future::Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }
    fn draw(&self, _f: &mut Buffer) -> impl std::future::Future<Output = Result<()>> + Send {
        async { Ok(()) }
    }
    fn should_quit(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn the_default_wake_never_resolves_so_existing_apps_are_unchanged() {
    let app = InputOnlyApp {
        keymap: KeyMap::new(),
    };
    let r = tokio::time::timeout(std::time::Duration::from_millis(80), app.wake()).await;
    assert!(
        r.is_err(),
        "the default wake must be pending() — an app that ignores it stays input-only"
    );
}
