//! Print the sample document as it would appear on a screen of a given size. Handy for
//! looking at the layout without a terminal to run the editor in.

use markatui::editor::Editor;
use markatui::tui::{theme, view};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let path = arguments.next().expect("usage: render <file.md> [width] [height]");
    let width: u16 = arguments.next().map(|w| w.parse().unwrap()).unwrap_or(80);
    let height: u16 = arguments.next().map(|h| h.parse().unwrap()).unwrap_or(40);

    let mut editor = Editor::open(std::path::Path::new(&path));
    editor.activate(0, 0);
    let mut document = view::Document::default();
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            document.rebuild(&editor, theme::CONTENT_WIDTH.min(width));
            view::draw(frame, frame.area(), &editor, &document, 0);
        })
        .unwrap();

    let buffer = terminal.backend().buffer().clone();
    for y in 0..height {
        let row: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
        println!("{}", row.trim_end());
    }
}
