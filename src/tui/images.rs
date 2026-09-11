//! Pictures. Everything about drawing one is here: how many rows it takes, the thread
//! that reads it, and the slices of it that fall inside the screen. Which protocol the
//! terminal draws with — kitty, sixel, or a mosaic of half-blocks — is the probe's answer
//! and is carried in the picker; nothing here names a terminal.
//!
//! A picture is asked for by the block that holds it and answered on a later frame: the
//! file is read, scaled and encoded off the event loop, and until it comes back the block
//! says what it is of and where it is kept. A file that is not there says the same thing
//! for good.
//!
//! A gif moves. Every frame of it is encoded on the same thread and at the same time as
//! the first, so turning to the next costs only the sending; what that comes to is the
//! whole screen, every turn, because a picture is not in the buffer and a frame that has
//! changed one cannot be told from the last cell by cell. Which is why a frame is never
//! given less than `QUICKEST`, whatever the gif asks for.

use crate::tui::view::{self, Document};
use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder, DynamicImage, ImageFormat};
use ratatui::Frame;
use ratatui::buffer::CellDiffOption;
use ratatui::layout::{Rect, Size};
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::sliced::{SignedPosition, SlicedImage, SlicedProtocol};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Seek};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

/// The least time a frame of a gif is given, whatever it asks for. A turn costs a
/// screenful on the wire; ten a second is what a terminal was measured carrying without
/// the writer feeling it in the keyboard, and a gif asking to go faster is drawn at this
/// instead — which is what a browser does with one too.
const QUICKEST: Duration = Duration::from_millis(100);

/// How many frames of a gif are kept. Each is an encoded picture in its own right, so a
/// long gif is one picture held over and over; past this it loops early rather than
/// filling the machine with something nobody asked to be this long.
const FRAMES: usize = 120;

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
    /// started on, how big it was, and which frame of it was up. A frame that has any of
    /// them elsewhere — a gif that has turned included — is sent whole.
    placed: Vec<(usize, i16, Size, usize)>,
    finished: Receiver<Loaded>,
    back: Sender<Loaded>,
}

/// One picture, and what has become of it. Each carries what it was asked for, so that an
/// answer to an older question can be told from the one wanted now.
enum Picture {
    Reading(Stamp),
    Ready(Stamp, Reel),
    /// No file there, or nothing in it that decodes as a picture.
    Nothing(Stamp),
}

/// One frame, and how long it stays up.
struct Still {
    picture: SlicedProtocol,
    lasts: Duration,
}

/// The frames of a picture and which of them is up. A still is a reel of one: it never
/// turns, is never waited on, and costs nothing that the single picture did not.
struct Reel {
    stills: Vec<Still>,
    showing: usize,
    /// When the frame that is up has had its time, or nothing for a reel of one.
    until: Option<Instant>,
}

impl Reel {
    fn new(stills: Vec<Still>) -> Self {
        let until = (stills.len() > 1).then(|| Instant::now() + stills[0].lasts);
        Self { stills, showing: 0, until }
    }

    fn showing(&self) -> &SlicedProtocol {
        &self.stills[self.showing].picture
    }

    fn size(&self) -> Size {
        self.showing().size()
    }

    /// Turn to the next frame, if the one up has had its time. The next is due a frame's
    /// worth after this one was rather than after now, so that a turn taken a little late
    /// does not push the rest of the reel back with it.
    fn turn(&mut self, now: Instant) {
        let Some(until) = self.until else { return };
        if now < until {
            return;
        }
        self.showing = (self.showing + 1) % self.stills.len();
        let lasts = self.stills[self.showing].lasts;
        // Unless it is properly behind — the machine was busy, or the document was
        // scrolled away and back. Then the reel takes up from where it is rather than
        // racing through every frame it was away for.
        let next = until + lasts;
        self.until = Some(if next > now { next } else { now + lasts });
    }
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
    stills: Option<Vec<Still>>,
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
            let picture = match loaded.stills {
                Some(stills) => Picture::Ready(loaded.stamp, Reel::new(stills)),
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
            Some(Picture::Ready(had, reel)) if *had == stamp => {
                return Some(reel.size().height);
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
        self.turn();
        let column = view::column(area);
        let mut placed = Vec::new();
        for picture in document.pictures() {
            let Some(Picture::Ready(_, reel)) = self.pictures.get(&self.root.join(&picture.path))
            else {
                continue;
            };
            let top = document.top(picture.index) as isize - scroll as isize;
            let Ok(top) = i16::try_from(top) else { continue };
            let showing = SlicedImage::new(reel.showing(), SignedPosition::from((0, top)));
            frame.render_widget(showing, column);
            placed.push((picture.index, top, reel.size(), reel.showing));
        }
        if placed != self.placed {
            resend(frame);
        }
        self.placed = placed;
    }

    /// When the soonest gif on the screen is due to turn, which is when the loop has to
    /// look up from the keyboard although nothing has been typed. Nothing at all where no
    /// picture moves, which is every document without a gif in it.
    pub fn due(&self) -> Option<Instant> {
        self.pictures
            .values()
            .filter_map(|picture| match picture {
                Picture::Ready(_, reel) => reel.until,
                _ => None,
            })
            .min()
    }

    /// Turn every reel whose frame has had its time. Done as the frame is drawn, so that
    /// what is sent is whatever is up at the moment of sending.
    fn turn(&mut self) {
        let now = Instant::now();
        for picture in self.pictures.values_mut() {
            if let Picture::Ready(_, reel) = picture {
                reel.turn(now);
            }
        }
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
        let stills = decode(&picker, &file, stamp.width);
        let _ = back.send(Loaded { file, stamp, stills });
    });
}

fn decode(picker: &Picker, file: &Path, width: u16) -> Option<Vec<Still>> {
    let reader = image::ImageReader::open(file).ok()?.with_guessed_format().ok()?;
    if reader.format() == Some(ImageFormat::Gif) {
        return reel(picker, reader.into_inner(), width);
    }
    let image = reader.decode().ok()?;
    let room = fitted(picker, &image, width);
    let picture = SlicedProtocol::new_with_resize(picker, image, room, Resize::Fit(None)).ok()?;
    // A reel of one, which is a still: the time is never asked for.
    Some(vec![Still { picture, lasts: Duration::ZERO }])
}

/// Every frame of a gif, encoded and timed. The room is measured from the first frame and
/// given to all of them: the frames of a gif are one picture, and one drawn to its own
/// size would jump. A gif of a single frame comes out of here a still.
fn reel(picker: &Picker, read: impl BufRead + Seek, width: u16) -> Option<Vec<Still>> {
    let mut room = None;
    let mut stills = Vec::new();
    for frame in GifDecoder::new(read).ok()?.into_frames().take(FRAMES) {
        let frame = frame.ok()?;
        let lasts = lasts(&frame);
        let image = DynamicImage::ImageRgba8(frame.into_buffer());
        let room = *room.get_or_insert_with(|| fitted(picker, &image, width));
        let picture =
            SlicedProtocol::new_with_resize(picker, image, room, Resize::Fit(None)).ok()?;
        stills.push(Still { picture, lasts });
    }
    (!stills.is_empty()).then_some(stills)
}

/// How long a frame stays up: what the gif asks for, and never less than the terminal can
/// carry. A gif that asks for nothing — plenty do — gets the floor, which is the speed it
/// would have been played at anyway.
fn lasts(frame: &image::Frame) -> Duration {
    let (numerator, denominator) = frame.delay().numer_denom_ms();
    Duration::from_millis((numerator / denominator.max(1)) as u64).max(QUICKEST)
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
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/post.md");
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

    /// A gif is a picture like any other: it reserves the rows its first frame takes, and
    /// every frame after that is the same size.
    #[test]
    fn reads_a_gif_and_says_how_many_rows_it_takes() {
        // Sixty by forty pixels, and a mosaic counts a cell as ten by twenty: two rows.
        assert_eq!(settled(&mut gallery(), "loop.gif"), Some(2));
    }

    #[test]
    fn holds_every_frame_of_a_gif() {
        let mut gallery = gallery();
        settled(&mut gallery, "loop.gif");
        let Some(Picture::Ready(_, reel)) = gallery.pictures.get(&gallery.root.join("loop.gif"))
        else {
            panic!("the gif did not come back");
        };
        assert_eq!(reel.stills.len(), 4);
    }

    #[test]
    fn says_nothing_of_a_file_that_is_not_there() {
        // Which leaves the block saying what it is of and where it should have been.
        assert_eq!(settled(&mut gallery(), "nothing.png"), None);
    }

    /// A reel of the given number of frames, each up for `lasts`. What is in them does
    /// not matter here; what is being asked about is the clock.
    fn reel(frames: usize, lasts: Duration) -> Reel {
        let picker = Picker::halfblocks();
        let stills = (0..frames)
            .map(|_| {
                let image = DynamicImage::ImageRgba8(image::RgbaImage::new(10, 20));
                let size = Size::new(1, 1);
                let picture =
                    SlicedProtocol::new_with_resize(&picker, image, size, Resize::Fit(None))
                        .expect("a cell of a picture encodes");
                Still { picture, lasts }
            })
            .collect();
        Reel::new(stills)
    }

    #[test]
    fn turns_to_the_next_frame_once_the_one_up_has_had_its_time() {
        let mut reel = reel(3, Duration::from_millis(100));
        let at = Instant::now();
        reel.turn(at);
        assert_eq!(reel.showing, 0, "its time is not up yet");
        reel.turn(at + Duration::from_millis(150));
        assert_eq!(reel.showing, 1);
        reel.turn(at + Duration::from_millis(250));
        assert_eq!(reel.showing, 2);
        reel.turn(at + Duration::from_millis(350));
        assert_eq!(reel.showing, 0, "and round again");
    }

    /// A gif scrolled off the screen and back, or a machine that was busy elsewhere, comes
    /// back to the frame it is on rather than racing through every one it was away for.
    #[test]
    fn a_reel_left_alone_takes_up_where_it_is() {
        let mut reel = reel(3, Duration::from_millis(100));
        let late = Instant::now() + Duration::from_secs(5);
        reel.turn(late);
        assert_eq!(reel.showing, 1);
        reel.turn(late);
        assert_eq!(reel.showing, 1, "one turn for one look, however long it was away");
    }

    /// A still is a reel of one. Nothing waits on it and it never turns, so a document
    /// with no gif in it costs what it always did.
    #[test]
    fn a_still_never_turns_and_is_never_waited_for() {
        let mut reel = reel(1, Duration::ZERO);
        assert!(reel.until.is_none());
        reel.turn(Instant::now() + Duration::from_secs(60));
        assert_eq!(reel.showing, 0);
    }

    #[test]
    fn a_gif_in_a_hurry_is_slowed_to_what_the_terminal_carries() {
        let frame = |ms| {
            image::Frame::from_parts(
                image::RgbaImage::new(1, 1),
                0,
                0,
                image::Delay::from_numer_denom_ms(ms, 1),
            )
        };
        assert_eq!(lasts(&frame(20)), QUICKEST, "faster than the wire can carry");
        assert_eq!(lasts(&frame(0)), QUICKEST, "asking for nothing at all");
        assert_eq!(lasts(&frame(500)), Duration::from_millis(500), "its own pace, kept");
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
