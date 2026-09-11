//! The one place that asks what this terminal can do. Everything above it reads the
//! answer and never asks which terminal it is in, so adding a terminal means checking
//! what the probe reports in it rather than touching the editor.
//!
//! What is asked, and how:
//!
//! - the keyboard: the kitty enhancement flags are pushed and queried back, which foot,
//!   Alacritty and kitty all answer yes to. Legacy exists only so that a terminal saying
//!   no still runs, with the collisions it brings — Ctrl+I arriving as Tab, Ctrl+Enter as
//!   Enter — documented rather than worked around.
//! - synchronized updates: terminals answering the kitty keyboard query are the known
//!   set that hold them. Others get ordinary updates with the native cursor kept hidden
//!   and a caret painted into the cells; in particular, this keeps VTE/GNOME Terminal
//!   from showing either cursor motion or a hide and show through every frame.
//! - the clipboard: OSC 52 is assumed on. There is no reliable query, and a terminal that
//!   ignores it loses nothing the machine's own clipboard, which `tui::clipboard` reads
//!   and writes beside it, does not still do.
//! - pictures: what protocol draws one, and how many pixels a cell is, which is what says
//!   how many rows a picture takes. Both are the picker's answer, found from the
//!   environment first and then by asking the terminal — kitty answers the kitty query,
//!   foot answers the sixel one, and a terminal that answers neither gets a mosaic of
//!   half-blocks, which needs no pixel size to draw.
//!
//! `MARKATUI_PICTURES=halfblocks` forces the mosaic, which is the one way to see the
//! fallback on a terminal that has a protocol of its own.

use crossterm::terminal;
use ratatui_image::picker::Picker;

/// The one way to overrule what the terminal says about pictures.
const OVERRIDE: &str = "MARKATUI_PICTURES";

/// Whether the terminal answers the kitty keyboard protocol, which is what tells Ctrl+I
/// from Tab and Ctrl+Enter from Enter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyboard {
    Kitty,
    Legacy,
}

#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    pub keyboard: Keyboard,
}

pub fn ask() -> Capabilities {
    let keyboard = match terminal::supports_keyboard_enhancement() {
        Ok(true) => Keyboard::Kitty,
        _ => Keyboard::Legacy,
    };
    Capabilities { keyboard }
}

/// Whether frames can be hidden from view until they are complete. There is no portable
/// query for mode 2026, so stay conservative: every terminal known here to answer the
/// kitty keyboard query also implements synchronized updates.
pub fn synchronized_updates(keyboard: Keyboard) -> bool {
    keyboard == Keyboard::Kitty
}

/// What pictures are drawn with. Asked separately from the rest because of when it has to
/// be asked: the query is answered on the screen the cursor is on, so it goes after the
/// alternate screen is entered and before the first key is read.
pub fn pictures() -> Picker {
    if std::env::var(OVERRIDE).is_ok_and(|forced| forced == "halfblocks") {
        return Picker::halfblocks();
    }
    // A terminal that will not say goes without a protocol, not without pictures: the
    // mosaic needs nothing from it.
    Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks())
}
