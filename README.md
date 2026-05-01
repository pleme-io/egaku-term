# egaku-term

Terminal renderer + runtime for [`egaku`](https://github.com/pleme-io/egaku)
widget state machines.

`egaku` is pure logic — `TextInput`, `ListView`, `TabBar`, `Modal`, etc. as
state machines with no rendering and no event loop. Every consumer that
wants a terminal UI today re-implements the same five things:

1. Enable raw mode + alternate screen + hide cursor on entry.
2. Restore those on exit and on panic.
3. Pump `crossterm::event::read()` in a loop.
4. Translate `KeyEvent` → an action your app understands.
5. Style each widget render — selected line reverse video, padding, column
   wrapping, scrollbars, modal centering.

`egaku-term` is the missing brick. One library, one runtime, one set of
drawers shared across every pleme-io TUI.

## Minimal example

```rust
use egaku::ListView;
use egaku_term::{App, Terminal, Result, key, keymap, draw, run};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action { Next, Prev, Quit, Select }

struct Wizard {
    list: ListView,
    keys: egaku::KeyMap<Action>,
    done: bool,
    chosen: Option<usize>,
}

impl App for Wizard {
    type Action = Action;
    fn keymap(&self) -> &egaku::KeyMap<Action> { &self.keys }
    fn handle(&mut self, action: &Action) {
        match action {
            Action::Next => self.list.select_next(),
            Action::Prev => self.list.select_prev(),
            Action::Select => {
                self.chosen = Some(self.list.selected_index());
                self.done = true;
            }
            Action::Quit => self.done = true,
        }
    }
    fn draw(&self, term: &mut Terminal) -> Result<()> {
        draw::header(term, egaku::Rect::new(0.0, 0.0, 80.0, 1.0), "Pick one")?;
        draw::list(term, egaku::Rect::new(0.0, 2.0, 40.0, 10.0), &self.list, true)
    }
    fn should_quit(&self) -> bool { self.done }
}

fn main() -> Result<()> {
    let mut wizard = Wizard {
        list: ListView::new(vec!["one".into(), "two".into(), "three".into()], 10),
        keys: keymap! {
            ["q", "esc"]      => Action::Quit,
            ["j", "down"]     => Action::Next,
            ["k", "up"]       => Action::Prev,
            ["enter"]         => Action::Select,
        },
        done: false,
        chosen: None,
    };
    run(&mut wizard)?;
    if let Some(idx) = wizard.chosen {
        println!("you picked {idx}");
    }
    Ok(())
}
```

## What's in here

| Module        | Surface                                                                              |
|---------------|--------------------------------------------------------------------------------------|
| `terminal`    | `Terminal::enter()` + Drop-safe restore                                              |
| `event`       | `from_crossterm(Event) -> Option<KeyCombo>`, `key!` macro                            |
| `keymap`      | `keymap!` declarative macro                                                          |
| `theme`       | `Palette::from_theme(&egaku::Theme)` — RGBA → crossterm `Color`                      |
| `draw`        | `header`/`list`/`text_input`/`tabs`/`modal`/`scrollbar`/`split` + `paragraph`/`bordered_block`/`status_line` |
| `app`         | sync `App` trait + `run()` runtime                                                   |
| `app_async` *(feature `tokio`)* | async `AsyncApp` trait + `run_async()` runtime |

Re-exports `crossterm` so consumers don't have to track its version
independently.

## Sync vs async

Two runtimes ship side by side:

```rust
// Sync — no tokio dependency.
use egaku_term::{App, run};
run(&mut my_app)?;

// Async — gated behind feature `tokio`.
use egaku_term::{AsyncApp, run_async};
run_async(&mut my_app).await?;
```

Pick `App` for one-shot wizards (alicerce-ui, picker scripts) and the
sync side has zero async deps. Pick `AsyncApp` when the app multiplexes
streams (chat clients, agentic loops, IMAP, MCP) — `run_async` drives
`crossterm::event::EventStream` so awaiting between events doesn't block
other tokio tasks.

```toml
# Cargo.toml — pick one
egaku-term = { git = "https://github.com/pleme-io/egaku-term" }                          # sync
egaku-term = { git = "https://github.com/pleme-io/egaku-term", features = ["tokio"] }    # async
```

## Macros

```rust
use egaku_term::{key, keymap};
use egaku::KeyMap;

let q = key!("q");
let ctrl_c = key!(ctrl + "c");
let ctrl_shift_p = key!(ctrl + shift + "p");

#[derive(Clone, Copy)]
enum Act { Quit, Save }

let km: KeyMap<Act> = keymap! {
    ["q", "esc"]   => Act::Quit,
    [ctrl + "s"]   => Act::Save,
};
```

## Build

```bash
cargo build
cargo test
```

## License

MIT.
