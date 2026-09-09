//! Taking the terminal over and giving it back: raw mode, a screen of our own, bracketed
//! paste, and the kitty keyboard flags where the terminal answers for them.

use crate::tui::probe::{Capabilities, Keyboard};
use crossterm::event::{
    self, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, terminal};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Stdout};

/// Raw mode, a screen of our own, bracketed paste, and the kitty keyboard flags where the
/// terminal answers for them. The panic hook puts every one of them back: flags left
/// pushed after a crash leave the writer's shell with odd keys.
pub(super) fn start(capabilities: Capabilities) -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, event::EnableBracketedPaste)?;
    if capabilities.keyboard == Keyboard::Kitty {
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let _ = stop(capabilities);
        previous(panic);
    }));
    Terminal::new(CrosstermBackend::new(io::stdout()))
}

pub(super) fn stop(capabilities: Capabilities) -> io::Result<()> {
    if capabilities.keyboard == Keyboard::Kitty {
        execute!(io::stdout(), PopKeyboardEnhancementFlags)?;
    }
    execute!(io::stdout(), event::DisableBracketedPaste, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()
}
