//! mojiban document rows onto the terminal.
//!
//! [`mojiban::render_markdown`] lays markdown out as rows of spans whose
//! style indexes a table of semantic [`Role`](mojiban::Role)s. This module is
//! the one mapping from those roles onto a [`Palette`], shared by every TTY
//! host: [`role_style`] for hosts that draw into a [`Buffer`](crate::Buffer),
//! [`write_ansi`] for hosts that print a document to a stream.

use std::io::Write;

use crossterm::QueueableCommand;
use crossterm::style::{Attribute, Print, SetAttribute};
use mojiban::{CellStyle, Rendered, Role, TextWeight};

use crate::cell::Style;
use crate::theme::{Palette, rgba_to_color};

/// The terminal style for one entry of a [`Rendered`] style table.
///
/// Structural roles take the palette's chrome colours (headings and markers
/// the accent, rules and frames the border, dimmed); code and inline roles
/// keep the colour mojiban resolved for them.
#[must_use]
pub fn role_style(cs: &CellStyle, palette: &Palette) -> Style {
    let fg = match cs.role {
        Role::Text | Role::TableHead => palette.foreground,
        Role::Heading(_) | Role::ListMarker => palette.accent,
        Role::HeadingRule | Role::QuoteBar | Role::CodeFrame | Role::TableRule | Role::Rule => {
            palette.border
        }
        Role::Quote | Role::CodeLabel => palette.muted,
        _ => rgba_to_color(cs.text.color),
    };
    let mut st = Style::default().fg(fg);
    if let Some(bg) = cs.background {
        st = st.bg(rgba_to_color(bg));
    }
    if cs.text.weight == TextWeight::Bold {
        st = st.bold();
    }
    if cs.text.italic {
        st = st.italic();
    }
    if cs.text.underline {
        st = st.underlined();
    }
    if matches!(
        cs.role,
        Role::HeadingRule | Role::CodeFrame | Role::TableRule | Role::Rule | Role::QuoteBar
    ) {
        st = st.dim();
    }
    st
}

/// The whole style table of `doc`, index-aligned with `doc.styles`.
#[must_use]
pub fn styles(doc: &Rendered, palette: &Palette) -> Vec<Style> {
    doc.styles
        .iter()
        .map(|cs| role_style(cs, palette))
        .collect()
}

/// Print `doc` to `out` as ANSI-styled lines, one row per line, attributes
/// reset at the end of every row so nothing bleeds into the next.
///
/// # Errors
/// Any write error from `out`.
pub fn write_ansi<W: Write>(out: &mut W, doc: &Rendered, palette: &Palette) -> std::io::Result<()> {
    let table = styles(doc, palette);
    for row in &doc.rows {
        for span in row {
            let style = table
                .get(usize::from(span.style()))
                .copied()
                .unwrap_or_default();
            crate::render::apply_style(out, style)?;
            out.queue(Print(span.text()))?;
        }
        out.queue(SetAttribute(Attribute::Reset))?;
        out.queue(Print("\n"))?;
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::Modifiers;

    fn doc() -> Rendered {
        mojiban::render_markdown(
            "# Title\n\nsome **bold** text that wraps at a narrow width\n\n- item",
            20,
            &mojiban::Theme::default(),
        )
    }

    #[test]
    fn headings_take_the_accent_and_rules_are_dimmed() {
        let p = Palette::default();
        let d = doc();
        for cs in &d.styles {
            let st = role_style(cs, &p);
            match cs.role {
                Role::Heading(_) | Role::ListMarker => assert_eq!(st.fg, p.accent),
                Role::HeadingRule | Role::Rule => {
                    assert_eq!(st.fg, p.border);
                    assert!(st.modifiers.contains(Modifiers::DIM));
                }
                Role::Text => assert_eq!(st.fg, p.foreground),
                _ => {}
            }
        }
        assert!(
            d.styles
                .iter()
                .any(|cs| matches!(cs.role, Role::Heading(_)))
        );
    }

    #[test]
    fn write_ansi_prints_every_row_and_resets_each_line() {
        let d = doc();
        let mut out = Vec::new();
        write_ansi(&mut out, &d, &Palette::default()).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s.matches('\n').count(), d.rows.len());
        assert!(s.contains("Title") && s.contains("bold"));
        for line in s.lines() {
            assert!(line.ends_with("\x1b[0m"), "row not reset: {line:?}");
        }
    }

    #[test]
    fn styles_is_index_aligned_with_the_table() {
        let d = doc();
        assert_eq!(styles(&d, &Palette::default()).len(), d.styles.len());
    }
}
