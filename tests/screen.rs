//! What the sample document looks like on a screen, at two widths and after a resize.
//! Nothing here asserts a colour: this is about where the rows fall and what is in them.

use markatui::editor::{Editor, Motion};
use markatui::tui::view;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// The sample document drawn on a screen `width` by `height`, as one string per row with
/// the trailing spaces off.
fn screen(editor: &Editor, width: u16, height: u16) -> Vec<String> {
    let mut document = view::Document::default();
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a test screen");
    terminal
        .draw(|frame| {
            document.rebuild(editor, markatui::tui::theme::CONTENT_WIDTH.min(width));
            view::draw(frame, frame.area(), editor, &document, 0);
        })
        .expect("a frame");

    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| {
            let row: String = (0..width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect();
            row.trim_end().to_string()
        })
        .collect()
}

fn sample() -> Editor {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("sample/post.md");
    let mut editor = Editor::open(&path);
    // Wherever the last session left the cursor, a snapshot starts at the top.
    editor.activate(0, 0);
    editor
}

#[test]
fn draws_the_sample_document() {
    let rows = screen(&sample(), 80, 30);
    // The heading has the cursor in it, so it is shown as it was typed.
    assert!(rows[1].contains("# markatui"), "{:?}", rows[1]);
    // Prose wraps inside the column and the markers are gone from it.
    assert!(rows.iter().any(|row| row.contains("raw source")), "{rows:#?}");
    assert!(!rows.iter().any(|row| row.contains("**raw source**")), "{rows:#?}");
    // A heading away from the cursor has lost its hashes.
    assert!(rows.iter().any(|row| row.trim() == "Lists"), "{rows:#?}");
    // And the list under it reads as a list.
    assert!(rows.iter().any(|row| row.trim_start().starts_with("• First item")), "{rows:#?}");
}

#[test]
fn keeps_inside_the_column_at_any_width() {
    for (width, height) in [(80, 40), (120, 40), (40, 40)] {
        let rows = screen(&sample(), width, height);
        for row in &rows {
            assert!(
                row.chars().count() <= width as usize,
                "a row ran past {width} columns: {row:?}"
            );
        }
    }
}

/// A resize is the same document laid out again; nothing about it is remembered from the
/// width before.
#[test]
fn lays_the_same_document_out_again_after_a_resize() {
    let editor = sample();
    let narrow = screen(&editor, 80, 40);
    let wide = screen(&editor, 120, 40);
    assert_eq!(narrow, screen(&editor, 80, 40));
    assert_ne!(narrow, wide);
}

#[test]
fn draws_the_quote_the_bullet_and_the_table() {
    let rows = screen(&sample(), 100, 60);
    assert!(rows.iter().any(|row| row.contains("▎ Writing is thinking")), "{rows:#?}");
    assert!(rows.iter().any(|row| row.trim_start().starts_with("┌─")), "{rows:#?}");
    assert!(rows.iter().any(|row| row.contains("│ Block")), "{rows:#?}");
    // A fenced block keeps its code and says what language it is in.
    assert!(rows.iter().any(|row| row.contains("fn main() {")), "{rows:#?}");
    assert!(rows.iter().any(|row| row.trim() == "rust"), "{rows:#?}");
    assert!(!rows.iter().any(|row| row.contains("```")), "{rows:#?}");
}

/// The caret follows the cursor through the document, and the block it lands in opens up
/// into its markdown while the ones around it stay as they read.
#[test]
fn shows_the_block_under_the_cursor_as_markdown() {
    let mut editor = sample();
    let rows = screen(&editor, 100, 60);
    assert!(rows.iter().any(|row| row.trim() == "Code"), "{rows:#?}");

    // Walk down to the "## Code" heading and it comes back as source. Down moves a line
    // at a time, so a block of several takes several presses to cross.
    for _ in 0..200 {
        if editor.active().text().starts_with("## Code") {
            break;
        }
        editor.move_cursor(Motion::Line(1), false);
    }
    assert!(editor.active().text().starts_with("## Code"), "the heading is in the sample");
    let rows = screen(&editor, 100, 60);
    assert!(rows.iter().any(|row| row.trim() == "## Code"), "{rows:#?}");
}
