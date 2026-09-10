//! Taking the terminal over and giving it back: raw mode, a screen of our own, bracketed
//! paste, the mouse, the page colour in padding outside the cell grid, and the kitty
//! keyboard flags where the terminal answers for them.

use crate::tui::config;
use crate::tui::probe::{Capabilities, Keyboard};
use crossterm::event::{
    self, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, terminal};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::style::Color;
use std::io::{self, Stdout, Write};

/// Taking the mouse over: the buttons and the wheel, motion while a button is held, and
/// the SGR encoding, which is the one that can carry a column past the 223rd.
///
/// Written out here rather than asked for with crossterm's `EnableMouseCapture`, which
/// also turns on reporting the pointer moving with no button held. The editor does
/// nothing with that, and every twitch of it would wake the loop and cost a frame. It
/// cannot simply be turned back off afterwards either: a terminal that holds what it
/// reports as one setting rather than a flag apiece — foot does, and it is not alone —
/// reads "stop reporting motion" as "stop reporting the mouse", and the editor is left
/// with no mouse at all. So it is never asked for, and the last thing asked for is the
/// drag reporting that is wanted, which is what such a terminal is left holding.
const MOUSE_ON: &str = "\x1b[?1000h\x1b[?1002h\x1b[?1006h";

/// Raw mode, a screen of our own, bracketed paste, the mouse where the config leaves it
/// on, and the kitty keyboard flags where the terminal answers for them. The panic hook
/// puts every one of them back: flags left pushed after a crash leave the writer's shell
/// with odd keys.
pub(super) fn start(
    capabilities: Capabilities,
    background: Option<Color>,
) -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, event::EnableBracketedPaste)?;
    if config::get().mouse {
        io::stdout().write_all(MOUSE_ON.as_bytes())?;
        io::stdout().flush()?;
    }
    if capabilities.keyboard == Keyboard::Kitty {
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let _ = stop(capabilities, background);
        previous(panic);
    }));
    if let Some(command) = background_commands(background).0 {
        io::stdout().write_all(command.as_bytes())?;
    }
    Terminal::new(CrosstermBackend::new(io::stdout()))
}

pub(super) fn stop(capabilities: Capabilities, background: Option<Color>) -> io::Result<()> {
    if capabilities.keyboard == Keyboard::Kitty {
        execute!(io::stdout(), PopKeyboardEnhancementFlags)?;
    }
    if let Some(command) = background_commands(background).1 {
        io::stdout().write_all(command.as_bytes())?;
    }
    // Crossterm's own, on the way out: it turns off more than was asked for, which is the
    // safe direction to be wrong in, and the mouse is the terminal's again either way.
    if config::get().mouse {
        execute!(io::stdout(), event::DisableMouseCapture)?;
    }
    execute!(io::stdout(), event::DisableBracketedPaste, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()
}

/// OSC 11 changes the terminal's default background, including padding beyond its cell
/// grid. OSC 111 restores the configured colour when the editor gives the terminal back.
fn background_commands(background: Option<Color>) -> (Option<String>, Option<&'static str>) {
    match background {
        Some(Color::Rgb(red, green, blue)) => {
            (Some(format!("\x1b]11;#{red:02x}{green:02x}{blue:02x}\x1b\\")), Some("\x1b]111\x1b\\"))
        }
        _ => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn explicit_paper_colours_the_terminal_padding_and_is_reset_afterwards() {
        assert_eq!(
            background_commands(Some(Color::Rgb(0xfc, 0xfa, 0xf5))),
            (Some("\x1b]11;#fcfaf5\x1b\\".to_string()), Some("\x1b]111\x1b\\"))
        );
        assert_eq!(background_commands(None), (None, None));
    }
}
