//! Adapter from `egaku::Theme` (RGBA float) to `crossterm::style::Color`.
//!
//! Egaku's [`Theme`](egaku::Theme) is GPU-shaped: every color is `[f32; 4]`
//! linear-RGBA. Terminals only understand 24-bit `Rgb { r, g, b }` (or
//! 16-color names if you go through the legacy ANSI palette). This module
//! is the only place those representations meet.

use crossterm::style::Color;
use egaku::Theme;

/// Convert an `[f32; 4]` color (alpha discarded) into a `crossterm` 24-bit
/// `Color::Rgb`. Floats are clamped to `[0.0, 1.0]` then rounded to the
/// nearest `u8` — terminals do not display alpha.
#[must_use]
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
pub fn rgba_to_color(rgba: [f32; 4]) -> Color {
    let to_u8 = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::Rgb {
        r: to_u8(rgba[0]),
        g: to_u8(rgba[1]),
        b: to_u8(rgba[2]),
    }
}

/// Bundle of crossterm colors derived from an [`egaku::Theme`]. All drawers
/// in [`crate::draw`] take a `&Palette`; constructing it once per frame is
/// cheap (it just unpacks 9 fields).
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub background: Color,
    pub foreground: Color,
    pub accent: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub selection: Color,
    pub muted: Color,
    pub border: Color,
}

impl Palette {
    /// Derive a `Palette` from an egaku [`Theme`]. Only the semantic alias
    /// fields are read — base16 slots remain available on the Theme itself
    /// for callers that want raw access.
    #[must_use]
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            background: rgba_to_color(theme.background),
            foreground: rgba_to_color(theme.foreground),
            accent: rgba_to_color(theme.accent),
            error: rgba_to_color(theme.error),
            warning: rgba_to_color(theme.warning),
            success: rgba_to_color(theme.success),
            selection: rgba_to_color(theme.selection),
            muted: rgba_to_color(theme.muted),
            border: rgba_to_color(theme.border),
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::from_theme(&Theme::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_to_color_basic() {
        let c = rgba_to_color([1.0, 0.5, 0.0, 1.0]);
        assert_eq!(
            c,
            Color::Rgb {
                r: 255,
                g: 128,
                b: 0,
            }
        );
    }

    #[test]
    fn rgba_clamped() {
        let c = rgba_to_color([2.0, -1.0, 0.5, 1.0]);
        assert_eq!(
            c,
            Color::Rgb {
                r: 255,
                g: 0,
                b: 128,
            }
        );
    }

    #[test]
    fn nord_palette_distinct_colors() {
        let p = Palette::default();
        // Foreground and background should differ
        assert_ne!(p.foreground, p.background);
        // Accent and error should differ
        assert_ne!(p.accent, p.error);
    }
}
