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
//! - the clipboard: OSC 52 is assumed on. There is no reliable query, and a terminal that
//!   ignores it loses copy and nothing else.
//!
//! Images and the pixel size of a cell join this when the graphics work does.

use crossterm::terminal;

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
