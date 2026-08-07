//! Adapter from `crossterm::event::Event` to [`egaku::KeyCombo`].
//!
//! Egaku's keybinding system is intentionally backend-agnostic: a `KeyCombo`
//! is a string key name plus a sorted vector of modifier names. This module
//! is the only place the two vocabularies meet.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use egaku::KeyCombo;

/// Convert a single `crossterm::event::Event` into an [`egaku::KeyCombo`].
///
/// Returns `None` for non-key events, key release events (we only act on
/// `Press`), and key codes that don't map to a stable name (modifier-only
/// presses, `Null`, `CapsLock`, etc.).
#[must_use]
pub fn from_crossterm(event: &Event) -> Option<KeyCombo> {
    let Event::Key(key) = event else { return None };
    if key.kind != KeyEventKind::Press {
        return None;
    }
    from_key_event(key)
}

/// Convert a single `crossterm::event::KeyEvent` into a [`KeyCombo`].
///
/// Lower-level entry point if you've already destructured the event yourself.
#[must_use]
pub fn from_key_event(key: &KeyEvent) -> Option<KeyCombo> {
    let name = key_name(key.code)?;
    let mods = modifier_names(key.modifiers);
    Some(KeyCombo::new(&name, mods))
}

fn key_name(code: KeyCode) -> Option<String> {
    Some(match code {
        KeyCode::Char(c) => {
            // Normalise to lowercase; uppercase is conveyed via the `shift`
            // modifier instead. This matches how most apps want to spell
            // bindings ("ctrl+c", not "ctrl+C").
            c.to_ascii_lowercase().to_string()
        }
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => "backtab".into(),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Insert => "insert".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::PageUp => "pageup".into(),
        KeyCode::PageDown => "pagedown".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => return None,
    })
}

fn modifier_names(mods: KeyModifiers) -> Vec<String> {
    let mut out = Vec::new();
    if mods.contains(KeyModifiers::CONTROL) {
        out.push("ctrl".into());
    }
    if mods.contains(KeyModifiers::ALT) {
        out.push("alt".into());
    }
    if mods.contains(KeyModifiers::SHIFT) {
        out.push("shift".into());
    }
    if mods.contains(KeyModifiers::SUPER) {
        out.push("super".into());
    }
    out
}

// ── Typed delivery: crossterm → awase::Hotkey ───────────────────────────

/// Convert a `crossterm::event::KeyEvent` into a typed [`awase::Hotkey`].
///
/// **The typed peer of [`from_key_event`], and the reason a chord bridge is
/// no longer needed.** `KeyCombo` is a key *name* plus modifier *names* —
/// strings — so anything that authors chords as typed values has to project
/// them down into this crate's spelling before a comparison can happen.
/// Every such projection is a place two vocabularies can disagree, and the
/// fleet grew twenty-one of them. Delivering a `Hotkey` removes the
/// projection instead of standardising it.
///
/// # Refusals are refusals, never guesses
///
/// `None` for: a non-`Press` event, a key crossterm cannot name stably
/// (`Null`, media keys, modifier-only presses), and — importantly — a
/// modifier awase cannot express.
///
/// That last one is a behaviour **difference** from [`from_key_event`], and
/// it is deliberate. `modifier_names` silently drops crossterm's `HYPER` and
/// `META`, so `Meta+X` is delivered as a bare `X` — not a dead key but a
/// *wrong* one, which is worse. Here it is a refusal.
///
/// # Space
///
/// `KeyCode::Char(' ')` becomes [`awase::Key::Space`]. In the string
/// vocabulary it becomes a one-character name `" "`, which no author writes
/// and which is why banken's bridge had to refuse `space` outright. It is an
/// ordinary key here.
#[must_use]
pub fn to_hotkey(key: &KeyEvent) -> Option<awase::Hotkey> {
    use awase::{Key as AKey, Modifiers as AMods};

    // Refuse rather than silently drop a modifier we cannot represent.
    if key
        .modifiers
        .intersects(KeyModifiers::HYPER | KeyModifiers::META)
    {
        return None;
    }

    let k = match key.code {
        // Space is the one char whose crossterm representation and awase name
        // genuinely differ: crossterm reports `Char(' ')`, awase names it
        // `space`. Handled here rather than by teaching awase to accept a
        // literal " " as a key name — that would make `"ctrl+ "` parseable in
        // every operator config, which is not a spelling anyone should write.
        KeyCode::Char(' ') => AKey::Space,
        // Everything else reuses awase's own vocabulary (`Key::from_name`
        // accepts the literal character as well as the canonical name), so
        // this adapter never holds a second copy of the key table.
        KeyCode::Char(c) => AKey::from_name(&c.to_ascii_lowercase().to_string())?,
        KeyCode::Enter => AKey::Return,
        KeyCode::Esc => AKey::Escape,
        // Both are the Tab KEY. `BackTab` IS shift+tab — crossterm reports
        // the composite as its own code and does not always set SHIFT, so the
        // modifier is added below rather than here.
        KeyCode::Tab | KeyCode::BackTab => AKey::Tab,
        KeyCode::Backspace => AKey::Backspace,
        KeyCode::Delete => AKey::Delete,
        KeyCode::Insert => AKey::Insert,
        KeyCode::Home => AKey::Home,
        KeyCode::End => AKey::End,
        KeyCode::PageUp => AKey::PageUp,
        KeyCode::PageDown => AKey::PageDown,
        KeyCode::Up => AKey::Up,
        KeyCode::Down => AKey::Down,
        KeyCode::Left => AKey::Left,
        KeyCode::Right => AKey::Right,
        KeyCode::F(n) => AKey::from_name(&{
            let mut s = String::from("f");
            s.push_str(&n.to_string());
            s
        })?,
        _ => return None,
    };

    let mut mods = AMods::NONE;
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        mods = mods.with(AMods::CTRL);
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        mods = mods.with(AMods::ALT);
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) || matches!(key.code, KeyCode::BackTab) {
        mods = mods.with(AMods::SHIFT);
    }
    if key.modifiers.contains(KeyModifiers::SUPER) {
        mods = mods.with(AMods::CMD);
    }

    Some(awase::Hotkey::new(mods, k))
}

/// Convert a `crossterm::event::Event` into a typed [`awase::Hotkey`].
///
/// The typed peer of [`from_crossterm`]. See [`to_hotkey`] for what is
/// refused and why.
#[must_use]
pub fn hotkey_from_crossterm(event: &Event) -> Option<awase::Hotkey> {
    let Event::Key(key) = event else { return None };
    if key.kind != KeyEventKind::Press {
        return None;
    }
    to_hotkey(key)
}

/// Test support for proving an authored chord can actually be pressed.
///
/// # The class this exists to kill
///
/// A chord that no keypress can produce is a **dead key**: it compiles, it
/// passes every duplicate and conflict check, it appears in the help text,
/// and it never fires. Nothing at runtime reports it. The fleet audit found
/// four live instances across four repos — alicerce advertising a `"G"` that
/// egaku-term can never deliver being the clearest.
///
/// The defence is not a list of valid key names — a second copy of this
/// module's vocabulary would drift from the first. It is to drive synthetic
/// events through [`to_hotkey`], **the same function the runtime uses**, and
/// see what comes out. Nothing here to update when a key is added.
pub mod testing {
    use super::{to_hotkey, KeyCode, KeyEvent, KeyModifiers};
    use std::collections::HashSet;

    /// Every unmodified [`awase::Hotkey`] a terminal can deliver.
    ///
    /// The bare-key set, which is what a full-screen TUI binds: it owns the
    /// keyboard, so it needs no modifier to disambiguate. For a modified
    /// chord, build the `KeyEvent` and call [`to_hotkey`] directly.
    #[must_use]
    pub fn deliverable() -> HashSet<awase::Hotkey> {
        let mut codes: Vec<KeyCode> = "abcdefghijklmnopqrstuvwxyz0123456789 /?-=[];',.`\\"
            .chars()
            .map(KeyCode::Char)
            .collect();
        codes.extend([
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Insert,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
        ]);
        codes.extend((1..=20u8).map(KeyCode::F));

        codes
            .into_iter()
            .filter_map(|code| to_hotkey(&KeyEvent::new(code, KeyModifiers::NONE)))
            .collect()
    }

    /// Assert every chord in `authored` is one a keypress can produce.
    ///
    /// # Panics
    ///
    /// Panics naming the offending chord if any is undeliverable — and also
    /// if the probe itself comes back implausibly small, which is the
    /// non-vacuity floor: without it, a broken [`to_hotkey`] would make every
    /// assertion pass against an empty set.
    pub fn assert_all_deliverable(authored: &[awase::Hotkey]) {
        let reachable = deliverable();
        assert!(
            reachable.len() > 50,
            "the probe found only {} deliverable chords — to_hotkey is broken, \
             and every assertion below would pass vacuously",
            reachable.len()
        );
        for hk in authored {
            assert!(
                reachable.contains(hk),
                "`{hk}` is not deliverable by any keypress — it would never fire"
            );
        }
    }
}

/// Construct a [`KeyCombo`] from a literal description.
///
/// ```
/// use egaku_term::key;
/// let k = key!("q");
/// let ctrl_c = key!(ctrl + "c");
/// let ctrl_shift_p = key!(ctrl + shift + "p");
/// let enter = key!("enter");
/// ```
///
/// The macro normalises modifier order, so `key!(shift + ctrl + "x") ==
/// key!(ctrl + shift + "x")`.
#[macro_export]
macro_rules! key {
    ($name:literal) => {
        $crate::__re::KeyCombo::key($name)
    };
    ($($modifier:ident +)+ $name:literal) => {
        $crate::__re::KeyCombo::new(
            $name,
            vec![ $( stringify!($modifier).to_string() ),+ ],
        )
    };
}

#[cfg(test)]
mod typed_delivery_tests {
    use super::*;
    use awase::{Hotkey, Key as AKey, Modifiers as AMods};

    fn press(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn a_bare_letter_delivers_as_a_typed_key() {
        assert_eq!(
            to_hotkey(&press(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Hotkey::new(AMods::NONE, AKey::Q))
        );
    }

    #[test]
    fn uppercase_arrives_as_lowercase_plus_shift() {
        // The convention the whole fleet shares, and the reason an authored
        // `"G"` is dead: crossterm reports shift+g, never a capital G.
        assert_eq!(
            to_hotkey(&press(KeyCode::Char('G'), KeyModifiers::SHIFT)),
            Some(Hotkey::new(AMods::SHIFT, AKey::G))
        );
    }

    #[test]
    fn modifiers_map_onto_the_typed_flags() {
        assert_eq!(
            to_hotkey(&press(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            Some(Hotkey::new(AMods::CTRL, AKey::S))
        );
        assert_eq!(
            to_hotkey(&press(KeyCode::Char('c'), KeyModifiers::SUPER)),
            Some(Hotkey::new(AMods::CMD, AKey::C)),
            "crossterm SUPER is awase CMD"
        );
    }

    /// Behaviour DIFFERENCE from the string path, and the point of it.
    /// `modifier_names` silently drops HYPER/META, so `Meta+X` is delivered
    /// as a bare `X` — a wrong bind, not a dead one.
    #[test]
    fn an_inexpressible_modifier_is_refused_not_silently_dropped() {
        assert_eq!(
            to_hotkey(&press(KeyCode::Char('x'), KeyModifiers::META)),
            None,
            "META has no awase equivalent — refusing beats delivering a bare x"
        );
        assert_eq!(
            to_hotkey(&press(KeyCode::Char('x'), KeyModifiers::HYPER)),
            None
        );

        // …and the string path still exhibits the old behaviour, which is
        // what makes this a real difference rather than a restatement.
        let combo = from_key_event(&press(KeyCode::Char('x'), KeyModifiers::META))
            .expect("the string path still accepts it");
        assert_eq!(combo.key, "x");
        assert!(
            combo.modifiers.is_empty(),
            "the string path drops META silently — this is the bug being fixed"
        );
    }

    /// banken's bridge had to refuse `space` outright: the string vocabulary
    /// names it `" "`, which no author writes. It is an ordinary key here.
    #[test]
    fn space_is_an_ordinary_key() {
        assert_eq!(
            to_hotkey(&press(KeyCode::Char(' '), KeyModifiers::NONE)),
            Some(Hotkey::new(AMods::NONE, AKey::Space))
        );
        // The string path names it a literal space — unwritable in practice.
        assert_eq!(
            from_key_event(&press(KeyCode::Char(' '), KeyModifiers::NONE))
                .expect("delivers")
                .key,
            " "
        );
    }

    #[test]
    fn the_two_divergent_names_land_on_typed_variants() {
        // `esc`/`enter` on the string side, `Escape`/`Return` on the typed
        // side — the exact two-row table banken maintained by hand.
        assert_eq!(
            to_hotkey(&press(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Hotkey::new(AMods::NONE, AKey::Escape))
        );
        assert_eq!(
            to_hotkey(&press(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Hotkey::new(AMods::NONE, AKey::Return))
        );
    }

    #[test]
    fn backtab_is_shift_tab() {
        assert_eq!(
            to_hotkey(&press(KeyCode::BackTab, KeyModifiers::NONE)),
            Some(Hotkey::new(AMods::SHIFT, AKey::Tab)),
            "BackTab IS shift+tab; crossterm does not always set the flag"
        );
    }

    #[test]
    fn a_key_release_is_not_a_press() {
        let mut e = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        e.kind = KeyEventKind::Release;
        assert_eq!(hotkey_from_crossterm(&Event::Key(e)), None);
    }

    #[test]
    fn the_probe_finds_a_plausible_keyboard() {
        let d = testing::deliverable();
        assert!(d.len() > 50, "only {} — the probe is broken", d.len());
        for k in [AKey::Q, AKey::Escape, AKey::Return, AKey::Slash, AKey::Space] {
            assert!(
                d.contains(&Hotkey::new(AMods::NONE, k)),
                "{k} should be deliverable"
            );
        }
    }

    #[test]
    fn the_probe_rejects_an_undeliverable_chord() {
        // Non-vacuity: the probe must not accept everything. A media key is
        // real in awase's vocabulary and unreachable through a terminal.
        let d = testing::deliverable();
        assert!(!d.contains(&Hotkey::new(AMods::NONE, AKey::VolumeUp)));
    }

    #[test]
    #[should_panic(expected = "would never fire")]
    fn assert_all_deliverable_panics_on_a_dead_chord() {
        testing::assert_all_deliverable(&[Hotkey::new(AMods::NONE, AKey::VolumeUp)]);
    }

    #[test]
    fn assert_all_deliverable_accepts_real_chords() {
        testing::assert_all_deliverable(&[
            Hotkey::new(AMods::NONE, AKey::Q),
            Hotkey::new(AMods::NONE, AKey::Escape),
            Hotkey::new(AMods::NONE, AKey::Space),
        ]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventState;

    fn ev(code: KeyCode, mods: KeyModifiers) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: mods,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    #[test]
    fn plain_char() {
        let combo = from_crossterm(&ev(KeyCode::Char('q'), KeyModifiers::NONE)).unwrap();
        assert_eq!(combo, KeyCombo::key("q"));
    }

    #[test]
    fn uppercase_char_lowers_to_shift_modifier() {
        // crossterm typically reports an uppercase char with SHIFT set
        let combo = from_crossterm(&ev(KeyCode::Char('A'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(combo, KeyCombo::new("a", vec!["shift".into()]));
    }

    #[test]
    fn ctrl_c() {
        let combo = from_crossterm(&ev(KeyCode::Char('c'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(combo, KeyCombo::new("c", vec!["ctrl".into()]));
    }

    #[test]
    fn modifier_order_independent() {
        let a = from_crossterm(&ev(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ))
        .unwrap();
        let b = from_crossterm(&ev(
            KeyCode::Char('p'),
            KeyModifiers::SHIFT | KeyModifiers::CONTROL,
        ))
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn arrows() {
        for (code, name) in [
            (KeyCode::Up, "up"),
            (KeyCode::Down, "down"),
            (KeyCode::Left, "left"),
            (KeyCode::Right, "right"),
        ] {
            let combo = from_crossterm(&ev(code, KeyModifiers::NONE)).unwrap();
            assert_eq!(combo, KeyCombo::key(name));
        }
    }

    #[test]
    fn function_keys() {
        let combo = from_crossterm(&ev(KeyCode::F(5), KeyModifiers::NONE)).unwrap();
        assert_eq!(combo, KeyCombo::key("f5"));
    }

    #[test]
    fn key_release_is_ignored() {
        let evt = Event::Key(KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        });
        assert!(from_crossterm(&evt).is_none());
    }

    #[test]
    fn non_key_event_is_ignored() {
        // Resize events should pass through as None
        assert!(from_crossterm(&Event::Resize(80, 24)).is_none());
    }

    #[test]
    fn key_macro_plain() {
        assert_eq!(key!("q"), KeyCombo::key("q"));
    }

    #[test]
    fn key_macro_with_modifier() {
        assert_eq!(key!(ctrl + "c"), KeyCombo::new("c", vec!["ctrl".into()]));
    }

    #[test]
    fn key_macro_multiple_modifiers() {
        let a = key!(ctrl + shift + "p");
        let b = key!(shift + ctrl + "p");
        assert_eq!(a, b);
    }
}
