//! What the sample document looks like on a screen, at two widths and after a resize.
//! Nothing here asserts a colour: this is about where the rows fall and what is in them.

use markatui::editor::{Editor, Motion};
use markatui::marks::{Align, Mark};
use markatui::tui::images::Gallery;
use markatui::tui::view;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::CellDiffOption;
use ratatui_image::picker::Picker;

/// The sample document drawn on a screen `width` by `height`, as one string per row with
/// the trailing spaces off.
fn screen(editor: &Editor, width: u16, height: u16) -> Vec<String> {
    drawn(editor, width, height, &mut view::Document::default(), &mut Gallery::blind())
}

/// One frame. The document and the gallery are the caller's, so that a run of frames can
/// be drawn the way the editor draws them: a picture takes several to arrive, and what
/// the last frame made of a block is what tells how far this one moved it.
fn drawn(
    editor: &Editor,
    width: u16,
    height: u16,
    document: &mut view::Document,
    gallery: &mut Gallery,
) -> Vec<String> {
    drawn_and_resent(editor, width, height, document, gallery).0
}

/// One frame, and the rows of it that are being sent to the terminal again whatever it
/// already has in them — which is how a picture the terminal is holding is written over.
fn drawn_and_resent(
    editor: &Editor,
    width: u16,
    height: u16,
    document: &mut view::Document,
    gallery: &mut Gallery,
) -> (Vec<String>, Vec<u16>) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a test screen");
    let mut resent = Vec::new();
    terminal
        .draw(|frame| {
            document.rebuild(editor, markatui::tui::theme::content_width().min(width), gallery);
            view::draw(frame, frame.area(), editor, document, 0);
            gallery.draw(frame, frame.area(), document, 0);
            resent = (0..height)
                .filter(|y| {
                    (0..width).any(|x| {
                        frame.buffer_mut()[(x, *y)].diff_option == CellDiffOption::AlwaysUpdate
                    })
                })
                .collect();
        })
        .expect("a frame");

    let buffer = terminal.backend().buffer().clone();
    let rows = (0..height)
        .map(|y| {
            let row: String = (0..width).map(|x| buffer[(x, y)].symbol().to_string()).collect();
            row.trim_end().to_string()
        })
        .collect();
    (rows, resent)
}

/// A document of its own with the sample picture beside it, so that a test drawing one
/// disturbs nothing else.
fn beside_a_picture(name: &str, source: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("markatui-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("a temporary directory");
    let path = directory.join("post.md");
    std::fs::write(&path, source).expect("a document");
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("sample/image.png");
    std::fs::copy(sample, directory.join("image.png")).expect("a picture beside it");
    path
}

/// The document and the picture beside it, once the test is through with them.
fn forget(path: &std::path::Path) {
    std::fs::remove_dir_all(path.parent().expect("a directory of its own"))
        .expect("the temporary directory goes");
}

fn sample_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("sample/post.md")
}

fn sample() -> Editor {
    let mut editor = Editor::open(&sample_path());
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

/// Draw frames until the picture has been read, which waits on a thread coming back, and
/// answer with the block it belongs to and what the last frame looked like.
fn until_the_picture_lands(
    editor: &Editor,
    document: &mut view::Document,
    gallery: &mut Gallery,
) -> (usize, Vec<String>) {
    for _ in 0..500 {
        let rows = drawn(editor, 100, 80, document, gallery);
        if let Some(placed) = document.pictures().first() {
            return (placed.index, rows);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the picture was never read");
}

/// The picture in the sample, drawn as the mosaic a terminal with no graphics protocol of
/// its own gets. It is read off the event loop, so the first frame is still the line
/// naming the file and a later one is the picture.
#[test]
fn draws_a_picture_once_it_has_been_read() {
    let editor = sample();
    let mut gallery = Gallery::new(Picker::halfblocks(), &sample_path());
    let mut document = view::Document::default();

    let (index, rows) = until_the_picture_lands(&editor, &mut document, &mut gallery);
    // The block has given up its line of words for rows of its own to be drawn in.
    assert_eq!(document.rows(index).len(), 12, "{rows:#?}");
    assert!(!rows.iter().any(|row| row.contains("image.png")), "{rows:#?}");
    // A mosaic, which is what a terminal with no protocol of its own draws.
    assert!(rows.iter().filter(|row| row.contains('▄')).count() >= 6, "{rows:#?}");
}

/// Stepping into the picture's block opens it up into markdown like any other, and
/// stepping out has the picture back on the very next frame: it was kept, not read again.
#[test]
fn keeps_the_picture_while_the_cursor_is_in_its_block() {
    let mut editor = sample();
    let mut gallery = Gallery::new(Picker::halfblocks(), &sample_path());
    let mut document = view::Document::default();
    let (index, _) = until_the_picture_lands(&editor, &mut document, &mut gallery);

    editor.activate(index, 0);
    let rows = drawn(&editor, 100, 80, &mut document, &mut gallery);
    assert!(rows.iter().any(|row| row.contains("![A picture](image.png)")), "{rows:#?}");
    assert!(document.pictures().is_empty(), "{rows:#?}");

    editor.activate(0, 0);
    let rows = drawn(&editor, 100, 80, &mut document, &mut gallery);
    assert_eq!(document.pictures().len(), 1, "{rows:#?}");
    assert!(rows.iter().filter(|row| row.contains('▄')).count() >= 6, "{rows:#?}");
}

/// The terminal holds a picture over the cells until they are written over, and a cell
/// that has not changed is never written: a frame that has moved a picture is sent whole,
/// or the one the terminal is holding stays on the screen where it was.
#[test]
fn sends_the_frame_whole_when_a_picture_moves() {
    let mut editor = sample();
    let mut gallery = Gallery::new(Picker::halfblocks(), &sample_path());
    let mut document = view::Document::default();
    let (index, _) = until_the_picture_lands(&editor, &mut document, &mut gallery);

    // The picture is where it was, so the frame is a difference like any other.
    let (_, resent) = drawn_and_resent(&editor, 100, 80, &mut document, &mut gallery);
    assert!(resent.is_empty(), "{resent:?}");

    // Stepping into the block gives the picture up for the markdown it was made from,
    // and every row goes to the terminal again — the rows it stood in among them.
    editor.activate(index, 0);
    let (_, resent) = drawn_and_resent(&editor, 100, 80, &mut document, &mut gallery);
    assert_eq!(resent, (0..80).collect::<Vec<_>>());

    // And the screen having been sent once, the frames after it are differences again.
    let (_, resent) = drawn_and_resent(&editor, 100, 80, &mut document, &mut gallery);
    assert!(resent.is_empty(), "{resent:?}");
}

/// A picture that arrives late pushes everything under it down the screen, and the window
/// goes down with it: the block being written in stays where the writer is looking.
#[test]
fn takes_the_rows_a_late_picture_adds_off_the_scroll() {
    let path = beside_a_picture("late-picture", "![A picture](image.png)\n\nThe words under it.\n");

    let mut editor = Editor::open(&path);
    editor.activate(1, 0);
    let mut gallery = Gallery::new(Picker::halfblocks(), &path);
    let mut document = view::Document::default();

    let mut shift = 0;
    for _ in 0..500 {
        drawn(&editor, 100, 40, &mut document, &mut gallery);
        shift = document.shift();
        if shift != 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Twelve rows of picture where there was one line naming the file.
    assert_eq!(shift, 11);
    forget(&path);
}

/// A picture the writer left among the words is drawn all the same: it is moved into a
/// paragraph of its own as the document opens, which is the only place a terminal has the
/// rows to draw one in.
#[test]
fn draws_a_picture_that_was_left_among_the_words() {
    let path = beside_a_picture("words-picture", "Words ![A picture](image.png) more.\n");
    let editor = Editor::open(&path);
    assert_eq!(editor.blocks().len(), 3, "the picture is a paragraph of its own");

    let mut gallery = Gallery::new(Picker::halfblocks(), &path);
    let mut document = view::Document::default();
    let (index, rows) = until_the_picture_lands(&editor, &mut document, &mut gallery);

    assert_eq!(index, 1);
    assert_eq!(document.rows(index).len(), 12, "{rows:#?}");
    assert!(rows.iter().filter(|row| row.contains('▄')).count() >= 6, "{rows:#?}");
    assert!(rows.iter().any(|row| row.contains("Words")), "{rows:#?}");
    assert!(rows.iter().any(|row| row.contains("more.")), "{rows:#?}");
    forget(&path);
}

/// Enter opens a line under the one being written and the caret goes down onto it there
/// and then, with nothing typed on it yet: it does not sit out past the last word as
/// though a space had been typed, waiting for the next keystroke to carry it over.
#[test]
fn takes_the_caret_down_to_the_line_enter_opened() {
    let path = std::env::temp_dir().join(format!("markatui-enter-{}.md", std::process::id()));
    let mut editor = Editor::open(&path);
    let mut document = view::Document::default();
    editor.insert("one");
    drawn(&editor, 40, 10, &mut document, &mut Gallery::blind());
    let (row, column) = document.caret().expect("the caret is in the block being typed in");
    assert_eq!(column, 3);

    editor.enter();
    drawn(&editor, 40, 10, &mut document, &mut Gallery::blind());
    assert_eq!(document.caret(), Some((row + 1, 0)));
}

/// A paragraph made into something else is drawn as that thing the moment the cursor
/// leaves it: the marker at the head of the line is what the block is, and nothing has to
/// be told about the change twice.
#[test]
fn draws_a_paragraph_the_key_turned_into_a_heading_a_list_and_a_quote() {
    let path = written("marks", "Title\n\none\ntwo\n\nquoted");
    let mut editor = Editor::open(&path);

    editor.activate(0, 0);
    editor.mark(Mark::Heading(2));
    editor.activate(1, 0);
    editor.move_cursor(Motion::Block(1), true);
    editor.mark(Mark::Bullet);
    editor.activate(2, 0);
    editor.mark(Mark::Quote);
    editor.activate(0, 0);

    assert_eq!(editor.source(), "## Title\n\n- one\n- two\n\n> quoted");
    let rows = screen_of(&editor, 60, 20);
    assert!(rows.iter().any(|row| row.trim() == "\u{2022} one"), "{rows:#?}");
    assert!(rows.iter().any(|row| row.trim() == "\u{2022} two"), "{rows:#?}");
    assert!(rows.iter().any(|row| row.contains("\u{258e} quoted")), "{rows:#?}");
    forget(&path);
}

/// The one thing a column's alignment does not change: the file says which way the column
/// reads, and the terminal goes on drawing every cell the same way.
#[test]
fn sets_a_column_in_the_file_and_draws_the_table_as_it_always_did() {
    let path = written("align", "| a | b |\n| --- | --- |\n| 1 | 2 |\n\nwords");
    let mut editor = Editor::open(&path);

    editor.activate(0, 2);
    editor.align(Align::Centre);
    assert!(editor.block(0).contains("| :---: | --- |"), "{}", editor.block(0));

    editor.activate(1, 0);
    let rows = screen_of(&editor, 60, 20);
    assert!(
        rows.iter().any(|row| row.trim()
            == "\u{250c}\u{2500}\u{2500}\u{2500}\u{252c}\u{2500}\u{2500}\u{2500}\u{2510}"),
        "{rows:#?}"
    );
    assert!(rows.iter().any(|row| row.trim() == "\u{2502} a \u{2502} b \u{2502}"), "{rows:#?}");
    forget(&path);
}

/// A document on disk for a test to open, in a directory of its own so that [`forget`]
/// can take the whole of it away again.
fn written(name: &str, source: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("markatui-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("a temporary directory");
    let path = directory.join("post.md");
    std::fs::write(&path, source).expect("a document to open");
    path
}

/// The same as [`screen`], for a document a test built rather than the sample.
fn screen_of(editor: &Editor, width: u16, height: u16) -> Vec<String> {
    drawn(editor, width, height, &mut view::Document::default(), &mut Gallery::blind())
}
