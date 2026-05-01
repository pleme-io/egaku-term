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
