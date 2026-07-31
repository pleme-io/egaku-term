# Egaku-Term — terminal renderer + runtime for egaku widgets

## Why this crate exists

`egaku` is the canonical pleme-io widget toolkit, but by design it is
**pure logic, no rendering** — `TextInput`, `ListView`, `TabBar`, `Modal`
etc. are state machines that consumers project onto a renderer. For GPU
apps that renderer is `garasu`. For terminal apps the fleet had no
shared renderer at all — every consumer (alicerce-ui, hikyaku, kura,
arnes, escriba, tanken, …) hand-rolled the same five steps:

1. Enable raw mode + alternate screen + hide cursor.
2. Restore on exit and on panic.
3. Pump `crossterm::event::read()` in a loop.
4. Translate `KeyEvent` → an action enum.
5. Style each widget render — selected line reverse video, padding,
   column wrapping, scrollbar, modal centering.

`egaku-term` lifts those five steps into one crate. One library, one
runtime, one set of drawers shared across every pleme-io TUI.

## Layer position

```
Application TUI code
       ↓
    egaku-term (Terminal lifecycle + Event adapter + drawers + App runtime)
       ↓
    egaku (state machines)         crossterm (raw terminal)
```

## Build & test

```bash
cargo build
cargo test
```

**Both are green** as of 2026-07-31, against `egaku 0.1.4 @ e602369` — the
commit that added `Selectable`/`TableView`/the widget-owned `focused` flags.
They were RED for the length of one session while that commit sat unpushed,
verified in the meantime under a `--config patch` override rather than by
committing a lock that named an unfetchable rev.

That waiver is gone rather than merely satisfied, and `tests/pending_egaku_bump.rs`
is why: it ties the waiver token to `Cargo.lock`'s resolved `egaku` source,
stayed a no-op for the whole waiver's life, and **fired the moment the bump
landed** — *"STALE WAIVER … a false claim that this repo does not compile
against its locked egaku."* A pending note that nothing enforces outlives its
own truth; this one could not.

The token is deliberately not spelled out above: the seal matches the literal
string, so quoting it in prose re-arms the waiver. That is the seal working,
not a rough edge — it cannot tell an explanation from a claim, and it should
not try.

## Architecture

| Module       | Surface                                                                              |
|--------------|--------------------------------------------------------------------------------------|
| `terminal`   | `Terminal::enter()` + Drop-safe restore                                              |
| `event`      | `from_crossterm(Event) -> Option<KeyCombo>`, `key!` macro                            |
| `keymap`     | `keymap!` declarative macro                                                          |
| `theme`      | `Palette::from_theme(&egaku::Theme)` — RGBA → crossterm `Color`                      |
| `draw`       | the `Draw` trait + `header` / `list` / `table` / `text_input` / `tabs` / `modal` / `scrollbar` / `split` / `paragraph` / `bordered_block` / `status_line` |
| `app`        | sync `App` trait + `run()` runtime                                                   |
| `app_async` (feature `tokio`) | async `AsyncApp` trait + `run_async()` over `crossterm::EventStream` |
| `error`      | `Error`/`Result`                                                                     |

Re-exports `crossterm` so consumers don't have to track its version
independently.

### The `Draw` trait — and why it lives here, not in egaku

`egaku_term::Draw` is `fn draw(&self, buf: &mut Buffer, rect: Rect, palette:
&Palette)` — one uniform, object-safe surface over the drawer families.

It is in **this** crate on purpose. A drawer needs a `Buffer` and a
`Palette`, both terminal concepts; egaku depends on serde / tracing /
thiserror / unicode-* and no renderer at all, which is what lets one
`ListView` value drive a GPU pane (`garasu`) and a TTY pane (this crate).
`egaku-term → egaku` is one-way and load-bearing — a `Draw` trait upstream
would reverse it. egaku owns the *state* vocabulary (`Selectable`); each
renderer owns its own *rendering* vocabulary.

**Nothing was replaced.** `list` / `tabs` / `text_input` keep their explicit
`focused: bool` parameter, for callers tracking focus out-of-band via
`egaku::FocusManager` (which keys focus by widget *name*). `Draw` sits on
top and sources `focused` from the widget's own `is_focused()` — which is
why those widgets grew a self-owned focus flag upstream. Use the free
functions for a fixed set of named panes; use `&dyn Draw` for a
heterogeneous list rendered in a loop.

**Implementors:** `ListView`, `TableView`, `TextInput`, `TabBar` (focus from
the widget), `ScrollView` (its indicator gutter), `SplitPane` (its divider).

**`Modal` is a documented non-implementor.** `draw::modal`'s signature is
`(buf, bounds, modal, body: &[&str])` and the body is content the `Modal`
value does not own; `Draw::draw` has nowhere to put it. Both forcings were
rejected — giving `Modal` a body field turns a visibility FSM into a content
container, and `impl Draw for (&Modal, &[&str])` makes the trait's meaning
false for one implementor. Call `modal` / `modal_with` directly. Do not
"complete" the roster; the gap is argued in the trait docs.

### `draw::table` — the TableView drawer

`table` / `table_with` render `egaku::TableView<R>`: header row (accent +
bold), a rule, then data rows; column widths are the max **display width**
(via `unicode_width`) of the header and every projected cell. The selected
row gets a full-width selection bar, bold when focused.

**The viewport is derived here, not stored in the model.** `TableView`
carries no scroll offset — it is the ordered row set plus a cursor. This
drawer computes the visible window from `(selected_index, height)`,
bottom-anchored, so the selected row is always on screen. The model this was
lifted from (banken's `draw_pod_table`) had no windowing at all: a table
taller than the terminal simply clipped and the cursor could move somewhere
the operator could not see. Cost: zero new state, and no second place for
the offset to be wrong.

Header and data reach the shared cell-writer as the same `&[&str]` of
already-projected values, so there is no variant to mis-branch on — the
source model's `Option<&Row>` overloaded `None` to mean both "header row"
and "selected data row" and drew the headers onto the selected row.

**Not lifted (stays in the consumer):** per-cell semantic coloring. banken
colors a `STATUS` cell green/red/yellow by pod phase, keyed on a magic
header string; that is app knowledge, and banken's own note names the
destination as a typed `:colorize` hint on the authored column
(`pending-banken: column-render-hints`). Until that hint exists, a consumer
wanting semantic cell colors keeps its own drawer.

## Macros

`key!` — single combo:

```rust
use egaku_term::key;
let q = key!("q");
let ctrl_c = key!(ctrl + "c");
let ctrl_shift_p = key!(ctrl + shift + "p");
```

`keymap!` — full keybinding table:

```rust
use egaku_term::keymap;
#[derive(Clone, Copy)] enum Act { Quit, Next, Save }
let km = keymap! {
    ["q", "esc"]   => Act::Quit,        // array of literals -> same action (Clone)
    "j"            => Act::Next,         // bare literal
    (ctrl + "s")   => Act::Save,         // parenthesised modifier combo
};
# let _ = km;
```

## Conventions

- Edition 2024, Rust 1.89.0+, MIT, clippy pedantic, release profile.
- Builds via `substrate/lib/rust-library.nix` (sibling to egaku).
- crates.io target: yes — public library; HTTPS git URL acceptable for
  consumers since this is part of the shared Rust library tier
  (alongside garasu, egaku, mojiban, irodzuki, …).
- No async, no rendering loop owned by the runtime beyond the explicit
  `run()` entry point. Drawers are synchronous and queue commands.

## First consumer

[`pleme-io/alicerce`](https://github.com/pleme-io/alicerce) —
`alicerce-ui` migrated from a hand-rolled `crossterm` wizard to
`egaku-term::App` + `keymap!` + `draw::*`. ~150 LOC of lifecycle +
event-loop + render boilerplate eliminated.
