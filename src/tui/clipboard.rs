//! The clipboard, which is two of them: the machine's own, where the editor is running on
//! one with a display server, and OSC 52, which reaches the terminal on the other end of
//! an ssh connection. Copy goes to both. Paste reads the machine's own, and falls back on
//! what was last copied here — OSC 52 cannot be read back, so over ssh the editor's own
//! copies are the ones its paste can reach. A picture is the machine's own clipboard
//! alone: there is no OSC 52 for one, and nothing here keeps a copy of one.

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
