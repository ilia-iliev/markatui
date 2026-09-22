//! The colours, and how a cell's style bits turn into one. Light and dark paint the
//! whole screen; terminal leaves its ground and ink alone. Every preset can be adjusted
//! in the config file's `[palette]` table, and the foot of the screen has [`Footer`]
//! modes of its own.

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
    /// The band at the foot of the screen: the checker's message, the search bar, the
    /// quit prompt. Painted only where the footer is drawn as a band of its own.
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
                    prompt: hex(0xE4DFD3),
                    prompt_ink: hex(0x26231F),
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

/// How the foot of the screen is coloured. A band is a plate of its own in the palette's
/// `prompt` colours; the other two take the page's own ground and ink, either way round,
/// which is the only thing that stays right whatever the terminal is painted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Footer {
    #[default]
    Band,
    Invert,
    Paper,
}

impl Footer {
    /// The modes, in the words the config file and the error message use.
    pub const CHOICES: &'static str = "band, invert, or paper";

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "band" => Some(Self::Band),
            "invert" => Some(Self::Invert),
            "paper" => Some(Self::Paper),
            _ => None,
        }
    }
}

/// How wide the column of text is, in cells. Wider than this and a line of prose is
/// tiring to read back; the Qt front end held the same measure in pixels.
pub fn content_width() -> u16 {
    config::get().content_width
}

/// How much of a screen `area_width` cells wide the column takes: the measure above, or
/// the whole of a screen with no room for it.
pub fn column_width(area_width: u16) -> u16 {
    content_width().min(area_width)
}

/// What the screen is painted on where nothing else has claimed it: the terminal's own
/// ground, unless the config asked for paper of ours.
pub fn base() -> Style {
    base_for(config::get())
}

/// The page colour the terminal itself should wear while an explicit theme is open.
/// This reaches pixels outside its cell grid, which drawing the frame cannot.
pub fn terminal_background() -> Option<Color> {
    let config = config::get();
    (!config.inherit_background).then_some(config.palette.paper)
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

/// How far each level below the first carries a heading along the line from the accent
/// to the muted grey, in hundredths of it. The five steps run well past the grey: the
/// deepest headings come out on the cool side of it, which is what keeps one level
/// clearly apart from the next — the accent and the grey by themselves sit too close
/// together to be cut into six.
const FADE: i32 = 28;

/// A heading's colour: the accent at the top level, and a step further down the line at
/// every level under it, so that the hashes count down in colour as they do in size. The
/// line holds its lightness from end to end, so the deepest heading is as legible on the
/// page as the first.
fn heading(palette: &Palette, depth: usize) -> Color {
    let (Color::Rgb(red, green, blue), Color::Rgb(to_red, to_green, to_blue)) =
        (palette.accent, palette.muted)
    else {
        return palette.accent;
    };
    let part = FADE * depth.saturating_sub(1).min(5) as i32;
    let channel = |from: u8, to: u8| {
        let from = i32::from(from);
        (from + (i32::from(to) - from) * part / 100).clamp(0, 255) as u8
    };
    Color::Rgb(channel(red, to_red), channel(green, to_green), channel(blue, to_blue))
}

fn bits_for(config: &config::Config, bits: u16) -> Style {
    let palette = config.palette;
    let mut style = base_for(config);
    if bits & style::HEADING != 0 {
        style = style.fg(heading(&palette, style::depth(bits))).add_modifier(Modifier::BOLD);
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
    if bits & style::UNDERLINE != 0 {
        style = style.add_modifier(Modifier::UNDERLINED);
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
    match config.footer {
        Footer::Band => Style::default().bg(config.palette.prompt).fg(config.palette.prompt_ink),
        // Swapping what the page is already painted in is the one plate that cannot come
        // out wrong: it is the terminal's own two colours where those are what is showing.
        Footer::Invert => base_for(config).add_modifier(Modifier::REVERSED),
        Footer::Paper => base_for(config),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How far apart two colours are, summed over the channels.
    fn gap(one: Color, other: Color) -> u32 {
        let (Color::Rgb(red, green, blue), Color::Rgb(to_red, to_green, to_blue)) = (one, other)
        else {
            return 0;
        };
        u32::from(red.abs_diff(to_red))
            + u32::from(green.abs_diff(to_green))
            + u32::from(blue.abs_diff(to_blue))
    }

    /// Every level is a step further from the top heading's colour than the one above it,
    /// by a margin wide enough to see, and none of them lands on the marker's grey.
    #[test]
    fn fades_a_heading_one_visible_step_at_each_level() {
        for theme in [Theme::Light, Theme::Dark, Theme::Terminal] {
            let (_, palette) = theme.settings();
            let colours: Vec<Color> = (1..=6).map(|level| heading(&palette, level)).collect();
            assert_eq!(colours[0], palette.accent);
            assert!(colours.iter().all(|colour| *colour != palette.muted));
            let steps: Vec<u32> = colours.windows(2).map(|pair| gap(pair[0], pair[1])).collect();
            assert!(steps.iter().all(|step| *step >= 30), "{steps:?}");
        }
    }

    /// The level reaches the colour off the cell's own bits, so a `###` line is drawn
    /// fainter than a `#` one without anything else being asked.
    #[test]
    fn draws_a_deeper_heading_in_the_fainter_colour() {
        let config = config::Config::default();
        let of = |level| {
            bits_for(&config, style::HEADING | style::depth_bits(level)).fg.expect("a colour")
        };
        assert_eq!(of(1), config.palette.accent);
        assert_ne!(of(3), of(1));
        assert_ne!(of(6), of(3));
    }
}
