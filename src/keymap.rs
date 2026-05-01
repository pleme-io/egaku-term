//! `keymap!` declarative macro — ergonomic keybinding tables.
//!
//! Hand-authoring a [`KeyMap`](egaku::KeyMap) is verbose:
//!
//! ```
//! use egaku::{KeyCombo, KeyMap};
//! enum Action { Quit, Next }
//! let mut km = KeyMap::new();
//! km.bind(KeyCombo::key("q"), Action::Quit);
//! km.bind(KeyCombo::key("esc"), Action::Quit);
//! km.bind(KeyCombo::key("j"), Action::Next);
//! km.bind(KeyCombo::key("down"), Action::Next);
//! ```
//!
//! With [`keymap!`]:
//!
//! ```
//! use egaku_term::keymap;
//! #[derive(Clone, Copy)]
//! enum Action { Quit, Next, Save }
//! let km = keymap! {
//!     "q" => Action::Quit,
//!     "esc" => Action::Quit,
//!     ["j", "down"] => Action::Next,
//!     (ctrl + "s") => Action::Save,
//! };
//! # let _ = km;
//! ```
//!
//! Three combo forms are supported:
//!
//! - `"q"` — bare string literal, no modifiers.
//! - `["q", "esc", "f1"]` — array of literals; the action is bound to each.
//!   Requires `Action: Clone`.
//! - `(ctrl + "c")` — modifier-prefixed combo, parenthesised. One or more
//!   modifier idents (`ctrl`, `shift`, `alt`, `super`) followed by `+` and
//!   a string literal.

/// Build an [`egaku::KeyMap`] from a list of `combo => action` rows.
///
/// See module docs for combo forms.
#[macro_export]
macro_rules! keymap {
    ( $( $combo:tt => $action:expr ),* $(,)? ) => {{
        let mut __km = $crate::__re::KeyMap::new();
        $( $crate::__keymap_one!(__km, $combo, $action); )*
        __km
    }};
}

/// Internal helper — binds one `combo => action` row.
#[doc(hidden)]
#[macro_export]
macro_rules! __keymap_one {
    // Bare string literal: "q"
    ($km:ident, $name:literal, $action:expr) => {
        $km.bind($crate::__re::KeyCombo::key($name), $action);
    };
    // Parenthesised modifier combo: (ctrl + "c")
    ($km:ident, ( $($modifier:ident +)+ $name:literal ), $action:expr) => {
        $km.bind(
            $crate::__re::KeyCombo::new(
                $name,
                vec![ $( stringify!($modifier).to_string() ),+ ],
            ),
            $action,
        );
    };
    // Array of literals: ["q", "esc"]
    ($km:ident, [ $($name:literal),+ $(,)? ], $action:expr) => {{
        $( $km.bind($crate::__re::KeyCombo::key($name), ($action).clone()); )+
    }};
}

#[cfg(test)]
mod tests {
    use egaku::{KeyCombo, KeyMap};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Action {
        Quit,
        Next,
        Prev,
        Save,
    }

    #[test]
    fn single_binding() {
        let km: KeyMap<Action> = keymap! {
            "q" => Action::Quit,
        };
        assert_eq!(km.lookup(&KeyCombo::key("q")), Some(&Action::Quit));
    }

    #[test]
    fn array_binding_same_action() {
        let km: KeyMap<Action> = keymap! {
            ["q", "esc"] => Action::Quit,
        };
        assert_eq!(km.lookup(&KeyCombo::key("q")), Some(&Action::Quit));
        assert_eq!(km.lookup(&KeyCombo::key("esc")), Some(&Action::Quit));
    }

    #[test]
    fn modifier_binding() {
        let km: KeyMap<Action> = keymap! {
            (ctrl + "s") => Action::Save,
        };
        let combo = KeyCombo::new("s", vec!["ctrl".into()]);
        assert_eq!(km.lookup(&combo), Some(&Action::Save));
    }

    #[test]
    fn multiple_modifiers() {
        let km: KeyMap<Action> = keymap! {
            (ctrl + shift + "p") => Action::Save,
        };
        let combo = KeyCombo::new("p", vec!["ctrl".into(), "shift".into()]);
        assert_eq!(km.lookup(&combo), Some(&Action::Save));
    }

    #[test]
    fn full_table() {
        let km: KeyMap<Action> = keymap! {
            ["q", "esc"]    => Action::Quit,
            ["j", "down"]   => Action::Next,
            ["k", "up"]     => Action::Prev,
            (ctrl + "s")    => Action::Save,
        };
        assert_eq!(km.len(), 7);
        assert_eq!(km.lookup(&KeyCombo::key("j")), Some(&Action::Next));
        assert_eq!(km.lookup(&KeyCombo::key("up")), Some(&Action::Prev));
    }

    #[test]
    fn trailing_comma_ok() {
        let km: KeyMap<Action> = keymap! {
            "q" => Action::Quit,
        };
        assert_eq!(km.len(), 1);
    }
}
