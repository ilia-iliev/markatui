use markatui::tui;
use std::path::PathBuf;

fn main() {
    // A document is the whole point: there is no way to pick one from inside the editor.
    let Some(path) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: markatui <file.md>");
        std::process::exit(2);
    };
    if let Err(error) = tui::run(&path) {
        eprintln!("markatui: {error}");
        std::process::exit(1);
    }
}
