//! The clipboard, which is two of them: the machine's own, where the editor is running on
//! one with a display server, and OSC 52, which reaches the terminal on the other end of
//! an ssh connection. Copy goes to whichever of the two is there, the machine's own
//! first. Paste reads the machine's own, and falls back on what was last copied here —
//! OSC 52 cannot be read back, so over ssh the editor's own copies are the ones its paste
//! can reach. A picture is the machine's own clipboard alone: there is no OSC 52 for one,
//! and nothing here keeps a copy of one.

use crossterm::clipboard::CopyToClipboard;
use crossterm::execute;
use image::ExtendedColorType;
use image::ImageEncoder;
use image::codecs::png::PngEncoder;
use std::io;

/// What the clipboard has to give a paste.
pub enum Paste {
    Nothing,
    Words(String),
    Picture(Vec<u8>),
}

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

    /// Copy, to the one clipboard that can be reached. The machine's own where the editor
    /// runs beside a display server; OSC 52 where it does not, which is the ssh case.
    /// Both at once is one copy too many: on X11 the terminal answers the OSC 52 by
    /// taking the same selection we have just taken, sees us holding it when it checks,
    /// and calls that a failure. What is copied outlives the editor either way: over ssh
    /// the terminal goes on holding it, and here it is handed to the clipboard manager on
    /// the way out.
    pub fn copy(&mut self, text: String) {
        let copied = self.system.as_mut().is_some_and(|system| system.set_text(&text).is_ok());
        if !copied {
            let _ = execute!(io::stdout(), CopyToClipboard::to_clipboard_from(text.clone()));
        }
        self.own = text;
    }

    /// Whatever is on the clipboard, the picture first: a writer who copied a picture
    /// meant the picture, and what a clipboard offering both puts beside it is the page
    /// it was copied off, not something to write down.
    pub fn content(&mut self) -> Paste {
        if let Some(png) = self.picture() {
            return Paste::Picture(png);
        }
        match self.words() {
            words if words.is_empty() => Paste::Nothing,
            words => Paste::Words(words),
        }
    }

    /// The words on the clipboard. Empty where nothing has been copied anywhere, which
    /// is a paste that does nothing.
    fn words(&mut self) -> String {
        let system = self.system.as_mut().and_then(|system| system.get_text().ok());
        // An empty system clipboard is not an answer worth taking over our own text: it
        // is also what a terminal without one hands back.
        system.filter(|text| !text.is_empty()).unwrap_or_else(|| self.own.clone())
    }

    /// The picture on the clipboard, as a PNG. Nothing where the clipboard holds words
    /// rather than a picture, and nothing at all over ssh, where there is no clipboard of
    /// the machine's to read. The picture is handed over as raw pixels whatever it
    /// arrived as, so a file to put beside the document has to be encoded here.
    fn picture(&mut self) -> Option<Vec<u8>> {
        let picture = self.system.as_mut()?.get_image().ok()?;
        let (width, height) = (picture.width as u32, picture.height as u32);
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&picture.bytes, width, height, ExtendedColorType::Rgba8)
            .ok()?;
        Some(png)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::fd::AsRawFd;

    /// What a copy wrote to the terminal. The escape goes to standard output and nowhere
    /// else, so that is taken away for the length of the copy and read back afterwards.
    fn sent(copy: impl FnOnce()) -> String {
        let path = std::env::temp_dir().join(format!("markatui-copy-{}", std::process::id()));
        let file = std::fs::File::create(&path).expect("a file to catch standard output in");
        // SAFETY: standard output is put back from the duplicate taken of it here, before
        // the file it was pointed at is closed.
        let text = unsafe {
            let stdout = libc::dup(1);
            libc::dup2(file.as_raw_fd(), 1);
            copy();
            io::stdout().flush().ok();
            libc::dup2(stdout, 1);
            libc::close(stdout);
            std::fs::read_to_string(&path).expect("what the copy wrote")
        };
        std::fs::remove_file(&path).ok();
        text
    }

    /// One copy, never two. Where the machine has a clipboard the copy is already on it
    /// and the escape is not sent: a terminal that took it would go for the very
    /// selection this process has just taken, and would be told it had lost it. Where
    /// there is none — over ssh — the escape is all there is, and it goes.
    #[test]
    fn sends_the_escape_only_where_the_machine_has_no_clipboard() {
        let mut clipboard = Clipboard::open();
        let machine = clipboard.system.is_some();
        // Whoever is running the tests was using this clipboard before they started.
        let held = clipboard.words();

        let written = sent(|| clipboard.copy("copied".into()));

        assert_eq!(written.contains("\x1b]52;"), !machine, "{written:?}");
        clipboard.copy(held);
    }
}
