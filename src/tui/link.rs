//! Handing a link to the machine. The desktop decides what opens it — a browser, a
//! viewer, another editor — and the only thing said here is that the editor is not
//! waiting for it: a writer who follows a link is still writing.

use std::io;
use std::process::{Command, Stdio};

/// The one program that knows what a link is for. Every desktop this editor runs on
/// ships it, and it is the only thing between here and their own choice of browser.
const OPENER: &str = "xdg-open";

/// Hand `url` to the desktop. What comes back is what the writer needs told: nothing,
/// where it opened, and why not, where it did not.
pub fn url(url: &str) -> Result<(), String> {
    match spawn(url) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(format!("Could not open {url}: {OPENER} is not installed"))
        }
        Err(error) => Err(format!("Could not open {url}: {error}")),
    }
}

/// Started and let go of: its output would be written over the editor's screen, and
/// waiting for it would hold the editor still while the writer reads.
fn spawn(url: &str) -> io::Result<()> {
    Command::new(OPENER)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}
