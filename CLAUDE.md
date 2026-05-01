# Egaku-Term — terminal renderer + runtime for egaku widgets

> **★★★ CSE / Knowable Construction.** This repo operates under
> **Constructive Substrate Engineering** — canonical specification at
> [`pleme-io/theory/CONSTRUCTIVE-SUBSTRATE-ENGINEERING.md`](https://github.com/pleme-io/theory/blob/main/CONSTRUCTIVE-SUBSTRATE-ENGINEERING.md).
> The Compounding Directive (operational rules) is in the org-level
> pleme-io/CLAUDE.md ★★★ section. Read both before non-trivial changes.

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

## Architecture

| Module       | Surface                                                                              |
|--------------|--------------------------------------------------------------------------------------|
| `terminal`   | `Terminal::enter()` + Drop-safe restore                                              |
| `event`      | `from_crossterm(Event) -> Option<KeyCombo>`, `key!` macro                            |
| `keymap`     | `keymap!` declarative macro                                                          |
| `theme`      | `Palette::from_theme(&egaku::Theme)` — RGBA → crossterm `Color`                      |
| `draw`       | `header` / `list` / `text_input` / `tabs` / `modal` / `scrollbar` / `split` / `paragraph` / `bordered_block` / `status_line` |
| `app`        | sync `App` trait + `run()` runtime                                                   |
| `app_async` (feature `tokio`) | async `AsyncApp` trait + `run_async()` over `crossterm::EventStream` |
| `error`      | `Error`/`Result`                                                                     |

Re-exports `crossterm` so consumers don't have to track its version
independently.

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
