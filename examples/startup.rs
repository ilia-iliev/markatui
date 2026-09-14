//! How long the editor takes to put a document on the screen, measured the way the
//! writer feels it: from the moment the process starts to the moment the first frame has
//! finished arriving.
//!
//! There is no way to measure that without a terminal. The editor asks the terminal what
//! it can do before it draws anything, and those questions are answered on the same
//! stream the drawing goes out on; run it into a pipe and it takes a different path
//! through the code and waits for answers that never come. So this opens a
//! pseudo-terminal, runs the editor in it, and plays the part of the terminal on the
//! other end — answering the questions a real one would, and writing down when every
//! byte arrived.
//!
//! ```text
//! cargo run --release --example startup -- sample/demo.md
//! cargo run --release --example startup -- sample/demo.md --runs 10 --timeline
//! ```
//!
//! The timeline is where a slow start is found. Output comes in bursts with the work
//! between them, so a gap in the timeline is a phase of the startup, and the phase that
//! is costing the writer their patience is the widest gap.

mod arguments;

use arguments::number;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long the output has to stay quiet before the frame counts as finished. Long
/// enough that a frame written in several writes is not mistaken for two, short enough
/// that it is a rounding error next to a hundred milliseconds.
const QUIET: Duration = Duration::from_millis(30);
/// How long to wait for the editor to say anything at all before giving up on it.
const PATIENCE: Duration = Duration::from_secs(10);
/// The screen the editor is measured on. A wider one has more to draw.
const SIZE: (u16, u16) = (100, 30);

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(document) = arguments.iter().find(|argument| !argument.starts_with('-')) else {
        eprintln!("usage: startup <file.md> [--runs N] [--timeline]");
        std::process::exit(2);
    };
    let runs = number(&arguments, "--runs").unwrap_or(5);
    let timeline = arguments.iter().any(|argument| argument == "--timeline");
    let editor = editor();

    println!("{} on a {}x{} screen, {runs} runs", editor.display(), SIZE.0, SIZE.1);
    let mut measured: Vec<Duration> = Vec::new();
    for run in 0..runs {
        let bursts = start(&editor, Path::new(document));
        let Some(first_frame) = bursts.last().map(|(at, _)| *at) else {
            eprintln!("the editor drew nothing");
            std::process::exit(1);
        };
        if timeline && run == 0 {
            show(&bursts);
        }
        measured.push(first_frame);
    }

    measured.sort();
    println!(
        "\nfirst frame: {} best, {} median, {} worst",
        millis(measured[0]),
        millis(measured[measured.len() / 2]),
        millis(measured[measured.len() - 1])
    );
}

/// The editor this example was built beside, which is the one worth measuring: an example
/// lives in `target/<profile>/examples` and the binary in `target/<profile>`.
fn editor() -> PathBuf {
    let path = std::env::current_exe().expect("the example knows where it is");
    path.parent()
        .and_then(Path::parent)
        .expect("an example is two directories down from the profile")
        .join("markatui")
}

/// One run: the editor opened on `document` in a terminal of our own, up to the moment
/// its first frame has finished arriving. What comes back is every burst of output it
/// wrote, each with how long after the start it landed.
fn start(editor: &Path, document: &Path) -> Vec<(Duration, Vec<u8>)> {
    let (master, slave) = pseudoterminal();
    let began = Instant::now();
    let mut child = Command::new(editor)
        .arg(document)
        .stdin(Stdio::from(slave.try_clone().expect("the terminal is readable")))
        .stdout(Stdio::from(slave.try_clone().expect("the terminal is writable")))
        .stderr(Stdio::from(slave))
        .spawn()
        .expect("the editor runs");

    let bursts = listen(&master, began);
    // Ctrl+Q, which is what the keymap calls quit. A document nobody has typed into goes
    // without being asked about.
    let _ = (&master).write_all(b"\x11");
    let _ = child.wait();
    bursts
}

/// Read from the terminal until the frame has arrived and the editor has gone quiet for
/// [`QUIET`], answering the questions it asks along the way. Reading happens on a thread
/// of its own so that the quiet can be timed rather than waited into.
///
/// Quiet only counts once something has been drawn. The editor asks its questions and
/// then goes away to lay the document out, and on a long one that pause is longer than
/// [`QUIET`] — a measurement that stopped there would be timing the questions and calling
/// it the frame.
fn listen(master: &File, began: Instant) -> Vec<(Duration, Vec<u8>)> {
    let (send, receive) = mpsc::channel();
    let reading = master.try_clone().expect("the terminal is readable");
    std::thread::spawn(move || {
        let mut terminal = reading;
        let mut buffer = [0u8; 8192];
        while let Ok(read) = terminal.read(&mut buffer) {
            if read == 0 || send.send((began.elapsed(), buffer[..read].to_vec())).is_err() {
                return;
            }
        }
    });

    let mut bursts: Vec<(Duration, Vec<u8>)> = Vec::new();
    let mut drawn = false;
    loop {
        let waiting = if drawn { QUIET } else { PATIENCE };
        let Ok((at, bytes)) = receive.recv_timeout(waiting) else {
            return bursts;
        };
        for answer in answers(&bytes) {
            let _ = (&*master).write_all(answer);
        }
        drawn |= draws(&bytes);
        bursts.push((at, bytes));
    }
}

/// Whether a burst is the editor drawing rather than setting the terminal up. Everything
/// it writes before the first frame is a question or a mode being turned on, and each of
/// those is one of the sequences [`asked_about`] names.
fn draws(bytes: &[u8]) -> bool {
    asked_about(bytes).is_empty()
}

/// What a burst is about, in the words the timeline prints. Empty for a burst that is
/// none of these things, which is the editor drawing.
fn asked_about(bytes: &[u8]) -> Vec<&'static str> {
    let text = String::from_utf8_lossy(bytes);
    let wrote = |sequence: &str| text.contains(sequence);
    [
        (wrote("\x1b[?u") || wrote("\x1b[>")).then_some("asks about the keyboard"),
        wrote("\x1b_G").then_some("asks about pictures"),
        (wrote("\x1b[16t") || wrote("\x1b[14t")).then_some("asks about cell size"),
        wrote("\x1b[?1049h").then_some("takes the screen"),
        wrote("\x1b[?2004h").then_some("turns paste on"),
        wrote("\x1b]11;").then_some("colours terminal padding"),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// What a terminal would say back. The editor asks what the keyboard, the pictures and
/// the cell size can do before it draws, and a question left hanging is a wait of
/// seconds rather than milliseconds — so the part played here is a terminal that has all
/// three, which is the good case a writer on kitty or foot is actually in.
///
/// The order is the terminal's own: the status report goes last because that is what the
/// asking side reads as "there is nothing more coming".
fn answers(written: &[u8]) -> Vec<&'static [u8]> {
    let text = String::from_utf8_lossy(written);
    let asked = |question: &str| text.contains(question);
    [
        // The kitty keyboard flags, pushed and read back.
        asked("\x1b[?u").then_some(&b"\x1b[?1u"[..]),
        // The kitty graphics protocol, asked about by sending a one-pixel picture.
        asked("\x1b_G").then_some(&b"\x1b_Gi=31;OK\x1b\\"[..]),
        // What kind of terminal this is, which is where sixel support is read from.
        asked("\x1b[c").then_some(&b"\x1b[?62;4c"[..]),
        // How many pixels a cell is, which is what says how many rows a picture takes.
        asked("\x1b[16t").then_some(&b"\x1b[6;18;9t"[..]),
        // Are you there. Every terminal answers it, which is why it is asked last.
        asked("\x1b[5n").then_some(&b"\x1b[0n"[..]),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A pseudo-terminal, as the two ends of one: ours to watch, and the editor's to run in.
fn pseudoterminal() -> (File, File) {
    // SAFETY: the calls below are the documented way to open a pseudo-terminal, each
    // checked for failure, and the descriptors are handed straight to types that own them.
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        checked(master, "posix_openpt");
        checked(libc::grantpt(master), "grantpt");
        checked(libc::unlockpt(master), "unlockpt");

        let size = libc::winsize {
            ws_row: SIZE.1,
            ws_col: SIZE.0,
            ws_xpixel: SIZE.0 * 9,
            ws_ypixel: SIZE.1 * 18,
        };
        checked(libc::ioctl(master, libc::TIOCSWINSZ, &size), "TIOCSWINSZ");

        let name = libc::ptsname(master);
        assert!(!name.is_null(), "ptsname: {}", std::io::Error::last_os_error());
        let slave = libc::open(name, libc::O_RDWR | libc::O_NOCTTY);
        checked(slave, "open");

        (File::from_raw_fd(master), File::from_raw_fd(slave))
    }
}

fn checked(result: i32, call: &str) {
    assert!(result >= 0, "{call}: {}", std::io::Error::last_os_error());
}

/// Every burst of output with the gap in front of it. The gaps are the startup's phases:
/// the editor writes, goes away and does something, and writes again.
fn show(bursts: &[(Duration, Vec<u8>)]) {
    println!("\n{:>9}  {:>8}  what arrived", "at", "gap");
    let mut previous = Duration::ZERO;
    for (at, bytes) in bursts {
        println!("{:>9}  {:>8}  {}", millis(*at), millis(*at - previous), sketch(bytes));
        previous = *at;
    }
}

/// What a burst was, in a few words. The whole of it is escape codes and text, and
/// neither reads well; what is worth knowing is which part of the startup it was.
fn sketch(bytes: &[u8]) -> String {
    let mut parts = asked_about(bytes);
    if parts.is_empty() {
        parts.push("draws");
    }
    format!("{} ({} bytes)", parts.join(", "), bytes.len())
}

fn millis(span: Duration) -> String {
    format!("{:.1} ms", span.as_secs_f64() * 1000.0)
}
