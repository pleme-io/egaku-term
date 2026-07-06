//! The typed cell primitive — one attributed character of the framed board.
//!
//! A [`Cell`] is the atom of the [`Buffer`](crate::buffer::Buffer): a single
//! grapheme plus its foreground / background [`Color`] and a set of typed
//! [`Modifiers`]. This is the surface that makes terminal emission *typed* —
//! nothing in `egaku-term` hand-spells an escape or `format!()`s a styled
//! string; every visible glyph is a `Cell` value, and the renderer projects
//! cells to crossterm `Command`s (see [`crate::render`]).
//!
//! `Cell` carries the symbol as a `String` (a grapheme cluster) so combining
//! marks, emoji ZWJ sequences, and wide CJK glyphs are represented correctly.
//! A wide grapheme (display width 2) occupies one `Cell` holding the glyph and
//! a following continuation `Cell` whose symbol is empty; the renderer and the
//! diff both honour display width so the cursor advances correctly.

use crossterm::style::Color;

/// A typed set of terminal text attributes.
///
/// Backed by a `u8` bitset — one bit per attribute — so a [`Cell`]'s style is
/// `Copy` and comparisons in the diff are cheap. Compose with `|`:
///
/// ```
/// use egaku_term::Modifiers;
/// let m = Modifiers::BOLD | Modifiers::REVERSED;
/// assert!(m.contains(Modifiers::BOLD));
/// assert!(!m.contains(Modifiers::ITALIC));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(u8);

impl Modifiers {
    /// No attributes.
    pub const NONE: Self = Self(0);
    /// Bold / increased intensity.
    pub const BOLD: Self = Self(1 << 0);
    /// Dim / decreased intensity.
    pub const DIM: Self = Self(1 << 1);
    /// Italic.
    pub const ITALIC: Self = Self(1 << 2);
    /// Underlined.
    pub const UNDERLINED: Self = Self(1 << 3);
    /// Reverse video (swap fg/bg).
    pub const REVERSED: Self = Self(1 << 4);

    /// True when every attribute in `other` is present in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// True when no attributes are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The raw bits — mainly for tests and debugging.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// A typed style: foreground, background, and [`Modifiers`].
///
/// [`Color::Reset`] means "the terminal's default" — the renderer never emits
/// an explicit color for a `Reset` slot, so a default-styled cell inherits the
/// live terminal palette (honouring an OSC 10/11 bg/fg without a hardcoded
/// hex). Build with the chaining setters:
///
/// ```
/// use egaku_term::Style;
/// use egaku_term::crossterm::style::Color;
/// let s = Style::default().fg(Color::Red).bold();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    /// Foreground color. [`Color::Reset`] = terminal default.
    pub fg: Color,
    /// Background color. [`Color::Reset`] = terminal default.
    pub bg: Color,
    /// Text attributes.
    pub modifiers: Modifiers,
}

impl Default for Style {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Style {
    /// The default style: terminal-default fg/bg, no attributes.
    pub const DEFAULT: Self = Self {
        fg: Color::Reset,
        bg: Color::Reset,
        modifiers: Modifiers::NONE,
    };

    /// Set the foreground color.
    #[must_use]
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = color;
        self
    }

    /// Set the background color.
    #[must_use]
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = color;
        self
    }

    /// Replace the whole modifier set.
    #[must_use]
    pub const fn modifiers(mut self, modifiers: Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Add the [`Modifiers::BOLD`] attribute.
    #[must_use]
    pub fn bold(mut self) -> Self {
        self.modifiers |= Modifiers::BOLD;
        self
    }

    /// Add the [`Modifiers::DIM`] attribute.
    #[must_use]
    pub fn dim(mut self) -> Self {
        self.modifiers |= Modifiers::DIM;
        self
    }

    /// Add the [`Modifiers::ITALIC`] attribute.
    #[must_use]
    pub fn italic(mut self) -> Self {
        self.modifiers |= Modifiers::ITALIC;
        self
    }

    /// Add the [`Modifiers::UNDERLINED`] attribute.
    #[must_use]
    pub fn underlined(mut self) -> Self {
        self.modifiers |= Modifiers::UNDERLINED;
        self
    }

    /// Add the [`Modifiers::REVERSED`] attribute.
    #[must_use]
    pub fn reversed(mut self) -> Self {
        self.modifiers |= Modifiers::REVERSED;
        self
    }
}

/// One attributed cell of the framed board.
///
/// The `symbol` is a grapheme cluster; an empty symbol marks the continuation
/// slot after a wide (display-width-2) glyph. Two cells compare equal when
/// their symbol *and* style match — that equality is what the diff renderer
/// keys on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    symbol: String,
    /// Foreground color.
    pub fg: Color,
    /// Background color.
    pub bg: Color,
    /// Text attributes.
    pub modifiers: Modifiers,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            symbol: String::from(" "),
            fg: Color::Reset,
            bg: Color::Reset,
            modifiers: Modifiers::NONE,
        }
    }
}

impl Cell {
    /// A cell holding a single grapheme with the default style.
    #[must_use]
    pub fn new(symbol: &str) -> Self {
        let mut cell = Self::default();
        cell.set_symbol(symbol);
        cell
    }

    /// The grapheme this cell renders. An empty string is a continuation slot
    /// after a wide glyph.
    #[must_use]
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Overwrite the symbol, reusing the existing allocation.
    pub fn set_symbol(&mut self, symbol: &str) -> &mut Self {
        self.symbol.clear();
        self.symbol.push_str(symbol);
        self
    }

    /// Overwrite the symbol with a single `char`.
    pub fn set_char(&mut self, ch: char) -> &mut Self {
        self.symbol.clear();
        self.symbol.push(ch);
        self
    }

    /// Set the foreground color.
    pub fn set_fg(&mut self, color: Color) -> &mut Self {
        self.fg = color;
        self
    }

    /// Set the background color.
    pub fn set_bg(&mut self, color: Color) -> &mut Self {
        self.bg = color;
        self
    }

    /// Set the text attributes.
    pub fn set_modifiers(&mut self, modifiers: Modifiers) -> &mut Self {
        self.modifiers = modifiers;
        self
    }

    /// Apply a whole [`Style`] (fg + bg + modifiers) at once.
    pub fn set_style(&mut self, style: Style) -> &mut Self {
        self.fg = style.fg;
        self.bg = style.bg;
        self.modifiers = style.modifiers;
        self
    }

    /// The cell's style as a [`Style`] value.
    #[must_use]
    pub fn style(&self) -> Style {
        Style {
            fg: self.fg,
            bg: self.bg,
            modifiers: self.modifiers,
        }
    }

    /// Reset the cell to a blank default cell, reusing the allocation. Called
    /// once per cell per frame when the back buffer is cleared.
    pub fn reset(&mut self) {
        self.symbol.clear();
        self.symbol.push(' ');
        self.fg = Color::Reset;
        self.bg = Color::Reset;
        self.modifiers = Modifiers::NONE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_compose_and_contain() {
        let m = Modifiers::BOLD | Modifiers::REVERSED;
        assert!(m.contains(Modifiers::BOLD));
        assert!(m.contains(Modifiers::REVERSED));
        assert!(!m.contains(Modifiers::ITALIC));
        assert!(!m.is_empty());
        assert!(Modifiers::NONE.is_empty());
    }

    #[test]
    fn style_builder() {
        let s = Style::default()
            .fg(Color::Red)
            .bg(Color::Blue)
            .bold()
            .italic();
        assert_eq!(s.fg, Color::Red);
        assert_eq!(s.bg, Color::Blue);
        assert!(s.modifiers.contains(Modifiers::BOLD));
        assert!(s.modifiers.contains(Modifiers::ITALIC));
        assert!(!s.modifiers.contains(Modifiers::DIM));
    }

    #[test]
    fn cell_default_is_blank_space() {
        let c = Cell::default();
        assert_eq!(c.symbol(), " ");
        assert_eq!(c.fg, Color::Reset);
        assert_eq!(c.bg, Color::Reset);
        assert!(c.modifiers.is_empty());
    }

    #[test]
    fn cell_set_style_and_symbol() {
        let mut c = Cell::default();
        c.set_char('日')
            .set_style(Style::default().fg(Color::Green).reversed());
        assert_eq!(c.symbol(), "日");
        assert_eq!(c.fg, Color::Green);
        assert!(c.modifiers.contains(Modifiers::REVERSED));
    }

    #[test]
    fn cell_reset_restores_default() {
        let mut c = Cell::new("x");
        c.set_fg(Color::Red);
        c.reset();
        assert_eq!(c, Cell::default());
    }

    #[test]
    fn cell_equality_keys_on_symbol_and_style() {
        let a = Cell::new("a");
        let mut b = Cell::new("a");
        assert_eq!(a, b);
        b.set_fg(Color::Red);
        assert_ne!(a, b);
    }
}
