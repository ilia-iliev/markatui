//! The colours, and how a cell's style bits turn into one. Light and dark paint the
//! whole screen; terminal leaves its ground and ink alone. Every preset can be adjusted
//! in the config file's `[palette]` table.

use crate::layout::Cell;
use crate::style;
use crate::tui::config;
use ratatui::style::{Color, Modifier, Style};

/// The palette, written once: each colour's field, its default, and the name the config's
/// `[palette]` table calls it by all come off the same line, so that adding a colour is
/// the single edit this file has always claimed it was.
macro_rules! palette {
    ($($(#[$note:meta])* $name:ident = $hex:literal;)*) => {
        /// The colours the editor paints with. The names are the ones the config's
        /// `[palette]` table uses, and the defaults are the `terminal` preset.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Palette {
            $($(#[$note])* pub $name: Color,)*
        }

        impl Default for Palette {
            fn default() -> Self {
                Palette { $($name: hex($hex),)* }
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

/// A colour written the way the config file and the README write it, `0xRRGGBB`.
const fn hex(value: u32) -> Color {
    Color::Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

palette! {
    /// Headings: a burnt orange out of the paper's own family, dark enough to read as ink
    /// on a light ground and warm enough to lift off a dark one.
    accent = 0xC2622F;
    /// Links, which want the colour a reader already expects of them rather than the one
    /// the headings wear.
    link = 0x5578B8;
    /// Structure the writer is not reading: markers, bullets, rules, box drawing.
    muted = 0x8A8378;
    /// Behind anything the checker took exception to, a misspelled word and a clumsy
    /// phrase alike: wheat, with the ink forced dark so the words stay legible on it.
    lint = 0xF3E4C3;
    lint_ink = 0x2D2A26;
    /// The ground a fenced block sits on, a shade off the terminal's own either way.
    code = 0x3A3733;
    /// The foot of the screen: the checker's message, the search bar, the quit prompt.
    prompt = 0x26241F;
    prompt_ink = 0xFFFFFF;
    /// The ground and the ink of the whole screen, painted only where the config has
    /// asked for them with `inherit_background = false`.
    paper = 0xFAF6EC;
    ink = 0x2D2A26;
}

/// A complete, sensible starting palette. `Terminal` keeps the user's terminal ground
/// and ink; the other two paint both explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
    Terminal,
}

impl Theme {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "terminal" => Some(Self::Terminal),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Terminal => "terminal",
        }
    }

    pub fn settings(self) -> (bool, Palette) {
        match self {
            Self::Light => (
                false,
                Palette {
                    accent: hex(0xA4522A),
                    link: hex(0x2F4C7A),
                    muted: hex(0x857B6E),
                    lint: hex(0xF7D8A8),
                    lint_ink: hex(0x26231F),
                    code: hex(0xE9E6DE),
                    prompt: hex(0x2B2822),
                    prompt_ink: hex(0xF7F3EA),
                    paper: hex(0xFCFAF5),
                    ink: hex(0x26231F),
                },
            ),
            Self::Dark => (
                false,
                Palette {
                    accent: hex(0xE08A5A),
                    link: hex(0x8FAFE0),
                    muted: hex(0x9B948A),
                    lint: hex(0x6B4F1D),
                    lint_ink: hex(0xFFF4D6),
                    code: hex(0x2C2926),
                    prompt: hex(0x181715),
                    prompt_ink: hex(0xF5F1E8),
                    paper: hex(0x211F1C),
                    ink: hex(0xE8E2D8),
                },
            ),
            Self::Terminal => (true, Palette::default()),
        }
    }
}

/// How wide the column of text is, in cells. Wider than this and a line of prose is
/// tiring to read back; the Qt front end held the same measure in pixels.
pub fn content_width() -> u16 {
    content_width_for(config::get())
}

pub(crate) fn content_width_for(config: &config::Config) -> u16 {
    config.content_width
}

/// What the screen is painted on where nothing else has claimed it: the terminal's own
/// ground, unless the config asked for paper of ours.
pub fn base() -> Style {
    base_for(config::get())
}

pub(crate) fn base_for(config: &config::Config) -> Style {
    match config.inherit_background {
        true => Style::default(),
        false => Style::default().bg(config.palette.paper).fg(config.palette.ink),
    }
}

/// How one cell of a laid-out block is drawn. `row` is what the whole row carries.
pub fn of(cell: &Cell, row: u16) -> Style {
    of_for(config::get(), cell, row)
}

pub(crate) fn of_for(config: &config::Config, cell: &Cell, row: u16) -> Style {
    bits_for(config, cell.bits | row)
}

/// What a row is drawn on where it has no cell of its own: the ground a fenced block
/// sits on runs to the edge of the column, not to the end of the shortest line in it.
pub fn ground(row: u16) -> Style {
    bits_for(config::get(), row)
}

fn bits_for(config: &config::Config, bits: u16) -> Style {
    let palette = config.palette;
    let mut style = base_for(config);
    if bits & style::HEADING != 0 {
        style = style.fg(palette.accent).add_modifier(Modifier::BOLD);
    }
    if bits & style::MARKER != 0 {
        style = style.fg(palette.muted);
    }
    if bits & style::LINK != 0 {
        style = style.fg(palette.link).add_modifier(Modifier::UNDERLINED);
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
    prompt_for(config::get())
}

pub(crate) fn prompt_for(config: &config::Config) -> Style {
    Style::default().bg(config.palette.prompt).fg(config.palette.prompt_ink)
}
