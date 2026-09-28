use super::*;
use ratatui::backend::TestBackend;
use ratatui::{TerminalOptions, Viewport};
use ratatui_image::picker::Picker;
use std::sync::{Arc, Mutex};

/// A document of its own, so that a test writing into it disturbs nothing else, with
/// the sample pictures beside it for the block that names one: the still and the gif.
fn document(name: &str, source: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("markatui-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("a temporary directory");
    let path = directory.join("post.md");
    std::fs::write(&path, source).expect("a document");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for picture in ["image.png", "loop.gif"] {
        std::fs::copy(fixtures.join(picture), directory.join(picture))
            .expect("a picture beside it");
    }
    path
}

/// A document three rows down from the top of a fifteen-row screen with twelve rows of
/// picture under it: the last row of the picture and the last row of the screen are
/// the same row, which is the one place a picture must not be drawn.
const TO_THE_BOTTOM: &str = "# Title\n\n![A picture](image.png)\n";

/// The picture the fixture document names, as the bytes a paste puts on the clipboard.
fn sample_png() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/image.png"))
        .expect("the sample picture")
}

/// The document and the picture beside it, once the test is through with them.
fn forget(path: &Path) {
    std::fs::remove_dir_all(path.parent().expect("a directory of its own"))
        .expect("the temporary directory goes");
}

/// The editor on the document at `path`, drawing pictures as the mosaic a test can
/// make: there is no terminal here to ask for a protocol of its own.
fn app(path: &Path) -> App {
    let mut app = App::open(path, Gallery::new(Picker::halfblocks(), path), Keyboard::Kitty);
    // Wherever the last session left the cursor and whichever mode it was left in, a
    // test starts at the top of the document in the plain one.
    app.editor.activate(0, 0);
    app.set_mode("plain");
    app
}

/// The first Enter breaks the line inside the block and the second ends it, whatever
/// the terminal can tell apart. Shift+Enter is the line break on its own, for the
/// blocks where Enter has something else to do.
#[test]
fn breaks_the_line_on_enter_and_ends_the_block_on_the_second_press() {
    let path = document("enter", "one two");
    let mut app = app(&path);
    app.editor.activate(0, 3);
    app.act(Action::Enter);
    assert_eq!(app.editor.block(0), "one\n two");
    app.act(Action::Enter);
    assert_eq!(app.editor.blocks().len(), 2);
    assert_eq!(app.editor.block(1), " two");

    // A terminal that cannot tell Shift+Enter from Enter gets the same rule, because
    // it is the only rule there is now.
    let mut legacy = App::open(&path, Gallery::new(Picker::halfblocks(), &path), Keyboard::Legacy);
    legacy.set_mode("plain");
    legacy.editor.activate(0, 3);
    legacy.act(Action::Enter);
    assert_eq!(legacy.editor.block(0), "one\n two");
    legacy.act(Action::Enter);
    assert_eq!(legacy.editor.blocks().len(), 2);

    let mut shifted = App::open(&path, Gallery::new(Picker::halfblocks(), &path), Keyboard::Kitty);
    shifted.set_mode("plain");
    shifted.editor.activate(0, 3);
    shifted.act(Action::LineBreak);
    assert_eq!(shifted.editor.block(0), "one\n two");
    forget(&path);
}

/// Control with the arrows walks the blocks while the checker is off, the way control
/// with Left and Right walks the words.
#[test]
fn walks_the_blocks_on_control_with_the_arrows_while_the_checker_is_off() {
    let path = document("cycle", "alpha\n\nbeta\n\ngamma");
    let mut app = app(&path);
    app.set_mode("grammar-off");
    app.editor.activate(0, 2);

    app.act(Action::CycleLint(1));
    assert_eq!((app.editor.index(), app.editor.active().cursor()), (0, 5));
    app.act(Action::CycleLint(1));
    assert_eq!((app.editor.index(), app.editor.active().cursor()), (1, 4));
    app.act(Action::CycleLint(-1));
    assert_eq!((app.editor.index(), app.editor.active().cursor()), (1, 0));
    forget(&path);
}

/// The checker being on does not turn the arrows into line keys: with no suggestion
/// to walk, control with them walks the blocks, the same as when the checker is off.
#[test]
fn walks_the_blocks_on_control_with_the_arrows_while_the_checker_is_on() {
    let path = document("cycle-on", "alpha\nbeta\n\ngamma");
    let mut app = app(&path);
    app.grammar = true;
    app.editor.lint.replacements.clear();
    app.editor.activate(0, 2);

    app.act(Action::CycleLint(1));
    assert_eq!((app.editor.index(), app.editor.active().cursor()), (0, 10));
    app.act(Action::CycleLint(1));
    assert_eq!((app.editor.index(), app.editor.active().cursor()), (1, 5));
    forget(&path);
}

/// Tab is the document's as well as the search bar's: a level of indent in a list,
/// and the cell along in a table.
#[test]
fn takes_tab_to_the_block_the_cursor_is_in() {
    let path = document("tab", "- one\n- two");
    let mut app = app(&path);
    app.editor.activate(0, 8);
    app.act(Action::Tab(1));
    assert_eq!(app.editor.block(0), "- one\n  - two");
    app.act(Action::Tab(-1));
    assert_eq!(app.editor.block(0), "- one\n- two");
    forget(&path);
}

/// One frame, as one string per row with the trailing spaces off.
fn frame(app: &mut App, terminal: &mut Terminal<TestBackend>) -> Vec<String> {
    terminal.draw(|frame| app.draw(frame)).expect("a frame");
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| {
            let row: String =
                (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect();
            row.trim_end().to_string()
        })
        .collect()
}

/// What the terminal is sent, kept in the order it arrives: escapes, cells and all.
/// A screen buffer says what a frame came to; this says how it got there, which is
/// where the caret being left on the screen shows up.
#[derive(Clone, Default)]
struct Wire(Arc<Mutex<Vec<u8>>>);

impl Wire {
    fn clear(&self) {
        self.0.lock().expect("the wire").clear();
    }

    fn sent(&self) -> Vec<u8> {
        self.0.lock().expect("the wire").clone()
    }
}

impl io::Write for Wire {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().expect("the wire").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Taking the caret off the screen, and putting it back.
const HIDE: &[u8] = b"\x1b[?25l";
const SHOW: &[u8] = b"\x1b[?25h";

fn at(sent: &[u8], escape: &[u8]) -> Option<usize> {
    sent.windows(escape.len()).position(|window| window == escape)
}

/// Frames until the picture has been read, which waits on a thread coming back.
fn until_the_picture_lands(app: &mut App, terminal: &mut Terminal<TestBackend>) -> Vec<String> {
    for _ in 0..500 {
        let rows = frame(app, terminal);
        if !app.document.pictures().is_empty() {
            return rows;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the picture was never read");
}

/// A frame does not arrive all at once, and the caret is the terminal's own: it sits
/// wherever the writing has got to. A screenful resent under a picture is long enough
/// to be written in pieces, so leaving the caret on through one is the writer watching
/// it wander off and come back — which is what a gif does every time it turns.
///
/// So: the whole of a frame is one synchronized update, nothing of it goes out while
/// the caret is on the screen, and the caret comes back — inside the update still —
/// only once the last of the frame has gone.
#[test]
fn keeps_the_caret_off_the_screen_while_a_frame_is_written() {
    let path = document("caret", "moving\n\n![a picture](image.png)");
    let mut app = app(&path);
    let wire = Wire::default();
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(wire.clone()),
        TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, 80, 20)) },
    )
    .expect("a test screen");

    // Up to the frame that first draws the picture, which is the frame that resends
    // the screen — the long one, and the one the caret was seen wandering through.
    for _ in 0..500 {
        wire.clear();
        app.send(&mut terminal).expect("a frame");
        if !app.document.pictures().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!app.document.pictures().is_empty(), "the picture was never read");
    let sent = wire.sent();
    forget(&path);

    assert!(sent.starts_with(OPEN), "the frame does not open the update it is sent in");
    assert!(sent.ends_with(CLOSE), "the frame does not close it");
    assert_eq!(
        at(&sent, HIDE),
        Some(OPEN.len()),
        "the caret is off before a byte of the frame goes out"
    );
    let shown = at(&sent, SHOW).expect("and back on once it has gone");
    // What is left after it comes back is putting it where it belongs and closing the
    // update, and nothing else: anything more would be drawn with the caret on it.
    let tail = &sent[shown + SHOW.len()..sent.len() - CLOSE.len()];
    assert!(tail.starts_with(b"\x1b["), "only the caret being put back comes after");
    assert!(tail.len() <= 11, "{} bytes drawn with the caret on them", tail.len());
}

/// Opening and closing a synchronized update: between the two the terminal keeps
/// showing what it last presented, whatever it is being sent.
const OPEN: &[u8] = b"\x1b[?2026h";
const CLOSE: &[u8] = b"\x1b[?2026l";

/// What the caret did on the screen, in order, starting from the screen being entered
/// with it on. A caret taken off and put back inside one synchronized update never
/// reaches the screen at all, so it is not in here; one taken off outside of one is,
/// and every `false` in what comes back is a blink the writer saw.
fn caret_on_the_screen(sent: &[u8]) -> Vec<bool> {
    let mut screen = vec![true];
    let mut within = false;
    let mut held = true;
    let mut at = 0;
    while at < sent.len() {
        let rest = &sent[at..];
        let (step, caret) = if rest.starts_with(OPEN) {
            (held, within) = (*screen.last().expect("the screen starts somewhere"), true);
            (OPEN.len(), None)
        } else if rest.starts_with(CLOSE) {
            within = false;
            (CLOSE.len(), Some(held))
        } else if rest.starts_with(HIDE) {
            (HIDE.len(), Some(false))
        } else if rest.starts_with(SHOW) {
            (SHOW.len(), Some(true))
        } else {
            (1, None)
        };
        match caret {
            Some(caret) if within => held = caret,
            Some(caret) if caret != *screen.last().expect("the screen starts somewhere") => {
                screen.push(caret);
            }
            _ => {}
        }
        at += step;
    }
    screen
}

/// A legacy terminal does not hold synchronized updates. Its native caret stays off
/// while frames are sent so it cannot be watched following the renderer, and a drawn
/// caret stands in for it without blinking off and on between frames.
#[test]
fn keeps_the_native_caret_hidden_where_updates_are_not_synchronized() {
    let path = document("legacy-caret", "A paragraph with nothing happening to it.\n");
    let mut app = App::open(&path, Gallery::new(Picker::halfblocks(), &path), Keyboard::Legacy);
    let wire = Wire::default();
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(wire.clone()),
        TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, 80, 20)) },
    )
    .expect("a test screen");

    for _ in 0..10 {
        app.send(&mut terminal).expect("a frame");
    }
    let sent = wire.sent();
    forget(&path);

    assert_eq!(caret_on_the_screen(&sent), vec![true, false]);
    assert_eq!(at(&sent, SHOW), None, "a frame showed the native caret");
    assert_eq!(at(&sent, OPEN), None, "an unsupported synchronized update was opened");
}

/// A terminal's own caret is a block that swaps the two colours of the cell it stands
/// on, and a selected cell is already those two colours swapped: the character the
/// caret is against comes back out in the page's own colours and reads as the one
/// character of the selection that was left out of it. So the caret comes off while a
/// selection is running, and the selection is left whole.
#[test]
fn leaves_the_native_caret_off_a_selected_character() {
    let path = document("selected-caret", "A paragraph with a selection in it.\n");
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("a test screen");
    app.editor.activate(0, 10);
    app.editor.move_cursor(Motion::LineEdge(-1), true);

    frame(&mut app, &mut terminal);
    let (row, column) = app.document.caret().expect("a drawn caret");
    let cell = &terminal.backend().buffer()
        [(app.column.x + column, row.saturating_sub(app.scroll) as u16)];
    let selected = cell.style().add_modifier.contains(ratatui::style::Modifier::REVERSED);
    let native = terminal.backend().cursor_visible();
    forget(&path);

    assert!(selected, "the first character of the selection was not painted as selected");
    assert!(!native, "the terminal's caret was left standing on a selected character");
}

#[test]
fn paints_a_caret_where_the_native_one_stays_hidden() {
    let path = document("painted-caret", "A paragraph.\n");
    let mut app = App::open(&path, Gallery::new(Picker::halfblocks(), &path), Keyboard::Legacy);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("a test screen");

    frame(&mut app, &mut terminal);
    let (row, column) = app.document.caret().expect("a drawn caret");
    let caret = &terminal.backend().buffer()
        [(app.column.x + column, row.saturating_sub(app.scroll) as u16)];
    forget(&path);

    assert!(
        caret.style().add_modifier.contains(ratatui::style::Modifier::REVERSED),
        "no caret was painted into the frame"
    );
}

/// Every frame goes out with the caret off and ends with ratatui putting it back, and
/// a frame goes out every time the loop looks up from the keyboard — four times a
/// second with nobody touching it. Off and on again, four times a second, is the
/// caret blinking at a writer who is doing nothing at all.
#[test]
fn does_not_blink_the_caret_while_nothing_is_happening() {
    let path = document("still-caret", "A paragraph with nothing happening to it.\n");
    let mut app = app(&path);
    let wire = Wire::default();
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(wire.clone()),
        TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, 80, 20)) },
    )
    .expect("a test screen");

    for _ in 0..10 {
        app.send(&mut terminal).expect("a frame");
    }
    let screen = caret_on_the_screen(&wire.sent());
    forget(&path);

    assert_eq!(screen, vec![true], "the caret went off the screen and came back");
}

/// The same, with a gif on the screen, which is where it is worst: a turn resends the
/// screenful under the picture, so the frames are long as well as often — ten a
/// second, each one taking the caret off and giving it back.
#[test]
fn does_not_blink_the_caret_while_a_gif_turns() {
    let path = document("gif-caret", "Words\n\n![a gif](loop.gif)\n");
    let mut app = app(&path);
    let wire = Wire::default();
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(wire.clone()),
        TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, 80, 20)) },
    )
    .expect("a test screen");

    for _ in 0..500 {
        app.send(&mut terminal).expect("a frame");
        if !app.document.pictures().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!app.document.pictures().is_empty(), "the gif was never read");
    // Long enough for it to turn several times, at a frame every twentieth of a
    // second, which is about what a writer leaning on nothing gets.
    wire.clear();
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(50));
        app.send(&mut terminal).expect("a frame");
    }
    let sent = wire.sent();
    let screen = caret_on_the_screen(&sent);
    forget(&path);

    // A turn resends the screenful the picture stands in, which is the long frame the
    // caret was seen wandering through. Without one this has tested the short frames
    // twenty times over and the frame that matters not at all.
    assert!(sent.len() > 2_000, "the gif never turned: {} bytes over 20 frames", sent.len());
    assert_eq!(screen, vec![true], "the caret went off the screen and came back");
}

#[test]
fn page_keys_scroll_and_move_the_cursor() {
    let source = (0..20).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n\n");
    let path = document("pages", &source);
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 9)).expect("a test screen");
    frame(&mut app, &mut terminal);

    app.page(1, false);
    frame(&mut app, &mut terminal);
    assert!(app.scroll > 0, "PageDown left the first screenful in place");
    assert!(app.editor.index() > 0, "PageDown left the cursor in place");

    app.page(-1, false);
    frame(&mut app, &mut terminal);
    assert_eq!(app.scroll, 0);
    assert_eq!(app.editor.index(), 0);
    forget(&path);
}

/// Shift with a page key draws a selection over everything the caret travels past,
/// the way shift with an arrow does, and a selection already running grows rather
/// than being thrown away and started again where the caret happened to stand.
#[test]
fn shift_with_a_page_key_draws_the_selection_with_it() {
    let source = (0..20).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n\n");
    let path = document("page-selection", &source);
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 9)).expect("a test screen");
    frame(&mut app, &mut terminal);

    // A selection of the first four characters, to be carried along by the page key.
    app.act(Action::Move(Motion::LineEdge(1), true));
    assert_eq!(app.editor.selected_text(), "line 0");

    app.act(Action::Page(1, true));
    frame(&mut app, &mut terminal);
    let selected = app.editor.selected_text();
    assert!(selected.starts_with("line 0"), "the selection was started again: {selected:?}");
    assert!(selected.contains("line 1"), "nothing between the two ends: {selected:?}");
    assert!(app.editor.index() > 0, "the caret never left the first block");

    // A second press grows the same selection, and a press back up shrinks it to
    // what is left, rather than either of them starting one afresh.
    app.act(Action::Page(1, true));
    frame(&mut app, &mut terminal);
    let further = app.editor.selected_text();
    assert!(further.starts_with("line 0"), "the selection was started again: {further:?}");
    assert!(further.len() > selected.len(), "the second page took in nothing more");

    app.act(Action::Page(-1, true));
    frame(&mut app, &mut terminal);
    assert_eq!(app.editor.selected_text(), selected, "paging back did not shrink it");

    // Without shift the page key leaves nothing selected.
    app.act(Action::Page(1, false));
    assert_eq!(app.editor.selected_text(), "");
    forget(&path);
}

#[test]
fn page_keys_scroll_from_an_image_in_reading_mode() {
    let source = format!(
        "![A picture](image.png)\n\n{}",
        (0..20).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n\n")
    );
    let path = document("image-pages", &source);
    let mut app = app(&path);
    app.set_mode("reading");
    let mut terminal = Terminal::new(TestBackend::new(90, 9)).expect("a test screen");
    until_the_picture_lands(&mut app, &mut terminal);
    assert_eq!(app.document.caret(), None);

    app.page(1, false);
    frame(&mut app, &mut terminal);
    assert!(app.scroll > 0, "PageDown left the image in place");

    app.page(-1, false);
    frame(&mut app, &mut terminal);
    assert_eq!(app.scroll, 0, "PageUp left the image scrolled");
    forget(&path);
}

/// A picture drawn on the very last row of the terminal makes a sixel terminal scroll,
/// and the row that goes off the top never comes back. The foot of the screen is a
/// band whether or not it has anything to say, so nothing of the document — least of
/// all a picture — is ever drawn there.
#[test]
fn keeps_the_last_row_of_the_screen_for_the_foot() {
    let path = document("bottom", TO_THE_BOTTOM);
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 15)).expect("a test screen");
    let rows = until_the_picture_lands(&mut app, &mut terminal);

    assert!(rows.iter().filter(|row| row.contains('▄')).count() >= 6, "{rows:#?}");
    assert!(!rows[14].contains('▄'), "the picture ran into the foot of the screen: {rows:#?}");
    forget(&path);
}

#[test]
fn gives_a_long_explanation_enough_rows() {
    let path = document("long-message", "A document.\n");
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(24, 10)).expect("a test screen");
    app.editor.lint.message =
        "This grammar explanation needs enough room to reach its final word: visible".into();

    let rows = frame(&mut app, &mut terminal);

    assert!(rows.iter().any(|row| row.contains("visible")), "{rows:#?}");
    assert!(app.viewport <= 6, "only one row was given to the explanation");
    forget(&path);
}

#[test]
fn reading_mode_renders_the_active_block_and_disables_grammar() {
    let path = document("reading", "# A **heading**\n");
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("a test screen");

    assert!(app.grammar);
    app.act(Action::ToggleReading);
    let rows = frame(&mut app, &mut terminal);

    assert!(app.reading);
    assert!(!app.grammar);
    assert!(rows.iter().any(|row| row.trim() == "A heading"), "{rows:#?}");
    assert!(!rows.iter().any(|row| row.contains('#') || row.contains("**")), "{rows:#?}");
    forget(&path);
}

/// The three modes and the word each is written down as, both ways round.
#[test]
fn puts_every_mode_into_one_word_and_back() {
    let path = document("mode-words", "A document.\n");
    let mut app = app(&path);

    assert_eq!(app.mode_word(), "plain");
    app.act(Action::ToggleGrammar);
    assert_eq!(app.mode_word(), "grammar-off");
    app.act(Action::ToggleReading);
    assert_eq!(app.mode_word(), "reading");

    for word in ["reading", "grammar-off", "plain"] {
        app.set_mode(word);
        assert_eq!(app.mode_word(), word);
    }
    // A word from a store written by some other version is no reason to start oddly.
    app.set_mode("nonsense");
    assert_eq!(app.mode_word(), "plain");
    forget(&path);
}

/// The mode outlives the run: the editor opens the way the last one was left, rather
/// than in whichever mode happens to be first.
#[test]
fn opens_in_the_mode_the_last_run_was_left_in() {
    let path = document("remembered-mode", "A document.\n");
    let held = storage::recall_mode();

    let mut left = app(&path);
    left.act(Action::ToggleReading);
    left.remember();
    let opened = App::open(&path, Gallery::new(Picker::halfblocks(), &path), Keyboard::Kitty);
    assert_eq!(opened.mode_word(), "reading");

    let mut left = app(&path);
    left.act(Action::ToggleGrammar);
    left.remember();
    let opened = App::open(&path, Gallery::new(Picker::halfblocks(), &path), Keyboard::Kitty);
    assert_eq!(opened.mode_word(), "grammar-off");

    if let Some(held) = held {
        storage::remember_mode(&held);
    }
    forget(&path);
}

/// The one paste key takes whatever the clipboard was holding. Words are typed in
/// where the cursor is, the same as a terminal's own paste.
/// The bar is two halves and Tab is what moves between them: what is typed goes into
/// the half the writer is in, and only the word being looked for is looked for.
#[test]
fn types_into_the_half_of_the_search_bar_the_writer_is_in() {
    let path = document("swap", "a cat and a cat");
    let mut app = app(&path);
    app.act(Action::OpenSearch);
    for letter in "cat".chars() {
        app.act(Action::Type(letter.to_string()));
    }
    assert_eq!(app.editor.search.count, 2);

    app.act(Action::Tab(1));
    for letter in "dog".chars() {
        app.act(Action::Type(letter.to_string()));
    }
    assert_eq!(app.editor.search.needle, "cat");
    assert_eq!(app.editor.search.replacement, "dog");

    // Enter in this half is the swap, not the way out of the bar.
    app.act(Action::Enter);
    assert_eq!(app.editor.block(0), "a dog and a cat");
    assert!(app.mode == Mode::Searching, "Enter closed the bar instead of swapping");
    app.act(Action::ReplaceAll);
    assert_eq!(app.editor.block(0), "a dog and a dog");

    app.act(Action::Tab(1));
    app.act(Action::Enter);
    assert!(app.mode == Mode::Editing, "Enter left the bar open");
    forget(&path);
}

/// A search is one piece of work: the bar comes up empty however much was typed into
/// it last time, and the keys under it are the keys the half being typed into
/// answers to.
#[test]
fn opens_the_search_bar_with_nothing_left_in_it() {
    let path = document("fresh", "a cat and a cat");
    let mut app = app(&path);
    app.act(Action::OpenSearch);
    for letter in "cat".chars() {
        app.act(Action::Type(letter.to_string()));
    }
    app.act(Action::Tab(1));
    for letter in "dog".chars() {
        app.act(Action::Type(letter.to_string()));
    }
    app.act(Action::CloseSearch);

    app.act(Action::OpenSearch);
    assert_eq!(app.editor.search.needle, "");
    assert_eq!(app.editor.search.replacement, "");
    assert_eq!(app.editor.search.count, 0);

    let footer = app.footer(90);
    assert!(footer[0].starts_with("FIND    _"), "{footer:#?}");
    assert!(footer[1].starts_with("REPLACE "), "{footer:#?}");
    assert!(footer[2].contains("ctrl+↓ next"), "{footer:#?}");

    // The other half answers to other keys, and says so once the caret is in it.
    app.act(Action::Tab(1));
    let footer = app.footer(90);
    assert!(footer[2].contains("enter this one"), "{footer:#?}");
    assert!(!footer[2].contains("next"), "{footer:#?}");
    forget(&path);
}

/// Cut is copy and delete in one. With nothing selected there is nothing to cut, and
/// the key leaves the document where it was rather than eating a character.
#[test]
fn cuts_only_what_is_selected() {
    let path = document("cut", "one two");
    let mut app = app(&path);
    app.act(Action::Cut);
    assert_eq!(app.editor.block(0), "one two");

    app.act(Action::Move(crate::editor::Motion::Word(1), true));
    app.act(Action::Cut);
    assert_eq!(app.editor.block(0), " two");
    forget(&path);
}

#[test]
fn pastes_the_words_on_the_clipboard() {
    let path = document("paste-words", "A document.");
    let mut app = app(&path);

    app.paste_content(Paste::Words("Words. ".into()));

    assert_eq!(app.editor.block(0), "Words. A document.");
    forget(&path);
}

/// And a picture, which markdown cannot hold, becomes a file beside the document and
/// a line naming it, with the cursor between the brackets for its description.
#[test]
fn pastes_the_picture_on_the_clipboard() {
    let path = document("paste-picture", "A document.");
    let mut app = app(&path);
    let png = sample_png();

    app.paste_content(Paste::Picture(png.clone()));

    assert_eq!(app.editor.block(0), "![](post-1.png)A document.");
    assert_eq!(app.editor.active().cursor(), 2, "the cursor is not where the words go");
    let beside = path.parent().expect("a directory of its own").join("post-1.png");
    assert_eq!(std::fs::read(beside).ok(), Some(png), "the picture was not written");
    forget(&path);
}

#[test]
fn discarding_a_pasted_picture_removes_its_file() {
    let path = document("discard-picture", "A document.");
    let mut app = app(&path);
    let png = sample_png();
    app.paste_content(Paste::Picture(png));
    let picture = path.parent().unwrap().join("post-1.png");
    assert!(picture.exists());

    app.discard_pictures();

    assert!(!picture.exists());
    forget(&path);
}

#[test]
fn saving_without_the_pasted_reference_removes_the_file() {
    let path = document("remove-picture", "A document.");
    let mut app = app(&path);
    let png = sample_png();
    app.paste_content(Paste::Picture(png));
    let picture = path.parent().unwrap().join("post-1.png");
    app.act(Action::Undo);

    assert!(app.save());
    assert!(!picture.exists());
    forget(&path);
}

#[test]
fn saving_a_renamed_picture_reference_renames_the_file() {
    let path = document("rename-picture", "A document.");
    let mut app = app(&path);
    let png = sample_png();
    app.paste_content(Paste::Picture(png.clone()));
    for _ in 0..2 {
        app.act(Action::Move(crate::editor::Motion::Character(1), false));
    }
    for _ in 0..10 {
        app.act(Action::Move(crate::editor::Motion::Character(1), true));
    }
    app.act(Action::Type("better-name.png".into()));

    assert!(app.save());

    let directory = path.parent().unwrap();
    assert!(!directory.join("post-1.png").exists());
    assert_eq!(std::fs::read(directory.join("better-name.png")).unwrap(), png);
    forget(&path);
}

/// A clipboard with nothing on it is a key that does nothing, not an empty paste.
#[test]
fn pastes_nothing_from_an_empty_clipboard() {
    let path = document("paste-nothing", "A document.");
    let mut app = app(&path);

    app.paste_content(Paste::Nothing);

    assert_eq!(app.editor.block(0), "A document.");
    assert!(!app.editor.dirty(), "an empty paste counted as an edit");
    forget(&path);
}

/// A picture left among the words is moved into a paragraph of its own as the
/// document opens — a terminal has nowhere to draw one inside a line — and the foot of
/// the screen says so until the next keystroke.
#[test]
fn says_when_a_picture_was_moved_out_of_the_words() {
    let path = document("inline-picture", "Words ![A picture](image.png) more.\n");
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 12)).expect("a test screen");

    assert_eq!(app.footer(90), [HOISTED.to_string()]);
    let rows = frame(&mut app, &mut terminal);
    assert!(rows.iter().any(|row| row.contains(HOISTED)), "{rows:#?}");
    assert_eq!(app.editor.block(1), "![A picture](image.png)");

    // Read and gone: the writer touches a key and the band is the document's again.
    app.press(event::KeyEvent::new(event::KeyCode::Right, event::KeyModifiers::NONE));
    assert!(app.footer(90).is_empty());
    forget(&path);
}

/// A key coming back up is the same keystroke over again: a terminal that reports
/// releases must not type every letter twice.
#[test]
fn lets_a_key_release_alone() {
    let path = document("key-release", "Words.\n");
    let mut app = app(&path);

    let mut key = event::KeyEvent::new(event::KeyCode::Char('x'), event::KeyModifiers::NONE);
    app.press(key);
    key.kind = event::KeyEventKind::Release;
    app.press(key);

    assert_eq!(app.editor.block(0), "xWords.");
    forget(&path);
}

/// And a document with nothing to move says nothing.
#[test]
fn says_nothing_where_no_picture_had_to_move() {
    let path = document("no-inline-picture", "![A picture](image.png)\n\nWords.\n");
    let app = app(&path);

    assert!(app.footer(90).is_empty());
    forget(&path);
}

#[test]
fn says_which_mode_the_writer_is_in() {
    let path = document("modes", "A document.\n");
    let mut app = app(&path);

    assert!(app.footer(40).is_empty(), "the checker has said nothing yet");
    app.act(Action::ToggleGrammar);
    assert_eq!(app.footer(40), vec!["GRAMMAR OFF".to_string()]);
    let mut terminal = Terminal::new(TestBackend::new(90, 5)).expect("a test screen");
    assert!(frame(&mut app, &mut terminal)[4].starts_with("GRAMMAR OFF"));

    app.act(Action::ToggleReading);
    assert_eq!(app.footer(40), vec!["READING".to_string()]);
    forget(&path);
}

/// A check is not turned off by one key: the config is the writer's own, and a rule
/// gone by a mis-hit key is one they would never think to look for again.
#[test]
fn asks_before_it_turns_a_check_off() {
    let path = document("muting", "A document.\n");
    let mut app = app(&path);
    app.grammar = true;

    // The checker has objected to nothing, so there is nothing to ask about.
    app.act(Action::MuteCheck);
    assert!(app.mode == Mode::Editing);

    app.editor.lint.rule = "UseTitleCase".to_string();
    app.act(Action::MuteCheck);
    assert_eq!(app.footer(40), ["Never show UseTitleCase again?", "[y] [n] [esc]"]);

    // Saying no leaves the check running and writes nothing.
    app.act(Action::Cancel);
    assert!(app.mode == Mode::Editing);
    forget(&path);
}

#[test]
fn gives_a_long_search_enough_rows() {
    let path = document("long-search", "A document.\n");
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(24, 10)).expect("a test screen");
    app.mode = Mode::Searching;
    app.editor.search.needle = "an-unusually-long-search-term".into();

    let rows = frame(&mut app, &mut terminal);

    assert!(rows.iter().any(|row| row.starts_with("FIND")), "{rows:#?}");
    assert!(rows.iter().any(|row| row.contains("no matches")), "{rows:#?}");
    assert!(app.viewport <= 7, "only one row was given to the search");
    forget(&path);
}

/// The checker having something to say does not take a row off the document: the foot
/// was already there, and the rows above it are the rows they were.
#[test]
fn says_its_piece_without_moving_the_document() {
    let path = document("message", TO_THE_BOTTOM);
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 15)).expect("a test screen");
    let quiet = until_the_picture_lands(&mut app, &mut terminal);
    let viewport = app.viewport;

    app.editor.error = Some("Something to say".into());
    let spoken = frame(&mut app, &mut terminal);
    assert_eq!(app.viewport, viewport, "the document lost a row to the message");
    assert_eq!(quiet[..14], spoken[..14], "the document moved under the message");
    assert!(spoken[14].contains("Something to say"), "{spoken:#?}");

    // And with the message gone the foot is blank again, the document still where it was.
    app.editor.error = None;
    assert_eq!(frame(&mut app, &mut terminal), quiet);
    forget(&path);
}
/// A document with a link in its second block, drawn on a screen wide enough for the
/// whole column: the paragraph reads "A paragraph with a link in it." with the link's
/// words at columns 17 to 22.
const WITH_A_LINK: &str = "# Title\n\nA paragraph with [a link](https://example.com/x) in it.\n";

/// A line that fills the column has nothing left of it for the caret to stand on:
/// the place past its last character is the first column of the margin. That is where
/// the next character would go and still a place on the screen, so the caret goes
/// there — pressing Up onto such a line is not a keystroke that loses the caret.
#[test]
fn keeps_the_caret_on_a_line_that_fills_the_column() {
    let width = theme::content_width();
    let filled = "x".repeat(width as usize);
    let path = document("filled-line", &format!("{filled}\n\n## Install\n"));
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 24)).expect("a test screen");
    app.editor.activate(1, 0);
    frame(&mut app, &mut terminal);

    app.act(Action::Row(-1, false));
    frame(&mut app, &mut terminal);
    let (row, column) = app.document.caret().expect("a caret at the end of the line");
    let position = terminal.backend().cursor_position();
    forget(&path);

    assert_eq!(app.editor.index(), 0, "Up did not go to the line above");
    assert_eq!(column, width, "the caret is not past the last character of the line");
    assert!(terminal.backend().cursor_visible(), "the caret went off the screen");
    assert_eq!(
        position,
        ratatui::layout::Position::new(app.column.x + width, (row - app.scroll) as u16),
        "the caret is not where the next character would go"
    );
}

fn pointer(kind: event::MouseEventKind, column: u16, row: u16) -> event::MouseEvent {
    event::MouseEvent { kind, column, row, modifiers: event::KeyModifiers::NONE }
}

fn left_click(column: u16, row: u16) -> event::MouseEvent {
    pointer(event::MouseEventKind::Down(event::MouseButton::Left), column, row)
}

fn ctrl_click(column: u16, row: u16) -> event::MouseEvent {
    event::MouseEvent { modifiers: event::KeyModifiers::CONTROL, ..left_click(column, row) }
}

#[test]
fn a_click_puts_the_caret_where_it_landed() {
    let path = document("click", WITH_A_LINK);
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 12)).expect("a test screen");
    frame(&mut app, &mut terminal);

    // The paragraph is the second block: a row of air above the heading, the heading,
    // and a row between the two.
    app.point(left_click(app.column.x + 5, 3));

    assert_eq!(app.editor.index(), 1);
    assert_eq!(app.editor.active().cursor(), 5);
    forget(&path);
}

/// A click in the air between two blocks, and one below the last of them, land in the
/// document rather than nowhere.
#[test]
fn a_click_off_the_text_lands_on_the_nearest_row() {
    let path = document("click-air", WITH_A_LINK);
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 12)).expect("a test screen");
    frame(&mut app, &mut terminal);

    app.point(left_click(app.column.x, 2));
    assert_eq!(app.editor.index(), 0, "the row between the blocks belongs to the one above");

    app.point(left_click(app.column.x, 9));
    assert_eq!(app.editor.index(), 1, "a click below the document missed the last block");
    forget(&path);
}

/// A plain click on a link is a click in the words: the caret goes into the link,
/// which is where a writer who means to edit it wants it — and Ctrl+K from there
/// follows it, the same as ever.
#[test]
fn a_plain_click_on_a_link_puts_the_caret_in_it() {
    let path = document("click-link", WITH_A_LINK);
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 12)).expect("a test screen");
    frame(&mut app, &mut terminal);

    app.point(left_click(app.column.x + 18, 3));

    assert_eq!(app.editor.index(), 1);
    assert_eq!(
        app.editor.link_at_cursor().as_deref(),
        Some("https://example.com/x"),
        "the caret did not land in the link"
    );
    assert_eq!(app.clicked_link(left_click(app.column.x + 18, 3)), None);
    forget(&path);
}

/// Ctrl with the click is how a writer says they meant the link rather than the words
/// of it, wherever the pointer is on it: the words of a block drawn as it reads, and
/// the address as well once the markdown is on show.
#[test]
fn ctrl_and_a_click_follows_the_link_under_the_pointer() {
    let path = document("ctrl-click-link", WITH_A_LINK);
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 12)).expect("a test screen");
    frame(&mut app, &mut terminal);

    let followed = |app: &App, column: u16| app.clicked_link(ctrl_click(column, 3));
    assert_eq!(followed(&app, app.column.x + 18).as_deref(), Some("https://example.com/x"));
    assert_eq!(followed(&app, app.column.x + 2), None, "the words beside the link are not it");
    assert_eq!(followed(&app, app.column.x + 80), None, "the blank past the row is not it");

    // The cursor inside the link opens that link up as markdown — "A paragraph with
    // [a link](https://example.com/x) in it." — so column 30 is now inside the
    // address rather than out in the words after it.
    app.editor.activate(1, 20);
    frame(&mut app, &mut terminal);
    assert_eq!(followed(&app, app.column.x + 30).as_deref(), Some("https://example.com/x"));
    forget(&path);
}

#[test]
fn a_double_click_selects_the_word_under_it() {
    let path = document("double-click", WITH_A_LINK);
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 12)).expect("a test screen");
    frame(&mut app, &mut terminal);

    let at = left_click(app.column.x + 4, 3);
    app.point(at);
    app.point(at);

    assert_eq!(app.editor.selected_text(), "paragraph");
    forget(&path);
}

#[test]
fn a_drag_takes_the_selection_with_it() {
    let path = document("drag", WITH_A_LINK);
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 12)).expect("a test screen");
    frame(&mut app, &mut terminal);

    app.point(left_click(app.column.x + 2, 3));
    frame(&mut app, &mut terminal);
    app.point(pointer(event::MouseEventKind::Drag(event::MouseButton::Left), app.column.x + 8, 3));

    assert_eq!(app.editor.selected_text(), "paragr");
    forget(&path);
}

/// A drag out of the block it started in takes the selection with it, which is the
/// only way a mouse has of asking for several blocks at once.
#[test]
fn a_drag_reaches_into_the_next_block() {
    let path = document("drag-across", "# Title\n\nA paragraph.\n");
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 12)).expect("a test screen");
    frame(&mut app, &mut terminal);

    app.point(left_click(app.column.x + 2, 1));
    frame(&mut app, &mut terminal);
    app.point(pointer(event::MouseEventKind::Drag(event::MouseButton::Left), app.column.x + 1, 3));

    assert!(app.editor.selected_text().contains("Title"), "{:?}", app.editor.selected_text());
    assert!(app.editor.selected_text().ends_with('A'), "{:?}", app.editor.selected_text());
    forget(&path);
}

/// The wheel moves the window and leaves the cursor where it is, so a writer can read
/// on without the line they were writing pulling the screen back. The next keystroke
/// takes hold of the window again.
#[test]
fn the_wheel_scrolls_without_moving_the_cursor() {
    let source = (0..20).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n\n");
    let path = document("wheel", &source);
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 9)).expect("a test screen");
    frame(&mut app, &mut terminal);

    app.point(pointer(event::MouseEventKind::ScrollDown, 10, 4));
    let scrolled = app.scroll;
    assert!(scrolled > 0, "the wheel left the first screenful in place");
    assert_eq!(app.editor.index(), 0, "the wheel moved the cursor");

    frame(&mut app, &mut terminal);
    assert_eq!(app.scroll, scrolled, "the frame pulled the window back to the caret");

    app.act(Action::Move(Motion::Character(1), false));
    frame(&mut app, &mut terminal);
    assert_eq!(app.scroll, 0, "typing left the caret off the screen");
    forget(&path);
}

/// The wheel is the window and nothing else, so it works while the writer is being
/// asked something; a click would move the cursor behind the question, and does not.
#[test]
fn a_question_takes_the_wheel_and_not_the_clicks() {
    let source = (0..20).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n\n");
    let path = document("wheel-question", &source);
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 9)).expect("a test screen");
    frame(&mut app, &mut terminal);
    app.mode = Mode::Quitting;

    app.point(pointer(event::MouseEventKind::ScrollDown, 10, 4));
    assert!(app.scroll > 0, "the wheel would not turn while the quit prompt was up");

    app.point(left_click(app.column.x, 4));
    assert_eq!(app.editor.index(), 0, "a click moved the cursor behind the quit prompt");
    forget(&path);
}

/// Quitting over a file somebody else has written asks rather than going quiet and
/// taking the answer. The question is the file's alone — the quit prompt is off the
/// screen by then — and saying yes finishes the quit that asked for it.
#[test]
fn a_quit_over_a_changed_file_asks_before_writing_over_it() {
    let path = document("quit-changed", "A document.");
    let mut app = app(&path);
    app.act(Action::Type("X".into()));
    std::fs::write(&path, "Somebody else.").expect("the file changes under the editor");
    app.mode = Mode::Quitting;

    app.act(Action::SaveAndQuit);
    assert!(!app.quit, "the editor quit over somebody else's work");
    let asked = ["post.md has external changes. Overwrite?", "[y] [n] [esc]"];
    assert_eq!(app.footer(90), asked);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Somebody else.");

    app.act(Action::Save);
    assert!(app.quit);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "XA document.");
    forget(&path);
}

/// Saying no to that question leaves the file as somebody else wrote it and puts the
/// writer back in the document, with nothing left at the foot of the screen: backing
/// out of the quit is not an error to be told about.
#[test]
fn saying_no_to_overwriting_leaves_the_file_and_the_editor_alone() {
    let path = document("keep-changed", "A document.");
    let mut app = app(&path);
    app.act(Action::Type("X".into()));
    std::fs::write(&path, "Somebody else.").expect("the file changes under the editor");

    app.act(Action::Save);
    app.act(Action::Cancel);

    assert!(!app.quit);
    assert!(app.mode == Mode::Editing);
    assert!(app.footer(90).is_empty(), "{:?}", app.footer(90));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Somebody else.");
    forget(&path);
}

/// A table wider than the column spills into the margins either side of it, centred on
/// the column, rather than being cut off at its right edge.
#[test]
fn spreads_a_wide_table_into_the_margins() {
    let cell = "x".repeat(theme::content_width() as usize);
    let path =
        document("wide-table", &format!("intro\n\n| a | b |\n| - | - |\n| {cell} | {cell} |\n"));
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(250, 12)).expect("a test screen");
    let screen = frame(&mut app, &mut terminal);
    forget(&path);

    let top = screen.iter().find(|row| row.contains('┌')).expect("the table's top border");
    let left = top.find('┌').expect("a left corner");
    assert!(top.ends_with('┐'), "the table is cut off: {top}");
    let right = 250 - top.chars().count();
    assert!(left.abs_diff(right) <= 1, "the table is not centred: {left} left, {right} right");
}

/// A table wider than the whole terminal starts at its left edge.
#[test]
fn starts_a_table_wider_than_the_screen_at_its_left_edge() {
    let cell = "x".repeat(60);
    let path = document("wider-table", &format!("| a | b |\n| - | - |\n| {cell} | {cell} |\n"));
    let mut app = app(&path);
    let mut terminal = Terminal::new(TestBackend::new(90, 12)).expect("a test screen");
    app.set_mode("reading");
    let screen = frame(&mut app, &mut terminal);
    forget(&path);

    assert!(screen.iter().any(|row| row.starts_with('┌')), "{screen:#?}");
}
