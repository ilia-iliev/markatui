//! Whether an animated picture is worth having on this terminal, asked the only way it
//! can be answered: by animating one and typing over the top of it.
//!
//! The cost of a moving picture is not the decoding or the encoding — both happen off the
//! loop in the editor and are already paid for by the time a frame is drawn. It is what
//! goes out on the wire. A picture is not in the buffer, so a frame that has changed one
//! cannot be told from the last frame cell by cell; the whole screen goes out again, with
//! the picture, every tick. Under sixel that is the picture re-encoded and re-sent every
//! time, because there is nothing the terminal keeps between frames.
//!
//! So this writes down the one number the writer feels: how long the loop spends with its
//! back to the keyboard. Every tick is a stretch of time in which a keystroke can arrive
//! and not be read, and the worst of those stretches is the lag on a key pressed at the
//! wrong moment. Type while it runs — the line under the picture echoes what arrives, and
//! whether it keeps up is the answer the percentiles are only the evidence for.
//!
//! ```text
//! cargo run --release --example gifloop
//! cargo run --release --example gifloop -- --fps 4
//! cargo run --release --example gifloop -- sample/whatever.gif --fps 12
//! ```
//!
//! Press q or Esc to stop. What it prints on the way out is the verdict.

mod arguments;

use arguments::number;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, terminal};
use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder, DynamicImage, Rgba, RgbaImage};
// The gallery's own sizing and redraw, so that this goes on measuring the gallery when
// they change.
use markatui::tui::images::{fitted, resend};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::{Frame, widgets::Widget};
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::sliced::{SignedPosition, SlicedImage, SlicedProtocol};
use std::fs::File;
use std::io::{self, BufReader, Stdout, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The column the editor draws a document in, which is the room a picture in one gets.
const COLUMN: u16 = 72;
/// The picture made when no gif is named: wide enough to cost what a real one costs.
const MADE: (u32, u32) = (480, 240);

fn main() -> io::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let fps = number(&arguments, "--fps").unwrap_or(10) as u32;
    let path = named(&arguments);

    let stills = match &path {
        Some(path) => read(path),
        None => make(number(&arguments, "--frames").unwrap_or(20) as usize),
    };
    println!("{} frames at {fps} fps", stills.len());

    // The terminal is asked what it draws with on the screen the cursor is on, the way
    // the editor asks: after the screen is ours, before anything is drawn on it.
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

    let written = Arc::new(AtomicU64::new(0));
    let mut terminal = Terminal::new(CrosstermBackend::new(Counted {
        out: io::stdout(),
        written: written.clone(),
    }))?;

    let outcome = run(&mut terminal, &picker, stills, fps, &written);

    terminal::disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    match outcome {
        Ok(measured) => {
            report(&measured, &picker, fps);
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// What one run came to: how long each tick held the keyboard, and what went out on it.
struct Measured {
    encoding: Duration,
    ticks: Vec<Duration>,
    written: u64,
    ran: Duration,
    frames: usize,
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<Counted>>,
    picker: &Picker,
    stills: Vec<DynamicImage>,
    fps: u32,
    written: &AtomicU64,
) -> io::Result<Measured> {
    // Every frame encoded up front, which is what the gallery's thread would have done
    // by the time the first of them is drawn. None of this is on the tick.
    let at = Instant::now();
    let size = fitted(picker, &stills[0], COLUMN);
    let frames: Vec<SlicedProtocol> = stills
        .into_iter()
        .filter_map(|still| {
            SlicedProtocol::new_with_resize(picker, still, size, Resize::Fit(None)).ok()
        })
        .collect();
    let encoding = at.elapsed();

    let interval = Duration::from_micros(1_000_000 / fps as u64);
    let started = Instant::now();
    let mut ticks: Vec<Duration> = Vec::new();
    let mut typed = String::new();
    let mut showing = 0usize;
    let mut due = Instant::now();
    written.store(0, Ordering::Relaxed);

    loop {
        // The draw is the stretch the keyboard goes unheard through: nothing else on this
        // thread can run until the last byte of the frame has gone out.
        let at = Instant::now();
        terminal.draw(|frame| paint(frame, &frames[showing], showing, fps, &typed))?;
        ticks.push(at.elapsed());

        due += interval;
        // Behind rather than waiting: the terminal is taking longer to swallow a frame
        // than the gap between two, which is the animation asking for more than it can
        // have. Start the next gap from now so it does not spiral.
        let wait = due.saturating_duration_since(Instant::now());
        if wait.is_zero() {
            due = Instant::now();
        }
        if event::poll(wait)?
            && let Event::Key(key) = event::read()?
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char(letter) => typed.push(letter),
                KeyCode::Backspace => {
                    typed.pop();
                }
                _ => {}
            }
        }
        showing = (showing + 1) % frames.len();
    }

    Ok(Measured {
        encoding,
        ran: started.elapsed(),
        written: written.load(Ordering::Relaxed),
        frames: frames.len(),
        ticks,
    })
}

/// The screen the picture moves on: words above and below it, so that the resend has a
/// screenful to carry rather than an empty one.
fn paint(frame: &mut Frame, picture: &SlicedProtocol, showing: usize, fps: u32, typed: &str) {
    let area = frame.area();
    let width = COLUMN.min(area.width);
    let column = Rect { x: area.x + (area.width - width) / 2, width, ..area };
    let grey = Style::default().fg(Color::Rgb(140, 140, 150));

    let lines: Vec<Line> = (0..area.height).map(|row| Line::styled(filler(row), grey)).collect();
    frame.render_widget(Paragraph::new(lines), column);

    let top = 6i16;
    frame.render_widget(SlicedImage::new(picture, SignedPosition::from((0, top))), column);

    let under = top as u16 + picture.size().height + 1;
    if under + 2 < area.height {
        let foot = Rect { y: column.y + under, height: 2, ..column };
        let says = format!("frame {showing} at {fps} fps — q to stop\n> {typed}");
        Paragraph::new(says)
            .style(Style::default().fg(Color::Rgb(200, 200, 120)))
            .render(foot, frame.buffer_mut());
    }

    resend(frame);
}

/// A line of words for the row, so the screen has the weight of a document on it.
fn filler(row: u16) -> String {
    const WORDS: [&str; 8] = ["the", "picture", "moves", "and", "the", "words", "stay", "still"];
    (0..9).map(|word| WORDS[(row as usize + word) % WORDS.len()]).collect::<Vec<_>>().join(" ")
}

fn report(measured: &Measured, picker: &Picker, fps: u32) {
    let mut ticks = measured.ticks.clone();
    ticks.sort_unstable();
    let count = ticks.len().max(1);
    let total: Duration = ticks.iter().sum();
    let at = |part: f64| ticks[((count as f64 * part) as usize).min(count - 1)];

    println!("\n{:?} drawing {} frames", picker.protocol_type(), measured.frames);
    println!("encoding all of them up front: {:?}, off the loop", measured.encoding);
    println!("\nthe keyboard went unheard for, per tick:");
    println!(
        "  median {:?}   p90 {:?}   p99 {:?}   worst {:?}",
        at(0.5),
        at(0.9),
        at(0.99),
        ticks[count - 1]
    );
    println!("  which is the lag on a key pressed at the wrong moment");

    let seconds = measured.ran.as_secs_f64().max(0.001);
    let busy = total.as_secs_f64() / seconds;
    println!(
        "\nover {:.1}s: {count} ticks, {:.1} a second — {fps} asked for",
        seconds,
        count as f64 / seconds
    );
    println!("  {:.0}% of the loop spent writing to the terminal", busy * 100.0);
    println!(
        "  {:.0} KiB/s on the wire, {:.0} KiB a frame",
        measured.written as f64 / 1024.0 / seconds,
        measured.written as f64 / 1024.0 / count as f64
    );
    if busy > 0.5 {
        println!("\nmore than half the loop is the picture. Typing is what pays for it.");
    }
}

fn read(path: &str) -> Vec<DynamicImage> {
    let file = File::open(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let decoder = GifDecoder::new(BufReader::new(file)).expect("not a gif");
    decoder
        .into_frames()
        .collect_frames()
        .expect("the gif would not decode")
        .into_iter()
        .map(|frame| DynamicImage::ImageRgba8(frame.into_buffer()))
        .collect()
}

/// A picture that moves, for when no gif is named: a band of colour sweeping across a
/// field of it. Photographic enough that sixel has real work to do, and different enough
/// frame to frame that the eye can see whether it is keeping up.
fn make(frames: usize) -> Vec<DynamicImage> {
    let (width, height) = MADE;
    (0..frames)
        .map(|frame| {
            let sweep = (width as usize * frame / frames) as u32;
            let mut still = RgbaImage::new(width, height);
            for (x, y, pixel) in still.enumerate_pixels_mut() {
                let near = x.abs_diff(sweep).min(width - x.abs_diff(sweep));
                let lit = 255u32.saturating_sub(near * 4);
                *pixel = Rgba([
                    (x * 255 / width) as u8,
                    (y * 255 / height) as u8,
                    lit.min(255) as u8,
                    255,
                ]);
            }
            DynamicImage::ImageRgba8(still)
        })
        .collect()
}

/// The gif named on the command line, if one is. Every flag here takes a value, so the
/// word after a flag is that flag's and not a path.
fn named(arguments: &[String]) -> Option<String> {
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        if argument.starts_with('-') {
            rest.next();
            continue;
        }
        return Some(argument.clone());
    }
    None
}

/// Stdout with a tally on it. What a moving picture costs is measured in bytes, and this
/// is where they are counted: everything the backend writes passes through here.
struct Counted {
    out: Stdout,
    written: Arc<AtomicU64>,
}

impl Write for Counted {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let wrote = self.out.write(bytes)?;
        self.written.fetch_add(wrote as u64, Ordering::Relaxed);
        Ok(wrote)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}
