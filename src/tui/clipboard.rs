//! The clipboard, which is two of them: the machine's own, where the editor is running on
//! one with a display server, and OSC 52, which reaches the terminal on the other end of
//! an ssh connection. Copy goes to both. Paste reads the machine's own, and falls back on
//! what was last copied here — OSC 52 cannot be read back, so over ssh the editor's own
//! copies are the ones its paste can reach.

use crossterm::clipboard::CopyToClipboard;
use crossterm::execute;
use std::io;

pub struct Clipboard {
    /// Held open for as long as the editor runs rather than made per copy: on X11 and
    /// Wayland alike the process that copied has to stay alive to hand the text over when
    /// it is asked for.
    /// Nothing where there is no display server to reach, which is the ssh case.
    system: Option<arboard::Clipboard>,
    /// The last thing copied here, which is what paste falls back on.
    own: String,
}

impl Clipboard {
    pub fn open() -> Self {
        Clipboard { system: arboard::Clipboard::new().ok(), own: String::new() }
    }

    /// Copy, to every clipboard there is. OSC 52 goes last on purpose: a terminal that
    /// takes it owns the clipboard afterwards, and its ownership outlives the editor,
    /// where ours ends with the process. A terminal with OSC 52 turned off ignores it
    /// silently and leaves the copy above standing, which is the whole arrangement — one
    /// of the two always works, and there is nothing to detect.
    pub fn copy(&mut self, text: String) {
        if let Some(system) = &mut self.system {
            let _ = system.set_text(&text);
        }
        let _ = execute!(io::stdout(), CopyToClipboard::to_clipboard_from(text.clone()));
        self.own = text;
    }

    /// What a paste puts in. Empty where nothing has been copied anywhere, which is a
    /// paste that does nothing.
    pub fn paste(&mut self) -> String {
        let system = self.system.as_mut().and_then(|system| system.get_text().ok());
        // An empty system clipboard is not an answer worth taking over our own text: it
        // is also what a terminal without one hands back.
        system.filter(|text| !text.is_empty()).unwrap_or_else(|| self.own.clone())
    }
}
