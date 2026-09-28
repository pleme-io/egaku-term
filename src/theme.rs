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

/// Parse `#RRGGBB` into a 24-bit colour. `None` on anything else.
#[must_use]
pub fn hex_color(hex: &str) -> Option<Color> {
    egaku::theme::hex_to_rgba(hex).map(rgba_to_color)
}

/// Mix two colours: `t = 0` is `a`, `t = 1` is `b`, in between is a
/// per-channel linear blend. The fade primitive: a toast dimming into the
/// ground, a highlight easing in. A non-RGB colour (a named ANSI slot)
/// cannot be interpolated, so it snaps at the midpoint rather than invent
/// a value.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn blend(a: Color, b: Color, t: f32) -> Color {
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    match (a, b) {
        (
            Color::Rgb {
                r: ar,
                g: ag,
                b: ab,
            },
            Color::Rgb {
                r: br,
                g: bg,
                b: bb,
            },
        ) => {
            let mix =
                |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
            Color::Rgb {
                r: mix(ar, br),
                g: mix(ag, bg),
                b: mix(ab, bb),
            }
        }
        _ if t < 0.5 => a,
        _ => b,
    }
}

/// How many colours the terminal can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    /// 24-bit `38;2;r;g;b`.
    TrueColor,
    /// The xterm 256-colour palette `38;5;n`.
    Ansi256,
}

impl ColorDepth {
    /// Decide from `COLORTERM` and `TERM`: `truecolor`/`24bit` in
    /// `COLORTERM`, or a `TERM` known to carry it, is true colour;
    /// everything else gets the 256 palette, which every modern terminal
    /// renders. Pure, so it is testable without touching the environment.
    #[must_use]
    pub fn detect_from(colorterm: Option<&str>, term: Option<&str>) -> Self {
        let ct = colorterm.unwrap_or("").to_ascii_lowercase();
        let tm = term.unwrap_or("").to_ascii_lowercase();
        let declared = ct.contains("truecolor")
            || ct.contains("24bit")
            || tm.contains("direct")
            || tm.contains("truecolor");
        let known = tm == "xterm-kitty"
            || tm.starts_with("wezterm")
            || tm.starts_with("alacritty")
            || tm.contains("ghostty");
        if declared || known {
            Self::TrueColor
        } else {
            Self::Ansi256
        }
    }

    /// [`ColorDepth::detect_from`] over this process's environment.
    #[must_use]
    pub fn detect() -> Self {
        Self::detect_from(
            std::env::var("COLORTERM").ok().as_deref(),
            std::env::var("TERM").ok().as_deref(),
        )
    }

    /// Project one colour onto this depth.
    #[must_use]
    pub fn project(self, c: Color) -> Color {
        match (self, c) {
            (Self::Ansi256, Color::Rgb { r, g, b }) => Color::AnsiValue(rgb_to_ansi256(r, g, b)),
            _ => c,
        }
    }
}

/// Nearest xterm-256 index for an RGB triple: the better of the 6x6x6
/// cube and the 24-step grey ramp, by squared distance.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let near = |v: u8| -> usize {
        STEPS
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| (i32::from(**s) - i32::from(v)).abs())
            .map_or(0, |(i, _)| i)
    };
    let dist = |a: (u8, u8, u8)| -> i32 {
        let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).pow(2);
        d(a.0, r) + d(a.1, g) + d(a.2, b)
    };
    let (ri, gi, bi) = (near(r), near(g), near(b));
    let cube = (STEPS[ri], STEPS[gi], STEPS[bi]);
    let cube_idx = 16 + 36 * ri + 6 * gi + bi;
    let avg = (u32::from(r) + u32::from(g) + u32::from(b)) / 3;
    let gi_ = (avg.saturating_sub(8) / 10).min(23) as u8;
    let grey = 8 + 10 * gi_;
    if dist((grey, grey, grey)) < dist(cube) {
        232 + gi_
    } else {
        cube_idx as u8
    }
}

impl Palette {
    /// Project a fleet [`ResolvedTheme`](ishou_tokens::ResolvedTheme) onto
    /// the drawers' palette. Semantic slots come from the theme's xterm
    /// ANSI order (1/3/2 red/yellow/green, 12 bright blue for the accent, 8
    /// bright black for muted); the border sits a step above the ground,
    /// derived rather than chosen. A slot that fails to parse falls back to
    /// egaku's Nord value for that slot, never to black.
    #[must_use]
    pub fn from_resolved(r: &ishou_tokens::ResolvedTheme) -> Self {
        let floor = Self::default();
        let ansi = |i: usize, f: Color| r.ansi_16.get(i).and_then(|h| hex_color(h)).unwrap_or(f);
        let background = hex_color(&r.background).unwrap_or(floor.background);
        let foreground = hex_color(&r.foreground).unwrap_or(floor.foreground);
        Self {
            background,
            foreground,
            accent: ansi(12, floor.accent),
            error: ansi(1, floor.error),
            warning: ansi(3, floor.warning),
            success: ansi(2, floor.success),
            selection: hex_color(&r.selection_background).unwrap_or(floor.selection),
            muted: ansi(8, floor.muted),
            border: blend(background, foreground, 0.12),
        }
    }

    /// The palette for one fleet theme (`pleme_dark`, `vellum`, …).
    #[must_use]
    pub fn from_fleet(theme: ishou_tokens::FleetTheme) -> Self {
        Self::from_resolved(&theme.resolve())
    }

    /// Every slot projected onto `depth` — build once, after choosing the
    /// theme, so a 256-colour terminal gets the nearest palette entries
    /// instead of raw 24-bit escapes it would misrender.
    #[must_use]
    pub fn for_depth(self, depth: ColorDepth) -> Self {
        let p = |c| depth.project(c);
        Self {
            background: p(self.background),
            foreground: p(self.foreground),
            accent: p(self.accent),
            error: p(self.error),
            warning: p(self.warning),
            success: p(self.success),
            selection: p(self.selection),
            muted: p(self.muted),
            border: p(self.border),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fleet_nord_agrees_with_egakus_nord() {
        let f = Palette::from_fleet(ishou_tokens::FleetTheme::PlemeDark);
        let e = Palette::default();
        assert_eq!(f.background, e.background);
        // Foreground differs on purpose: the fleet resolves text to Snow
        // Storm 2 (#ECEFF4), egaku's base05 is Snow Storm 0 (#E5E9F0).
        assert_eq!(f.error, e.error);
        assert_eq!(f.success, e.success);
        assert_eq!(f.warning, e.warning);
    }

    #[test]
    fn every_fleet_theme_resolves_distinct_slots() {
        for t in ishou_tokens::FleetTheme::all() {
            let p = Palette::from_fleet(*t);
            assert_ne!(p.background, p.foreground, "{t:?}");
            assert_ne!(p.border, p.background, "{t:?}: the border must be visible");
        }
    }

    #[test]
    fn blend_endpoints_and_midpoint() {
        let a = Color::Rgb { r: 0, g: 0, b: 0 };
        let b = Color::Rgb {
            r: 200,
            g: 100,
            b: 50,
        };
        assert_eq!(blend(a, b, 0.0), a);
        assert_eq!(blend(a, b, 1.0), b);
        assert_eq!(
            blend(a, b, 0.5),
            Color::Rgb {
                r: 100,
                g: 50,
                b: 25
            }
        );
        assert_eq!(blend(a, b, f32::NAN), a, "NaN is the start, never garbage");
        assert_eq!(blend(Color::Red, b, 0.4), Color::Red);
        assert_eq!(blend(Color::Red, b, 0.6), b);
    }

    #[test]
    fn depth_detection() {
        assert_eq!(
            ColorDepth::detect_from(Some("truecolor"), Some("xterm-256color")),
            ColorDepth::TrueColor
        );
        assert_eq!(
            ColorDepth::detect_from(None, Some("xterm-kitty")),
            ColorDepth::TrueColor
        );
        assert_eq!(
            ColorDepth::detect_from(None, Some("screen-256color")),
            ColorDepth::Ansi256
        );
        assert_eq!(ColorDepth::detect_from(None, None), ColorDepth::Ansi256);
    }

    #[test]
    fn ansi256_picks_cube_and_grey() {
        assert_eq!(rgb_to_ansi256(255, 0, 0), 196);
        assert_eq!(rgb_to_ansi256(0, 0, 0), 16);
        assert_eq!(rgb_to_ansi256(128, 128, 128), 244);
        let p =
            Palette::from_fleet(ishou_tokens::FleetTheme::PlemeDark).for_depth(ColorDepth::Ansi256);
        assert!(matches!(p.accent, Color::AnsiValue(_)));
        let t = Palette::default().for_depth(ColorDepth::TrueColor);
        assert_eq!(t.accent, Palette::default().accent);
    }

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
