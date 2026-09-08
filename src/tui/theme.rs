//! The colours, and how a cell's style bits turn into one. The Qt front end painted a
//! warm paper background over the whole window; here the terminal's own ground shows
//! through instead, so the editor sits in the palette the writer already chose. What is
//! painted — the accent, the muted grey, the wash under a bad word — is picked to read on
//! a light terminal and a dark one alike.
//!
//! Every colour here is a default the config file can overrule, and
//! `inherit_background = false` is what puts the paper back.

use crate::layout::Cell;
use crate::style;
use crate::tui::config;
use ratatui::style::{Color, Modifier, Style};

/// The palette, written once: each colour's field, its default, and the name the config's
/// `[palette]` table calls it by all come off the same line, so that adding a colour is
/// the single edit this file has always claimed it was.
macro_rules! palette {
    ($($(#[$note:meta])* $name:ident = $red:literal, $green:literal, $blue:literal;)*) => {
        /// The colours the editor paints with. The names are the ones the config's
        /// `[palette]` table uses, and the defaults are the theme the Qt front end had.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Palette {
            $($(#[$note])* pub $name: Color,)*
        }

        impl Default for Palette {
            fn default() -> Self {
                Palette { $($name: Color::Rgb($red, $green, $blue),)* }
            }
        }

        impl Palette {
            /// The colour the config calls `name`.
            pub fn slot(&mut self, name: &str) -> Option<&mut Color> {
                Some(match name {
                    $(stringify!($name) => &mut self.$name,)*
                    _ => return None,
                })
            }
        }
    };
}

palette! {
    /// The green of the Qt theme, which reads as ink on paper and as a highlight on black.
    accent = 0x3E, 0x8E, 0x62;
    /// Structure the writer is not reading: markers, bullets, rules, box drawing.
    muted = 0x8A, 0x83, 0x78;
    /// Behind anything the checker took exception to, a misspelled word and a clumsy
    /// phrase alike: wheat, with the ink forced dark so the words stay legible on it.
    lint = 0xF3, 0xE4, 0xC3;
    lint_ink = 0x2D, 0x2A, 0x26;
    /// The ground a fenced block sits on, a shade off the terminal's own either way.
    code = 0x3A, 0x37, 0x33;
    /// The foot of the screen: the checker's message, the search bar, the quit prompt.
    prompt = 0x26, 0x24, 0x1F;
    prompt_ink = 0xFF, 0xFF, 0xFF;
    /// The ground and the ink of the whole screen, painted only where the config has
    /// asked for them with `inherit_background = false`.
    paper = 0xFA, 0xF6, 0xEC;
    ink = 0x2D, 0x2A, 0x26;
}

fn palette() -> Palette {
    config::get().palette
}

/// How wide the column of text is, in cells. Wider than this and a line of prose is
/// tiring to read back; the Qt front end held the same measure in pixels.
pub fn content_width() -> u16 {
    config::get().content_width
}

/// What the screen is painted on where nothing else has claimed it: the terminal's own
/// ground, unless the config asked for paper of ours.
pub fn base() -> Style {
    let palette = palette();
    match config::get().inherit_background {
        true => Style::default(),
        false => Style::default().bg(palette.paper).fg(palette.ink),
    }
}

/// How one cell of a laid-out block is drawn. `row` is what the whole row carries.
pub fn of(cell: &Cell, row: u16) -> Style {
    bits(cell.bits | row)
}

/// What a row is drawn on where it has no cell of its own: the ground a fenced block
/// sits on runs to the edge of the column, not to the end of the shortest line in it.
pub fn ground(row: u16) -> Style {
    bits(row)
}

fn bits(bits: u16) -> Style {
    let palette = palette();
    let mut style = base();
    if bits & style::HEADING != 0 {
        style = style.fg(palette.accent).add_modifier(Modifier::BOLD);
    }
    if bits & style::MARKER != 0 {
        style = style.fg(palette.muted);
    }
    if bits & style::LINK != 0 {
        style = style.fg(palette.accent).add_modifier(Modifier::UNDERLINED);
    }
    if bits & style::CODE != 0 {
        style = style.bg(palette.code);
    }
    if bits & style::BOLD != 0 {
        style = style.add_modifier(Modifier::BOLD);
    }
    if bits & style::ITALIC != 0 {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if bits & style::STRIKE != 0 {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    // The wash goes on last: it has to be the ground whatever else the words are.
    if bits & style::LINT != 0 {
        style = style.bg(palette.lint).fg(palette.lint_ink);
    }
    style
}

/// The same cell inside a selection: ink and paper swapped, so a selection running over
/// several blocks reads as one shape.
pub fn selected(style: Style) -> Style {
    style.add_modifier(Modifier::REVERSED)
}

pub fn prompt() -> Style {
    let palette = palette();
    Style::default().bg(palette.prompt).fg(palette.prompt_ink)
}
