//! Pictures. Everything about drawing one is here: how many rows it takes, the thread
//! that reads it, and the slices of it that fall inside the screen. Which protocol the
//! terminal draws with — kitty, sixel, or a mosaic of half-blocks — is the probe's answer
//! and is carried in the picker; nothing here names a terminal.
//!
//! A picture is asked for by the block that holds it and answered on a later frame: the
//! file is read, scaled and encoded off the event loop, and until it comes back the block
//! says what it is of and where it is kept. A file that is not there says the same thing
//! for good.

use crate::tui::view::{self, Document};
use image::DynamicImage;
use ratatui::Frame;
use ratatui::buffer::CellDiffOption;
use ratatui::layout::{Rect, Size};
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::sliced::{SignedPosition, SlicedImage, SlicedProtocol};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::SystemTime;

/// The pictures of one document: those on the screen, those still being read, and those
/// that turned out not to be there.
pub struct Gallery {
    /// What this terminal draws a picture with, or nothing at all where pictures are not
    /// drawn: a screen test, which has no terminal to ask.
    picker: Option<Picker>,
    /// Where a relative path is taken from: the directory the document is in.
    root: PathBuf,
    pictures: HashMap<PathBuf, Picture>,
    /// What the frame being built has asked for. Anything else is let go of at the next
    /// settle, so a picture whose block has been typed over does not stay in memory.
    wanted: HashSet<PathBuf>,
    /// Where the pictures went on the last frame: the block each belongs to, the row it
    /// started on, and how big it was. A frame that has any of them elsewhere is sent whole.
    placed: Vec<(usize, i16, Size)>,
    finished: Receiver<Loaded>,
    back: Sender<Loaded>,
}

/// One picture, and what has become of it. Each carries what it was asked for, so that an
/// answer to an older question can be told from the one wanted now.
enum Picture {
    Reading(Stamp),
    Ready(Stamp, SlicedProtocol),
    /// No file there, or nothing in it that decodes as a picture.
    Nothing(Stamp),
}

impl Picture {
    fn stamp(&self) -> Stamp {
        match self {
            Self::Reading(stamp) | Self::Ready(stamp, _) | Self::Nothing(stamp) => *stamp,
        }
    }
}

/// What a picture was made from: when the file was last written, and how wide the column
/// was. A change in either is a different picture, and is read again.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    written: Option<SystemTime>,
    width: u16,
}

struct Loaded {
    file: PathBuf,
    stamp: Stamp,
    picture: Option<SlicedProtocol>,
}

/// Where a picture sits in the document: the block it is of, and the file it names.
pub struct Placed {
    pub index: usize,
    pub path: String,
}

impl Gallery {
    pub fn new(picker: Picker, document: &Path) -> Self {
        Self::with(Some(picker), document)
    }

    /// A gallery that draws nothing, for a screen with no terminal behind it. Every image
    /// block keeps the line naming its file, which is what a picture still being read
    /// shows anyway.
    pub fn blind() -> Self {
        Self::with(None, Path::new(""))
    }

    fn with(picker: Option<Picker>, document: &Path) -> Self {
        let (back, finished) = channel();
        Self {
            picker,
            root: document.parent().unwrap_or(Path::new("")).to_path_buf(),
            pictures: HashMap::new(),
            wanted: HashSet::new(),
            placed: Vec::new(),
            finished,
            back,
        }
    }

    /// Take in whatever the threads have finished with, and let go of the pictures the
    /// last frame did not ask for. True when a picture arrived, which is the one thing
    /// that changes how many rows a block already laid out takes.
    pub fn settle(&mut self) -> bool {
        let mut arrived = false;
        while let Ok(loaded) = self.finished.try_recv() {
            // An answer to a question no longer being asked — the file changed again, or
            // the column did — is dropped rather than drawn.
            if self.pictures.get(&loaded.file).map(Picture::stamp) != Some(loaded.stamp) {
                continue;
            }
            let picture = match loaded.picture {
                Some(picture) => Picture::Ready(loaded.stamp, picture),
                None => Picture::Nothing(loaded.stamp),
            };
            self.pictures.insert(loaded.file, picture);
            arrived = true;
        }
        let wanted = std::mem::take(&mut self.wanted);
        self.pictures.retain(|file, _| wanted.contains(file));
        arrived
    }

    /// How many rows the picture at `path` takes, once there is one to draw. Asking is
    /// what starts it being read, and what keeps it: a path nothing asks about is let go.
    pub fn rows(&mut self, path: &str, width: u16) -> Option<u16> {
        let picker = self.picker.clone()?;
        let file = self.root.join(path);
        let stamp = Stamp { written: written(&file), width };
        self.wanted.insert(file.clone());

        match self.pictures.get(&file) {
            Some(Picture::Ready(had, picture)) if *had == stamp => {
                return Some(picture.size().height);
            }
            Some(picture) if picture.stamp() == stamp => return None,
            _ => {}
        }
        self.pictures.insert(file.clone(), Picture::Reading(stamp));
        read(picker, file, stamp, self.back.clone());
        None
    }

    /// Draw the pictures of the blocks that reserved rows for them. A picture may start
    /// above the top of the screen or run past its bottom; the slices of it that fall
    /// inside are the ones drawn, so scrolling past a tall one is not all or nothing.
    ///
    /// A picture is not in the buffer. The terminal draws it over the cells and holds it
    /// there until something is written over them, which the editor never knows to do: a
    /// cell that is a blank one frame and a blank the next has not changed, and an
    /// unchanged cell is not sent. So a frame that has moved a picture is not told from
    /// the last one cell by cell — it is sent whole, and the picture the terminal was
    /// holding is written over along with everything else.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, document: &Document, scroll: usize) {
        let column = view::column(area);
        let mut placed = Vec::new();
        for picture in document.pictures() {
            let Some(Picture::Ready(_, ready)) = self.pictures.get(&self.root.join(&picture.path))
            else {
                continue;
            };
            let top = document.top(picture.index) as isize - scroll as isize;
            let Ok(top) = i16::try_from(top) else { continue };
            frame.render_widget(SlicedImage::new(ready, SignedPosition::from((0, top))), column);
            placed.push((picture.index, top, ready.size()));
        }
        if placed != self.placed {
            resend(frame);
        }
        self.placed = placed;
    }
}

/// Send every cell the editor drew again, whatever the last frame had in it. The cells a
/// picture stands in are the terminal's own and are left out: they are marked to be
/// skipped as it is drawn, and writing over them would rub it out.
fn resend(frame: &mut Frame) {
    let area = frame.area();
    let buffer = frame.buffer_mut();
    for at in area.positions() {
        if let Some(cell) = buffer.cell_mut(at)
            && cell.diff_option == CellDiffOption::None
        {
            cell.set_diff_option(CellDiffOption::AlwaysUpdate);
        }
    }
}

fn written(file: &Path) -> Option<SystemTime> {
    file.metadata().ok()?.modified().ok()
}

/// Read, scale and encode a picture off the event loop. Encoding a sixel of any size
/// takes long enough to be felt as a dropped keystroke, so none of it happens on the
/// thread that draws.
fn read(picker: Picker, file: PathBuf, stamp: Stamp, back: Sender<Loaded>) {
    thread::spawn(move || {
        let picture = decode(&picker, &file, stamp.width);
        let _ = back.send(Loaded { file, stamp, picture });
    });
}

fn decode(picker: &Picker, file: &Path, width: u16) -> Option<SlicedProtocol> {
    let reader = image::ImageReader::open(file).ok()?.with_guessed_format().ok()?;
    let image = reader.decode().ok()?;
    let size = fitted(picker, &image, width);
    SlicedProtocol::new_with_resize(picker, image, size, Resize::Fit(None)).ok()
}

/// The room a picture is given: as many cells as its pixels come to at this terminal's
/// cell size, and never wider than the column. The height follows the width, because what
/// is drawn there keeps the picture's proportions.
fn fitted(picker: &Picker, image: &DynamicImage, width: u16) -> Size {
    let font = picker.font_size();
    let columns = (image.width().div_ceil(font.width as u32) as u16).max(1);
    let rows = (image.height().div_ceil(font.height as u32) as u16).max(1);
    if columns <= width {
        return Size::new(columns, rows);
    }
    Size::new(width, ((rows as u32 * width as u32) / columns as u32).max(1) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_image::picker::Picker;
    use std::time::Duration;

    fn gallery() -> Gallery {
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("sample/post.md");
        Gallery::new(Picker::halfblocks(), &sample)
    }

    /// Ask until the thread has come back with something, which is what a run of frames
    /// does. Nothing in the editor waits on a picture; the waiting is this test's own.
    fn settled(gallery: &mut Gallery, path: &str) -> Option<u16> {
        for _ in 0..500 {
            gallery.settle();
            let rows = gallery.rows(path, 72);
            let file = gallery.root.join(path);
            if !matches!(gallery.pictures.get(&file), Some(Picture::Reading(_))) {
                return rows;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the thread never came back");
    }

    #[test]
    fn reads_a_picture_and_says_how_many_rows_it_takes() {
        // The sample is 480 by 240 pixels, and a mosaic counts a cell as ten by twenty:
        // 48 columns by 12 rows, which is inside the column and so is not scaled down.
        assert_eq!(settled(&mut gallery(), "image.png"), Some(12));
    }

    #[test]
    fn says_nothing_of_a_file_that_is_not_there() {
        // Which leaves the block saying what it is of and where it should have been.
        assert_eq!(settled(&mut gallery(), "nothing.png"), None);
    }

    /// A picture nothing asked after on the last frame is let go of, so a block typed
    /// over does not leave its picture behind in memory.
    #[test]
    fn forgets_a_picture_the_document_no_longer_holds() {
        let mut gallery = gallery();
        assert_eq!(settled(&mut gallery, "image.png"), Some(12));
        gallery.settle();
        assert!(!gallery.pictures.is_empty(), "still asked after, so still here");
        gallery.settle();
        assert!(gallery.pictures.is_empty(), "asked after by nothing, so let go");
    }
}
