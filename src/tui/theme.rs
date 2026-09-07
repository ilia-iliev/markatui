//! The colours, and how a cell's style bits turn into one. The Qt front end painted a
//! warm paper background over the whole window; here the terminal's own ground shows
//! through instead, so the editor sits in the palette the writer already chose. What is
//! painted — the accent, the muted grey, the wash under a bad word — is picked to read on
//! a light terminal and a dark one alike.

use crate::layout::Cell;
use crate::style;
use ratatui::style::{Color, Modifier, Style};

/// The green of the Qt theme, which reads as ink on paper and as a highlight on black.
pub const ACCENT: Color = Color::Rgb(0x3E, 0x8E, 0x62);
/// Structure the writer is not reading: markers, bullets, rules, box drawing.
pub const MUTED: Color = Color::Rgb(0x8A, 0x83, 0x78);
/// Behind anything the checker took exception to, a misspelled word and a clumsy phrase
/// alike: wheat, with the ink forced dark so the words stay legible on it.
pub const LINT: Color = Color::Rgb(0xF3, 0xE4, 0xC3);
pub const LINT_INK: Color = Color::Rgb(0x2D, 0x2A, 0x26);
/// The ground a fenced block sits on, a shade off the terminal's own either way.
pub const CODE: Color = Color::Rgb(0x3A, 0x37, 0x33);
/// The foot of the screen: the checker's message, the search bar, the quit prompt.
pub const PROMPT: Color = Color::Rgb(0x26, 0x24, 0x1F);
pub const PROMPT_INK: Color = Color::Rgb(0xFF, 0xFF, 0xFF);

/// How wide the column of text is, in cells. Wider than this and a line of prose is
/// tiring to read back; the Qt front end held the same measure in pixels.
pub const CONTENT_WIDTH: u16 = 72;

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
    let mut style = Style::default();
    if bits & style::HEADING != 0 {
        style = style.fg(ACCENT).add_modifier(Modifier::BOLD);
    }
    if bits & style::MARKER != 0 {
        style = style.fg(MUTED);
    }
    if bits & style::LINK != 0 {
        style = style.fg(ACCENT).add_modifier(Modifier::UNDERLINED);
    }
    if bits & style::CODE != 0 {
        style = style.bg(CODE);
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
        style = style.bg(LINT).fg(LINT_INK);
    }
    style
}

/// The same cell inside a selection: ink and paper swapped, so a selection running over
/// several blocks reads as one shape.
pub fn selected(style: Style) -> Style {
    style.add_modifier(Modifier::REVERSED)
}

pub fn prompt() -> Style {
    Style::default().bg(PROMPT).fg(PROMPT_INK)
}

pub fn muted() -> Style {
    Style::default().fg(MUTED)
}
