//! The event loop and the state that only the screen cares about: how far down the
//! document is scrolled, what the writer is being asked, and the column a run of up and
//! down movement is aiming for.

pub mod clipboard;
pub mod config;
pub mod images;
pub mod keys;
pub mod probe;
mod scroll;
mod status;
mod terminal;
pub mod theme;
pub mod view;

use crate::active::Step;
use crate::editor::Editor;
use crate::lint;
use crate::state;
use crate::tui::clipboard::Clipboard;
use crate::tui::images::Gallery;
use crate::tui::keys::Action;
use crossterm::event::{self, Event};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Frame, Terminal};
use std::io::{self, Stdout};
use std::path::Path;
use std::time::{Duration, Instant};

/// How long a pause counts as having stopped typing. Long enough to type through the end
/// of a word and into the next one, short enough that a writer who paused to think finds
/// the checker has already caught up.
const SETTLE: Duration = Duration::from_millis(400);
/// How often the loop looks up from the keyboard while nothing is being typed, so that
/// findings which finished off-thread reach the screen without one.
const TICK: Duration = Duration::from_millis(250);
/// How many rows of the screen are kept below the cursor before the document scrolls.
const MARGIN: usize = 3;
/// How many rows the foot of the screen keeps whether or not it has anything to say. A
/// picture drawn on the very last row makes a sixel terminal scroll a line to make room
/// for what it thinks comes next, and the line that goes off the top is gone: the editor
/// draws its next frame as a difference from what it believes is on the screen, and every
/// row of it is now one row out. So the last row is the foot's, and the document ends
/// above it.
const FOOT: u16 = 1;

/// The three modes the writer can be in, each with the word it is written down as between
/// runs and the two flags it means. The plain mode is first, and so is what an unreadable
/// word or a first run comes to.
const MODES: [(&str, bool, bool); 3] = [
    ("plain", true, false),
    ("grammar-off", false, false),
    ("reading", false, true),
];

/// What the writer is being asked, if anything. The document is behind all of them.
#[derive(PartialEq, Eq)]
enum Mode {
    Editing,
    Searching,
    Quitting,
    /// The check the writer has asked to be rid of, waiting on them to say they mean it.
    /// The name is held here rather than read back off the cursor when they answer: the
    /// checker finishes with a block on a thread of its own, and what is under the cursor
    /// can change between the question and the answer.
    Muting(String),
}

struct App {
    editor: Editor,
    document: view::Document,
    gallery: Gallery,
    clipboard: Clipboard,
    mode: Mode,
    /// Whether checker findings are shown.
    grammar: bool,
    /// Whether every block is shown rendered, including the one under the cursor.
    reading: bool,
    /// The screen row at the top of the window.
    scroll: usize,
    /// The column a run of up and down movement is aiming for, so that passing through a
    /// short row does not drag the cursor in to its end for good.
    goal: Option<u16>,
    /// When the last keystroke landed, or nothing once the checker has had its say.
    typed_at: Option<Instant>,
    /// The checker's generation the last frame was drawn from.
    generation: u64,
    /// How many rows of the document the last frame had room for, which is what a page
    /// of it means.
    viewport: usize,
    /// What the config said that could not be read, until the first key is pressed.
    notice: Vec<String>,
    quit: bool,
}

pub fn run(path: &Path) -> io::Result<()> {
    // First of all: the palette, the column and the keys are all read from it.
    let notice = config::load();
    // Before the terminal, so that reading the dictionaries happens alongside bringing
    // it up rather than after.
    lint::preload(&config::get().checks);
    let capabilities = probe::ask();
    let mut terminal = terminal::start(capabilities)?;
    // The picker is asked for once the screen is ours: the terminal answers the question
    // by writing to it, and this way it is our screen that gets written on.
    let mut app = App::open(path, Gallery::new(probe::pictures(), path));
    app.notice = notice;

    let result = app.loop_until_quit(&mut terminal);
    terminal::stop(capabilities)?;
    result
}

impl App {
    fn open(path: &Path, gallery: Gallery) -> Self {
        let mut app = App {
            editor: Editor::open(path),
            document: view::Document::default(),
            gallery,
            clipboard: Clipboard::open(),
            mode: Mode::Editing,
            grammar: true,
            reading: false,
            scroll: 0,
            goal: None,
            typed_at: None,
            generation: lint::generation(),
            viewport: 1,
            notice: Vec::new(),
            quit: false,
        };
        // Open the way the last run closed. A writer who turned the checker off did not
        // mean only for that afternoon.
        if let Some(word) = state::recall_mode() {
            app.set_mode(&word);
        }
        app
    }

    /// The mode as the one word it is written down as.
    fn mode_word(&self) -> &'static str {
        let (word, ..) = MODES
            .iter()
            .find(|(_, grammar, reading)| (*grammar, *reading) == (self.grammar, self.reading))
            .expect("the flags are only ever set to a mode's own");
        word
    }

    /// Go into the mode `word` names. A word from no mode there is — an older store, or a
    /// newer one — leaves the writer in the plain mode rather than somewhere odd.
    fn set_mode(&mut self, word: &str) {
        let (_, grammar, reading) = MODES
            .iter()
            .find(|(name, ..)| *name == word)
            .unwrap_or(&MODES[0]);
        (self.grammar, self.reading) = (*grammar, *reading);
    }

    /// Note what the next run should pick up: where the cursor was left, and which mode.
    fn remember(&self) {
        self.editor.remember_position();
        state::remember_mode(self.mode_word());
    }

    fn loop_until_quit(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
        while !self.quit {
            terminal.draw(|frame| self.draw(frame))?;
            self.wait()?;
        }
        self.remember();
        Ok(())
    }

    /// Take the next keystroke, or, where none comes, let the checker have its say. The
    /// deadline is what a Qt timer was: 400 ms after the typing stops.
    fn wait(&mut self) -> io::Result<()> {
        let deadline = self.typed_at.map(|at| SETTLE.saturating_sub(at.elapsed()));
        if event::poll(deadline.unwrap_or(TICK))? {
            match event::read()? {
                Event::Key(key) => self.press(key),
                Event::Paste(text) => self.act(Action::Type(text)),
                _ => {}
            }
            return Ok(());
        }
        if self.typed_at.is_some_and(|at| at.elapsed() >= SETTLE) {
            self.typed_at = None;
            self.editor.settle(true);
        }
        // Findings that finished off-thread are read here rather than waited for.
        let generation = lint::generation();
        if generation != self.generation {
            self.generation = generation;
            self.editor.refresh_lint();
        }
        Ok(())
    }

    fn press(&mut self, key: event::KeyEvent) {
        // Whatever the config could not read has been read by the writer by now.
        self.notice.clear();
        let action = match self.mode {
            Mode::Editing => keys::editing(key),
            Mode::Searching => keys::searching(key),
            Mode::Quitting => keys::quitting(key),
            Mode::Muting(_) => keys::muting(key),
        };
        self.act(action);
    }

    fn act(&mut self, action: Action) {
        if !matches!(action, Action::Row(..)) {
            self.goal = None;
        }
        match self.mode {
            Mode::Searching => self.act_searching(action),
            Mode::Quitting => self.act_quitting(action),
            Mode::Muting(_) => self.act_muting(action),
            Mode::Editing => self.act_editing(action),
        }
    }

    fn act_editing(&mut self, action: Action) {
        match action {
            Action::Type(text) => self.edit(|editor| editor.insert(&text)),
            Action::Delete(step) => self.edit(|editor| editor.delete(step)),
            Action::Enter => self.edit(Editor::enter),
            Action::Surround(marker) => self.edit(|editor| editor.surround(marker)),
            Action::Link(prefix) => self.edit(|editor| editor.insert_link(prefix)),
            Action::Undo => self.edit(Editor::undo),
            Action::AcceptLint if self.grammar => self.edit(Editor::accept_lint),
            Action::Learn if self.grammar => self.editor.learn(),
            Action::MuteCheck if self.grammar => self.ask_to_mute(),
            Action::CycleLint(step) if self.grammar => self.cycle_lint(step),
            Action::CycleLint(step) => self.step_row(step, false),
            Action::Move(motion, extend) => self.editor.move_cursor(motion, extend),
            Action::Row(step, extend) => self.step_row(step, extend),
            Action::Page(step) => self.page(step),
            Action::SelectAll => self.editor.select_all(),
            Action::Copy => self.copy(),
            Action::Paste => self.paste(),
            Action::Save => {
                self.editor.save();
            }
            Action::OpenSearch => {
                self.editor.open_search();
                self.mode = Mode::Searching;
            }
            Action::ToggleGrammar => {
                self.grammar = !self.grammar;
                if self.grammar {
                    self.reading = false;
                    self.editor.refresh_lint();
                }
            }
            Action::ToggleReading => {
                self.reading = !self.reading;
                self.grammar = false;
            }
            Action::Quit => self.leave(),
            _ => {}
        }
    }

    fn act_searching(&mut self, action: Action) {
        let mut needle = self.editor.search.needle.clone();
        match action {
            Action::Type(text) => needle.push_str(&text),
            Action::Delete(_) => {
                needle.pop();
            }
            Action::CycleSearch(step) => return self.editor.cycle_search(step),
            Action::CloseSearch => {
                self.editor.close_search();
                self.mode = Mode::Editing;
                return;
            }
            _ => return,
        }
        self.editor.search_for(&needle);
    }

    fn act_quitting(&mut self, action: Action) {
        match action {
            // A failed save keeps the editor open rather than losing the text.
            Action::SaveAndQuit => self.quit = self.editor.save(),
            Action::DiscardAndQuit => self.quit = true,
            Action::Cancel => self.mode = Mode::Editing,
            _ => {}
        }
    }

    fn act_muting(&mut self, action: Action) {
        match action {
            Action::MuteCheck => self.mute_check(),
            Action::Cancel => self.mode = Mode::Editing,
            _ => {}
        }
    }

    /// Something that changes the text. The checker is told the typing has started, and
    /// hears again once it stops.
    fn edit(&mut self, change: impl FnOnce(&mut Editor)) {
        change(&mut self.editor);
        self.editor.settle(false);
        self.typed_at = Some(Instant::now());
    }

    /// Control with the up and down keys walks the checker's suggestions. Where it has
    /// offered none there is nothing to walk, and the keys move the cursor as they always did.
    fn cycle_lint(&mut self, step: Step) {
        if self.editor.lint.replacements.is_empty() {
            self.step_row(step, false);
            return;
        }
        self.editor.cycle_lint(step);
    }

    /// Ask before a check goes. It is the writer's own config being written to, and a rule
    /// turned off by a mis-hit key is one they would never think to look for again. A
    /// misspelling has no rule behind it and is nothing to turn off: the dictionary is
    /// what that key is for, and there is nothing to ask.
    fn ask_to_mute(&mut self) {
        let rule = self.editor.lint.rule.clone();
        if rule.is_empty() {
            return;
        }
        self.mode = Mode::Muting(rule);
    }

    /// Never show this check again: the rule that objected stops running now and stays off
    /// in the writer's config. Only ever reached through the question above.
    fn mute_check(&mut self) {
        let Mode::Muting(rule) = std::mem::replace(&mut self.mode, Mode::Editing) else {
            return;
        };
        match config::checks::set(&rule, false) {
            Ok(()) => {
                lint::mute(&rule);
                self.editor.refresh_lint();
            }
            Err(problem) => self.editor.error = Some(problem),
        }
    }

    fn copy(&mut self) {
        let selected = self.editor.selected_text();
        if selected.is_empty() {
            return;
        }
        self.clipboard.copy(selected);
    }

    /// Paste, which is a typing of what the clipboard holds: the same path a terminal's
    /// own paste takes, so a selection under it is replaced either way.
    fn paste(&mut self) {
        let text = self.clipboard.paste();
        if text.is_empty() {
            return;
        }
        self.edit(|editor| editor.insert(&text));
    }

    /// Ctrl+Q, Esc or Ctrl+D. A document with unsaved work asks first, with the three
    /// answers the Qt window's close prompt had.
    fn leave(&mut self) {
        if self.editor.dirty() {
            self.mode = Mode::Quitting;
            return;
        }
        self.quit = true;
    }

    // ---- drawing ---------------------------------------------------------------

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let width = theme::content_width().min(area.width);
        self.document.configure(self.grammar, self.reading);
        self.document.rebuild(&self.editor, width, &mut self.gallery);
        // A picture that has just been read pushes everything under it down. The window
        // goes with it, so the block being written in does not move under the writer.
        self.scroll = self.scroll.saturating_add_signed(self.document.shift());

        // The ground first, so that the rows between blocks are paper too where the
        // config asked for paper. Where it did not, this is the terminal's own and
        // nothing is painted over it.
        frame.buffer_mut().set_style(area, theme::base());
        let footer = self.footer(area.width);
        let footer_heights: Vec<_> = footer
            .iter()
            .map(|message| view::footer_height(area, message))
            .collect();
        // The band is there with nothing in it as readily as with something, so that the
        // checker speaking up does not move the document under the writer. A message
        // longer than the column gets every row it wraps onto rather than being clipped.
        let band = footer_heights
            .iter()
            .fold(0u16, |total, height| total.saturating_add(*height))
            .max(FOOT)
            .min(area.height);
        let text = Rect { height: area.height.saturating_sub(band), ..area };
        self.viewport = text.height as usize;
        self.follow_the_caret(self.viewport);
        view::draw(frame, text, &self.editor, &self.document, self.scroll);
        // Over the rows the layout left empty for them, and after the text: a picture is
        // drawn by the terminal itself, not out of the cells underneath it.
        self.gallery.draw(frame, text, &self.document, self.scroll);

        let mut y = text.y + text.height;
        for (message, height) in footer.iter().zip(footer_heights) {
            let height = height.min(area.bottom().saturating_sub(y));
            if height == 0 {
                break;
            }
            view::footer(frame, Rect { y, height, ..area }, message);
            y += height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui_image::picker::Picker;

    /// A document of its own, so that a test writing into it disturbs nothing else, with
    /// the sample picture beside it for the block that names one.
    fn document(name: &str, source: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("markatui-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a temporary directory");
        let path = directory.join("post.md");
        std::fs::write(&path, source).expect("a document");
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("sample/image.png");
        std::fs::copy(sample, directory.join("image.png")).expect("a picture beside it");
        path
    }

    /// A document three rows down from the top of a fifteen-row screen with twelve rows of
    /// picture under it: the last row of the picture and the last row of the screen are
    /// the same row, which is the one place a picture must not be drawn.
    const TO_THE_BOTTOM: &str = "# Title\n\n![A picture](image.png)\n";

    /// The document and the picture beside it, once the test is through with them.
    fn forget(path: &Path) {
        std::fs::remove_dir_all(path.parent().expect("a directory of its own"))
            .expect("the temporary directory goes");
    }

    /// The editor on the document at `path`, drawing pictures as the mosaic a test can
    /// make: there is no terminal here to ask for a protocol of its own.
    fn app(path: &Path) -> App {
        let mut app = App::open(path, Gallery::new(Picker::halfblocks(), path));
        // Wherever the last session left the cursor and whichever mode it was left in, a
        // test starts at the top of the document in the plain one.
        app.editor.activate(0, 0);
        app.set_mode("plain");
        app
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

    #[test]
    fn page_keys_scroll_and_move_the_cursor() {
        let source = (0..20)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let path = document("pages", &source);
        let mut app = app(&path);
        let mut terminal = Terminal::new(TestBackend::new(90, 9)).expect("a test screen");
        frame(&mut app, &mut terminal);

        app.page(1);
        frame(&mut app, &mut terminal);
        assert!(app.scroll > 0, "PageDown left the first screenful in place");
        assert!(app.editor.index() > 0, "PageDown left the cursor in place");

        app.page(-1);
        frame(&mut app, &mut terminal);
        assert_eq!(app.scroll, 0);
        assert_eq!(app.editor.index(), 0);
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
        assert!(
            !rows[14].contains('▄'),
            "the picture ran into the foot of the screen: {rows:#?}"
        );
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
        let held = state::recall_mode();

        let mut left = app(&path);
        left.act(Action::ToggleReading);
        left.remember();
        let opened = App::open(&path, Gallery::new(Picker::halfblocks(), &path));
        assert_eq!(opened.mode_word(), "reading");

        let mut left = app(&path);
        left.act(Action::ToggleGrammar);
        left.remember();
        let opened = App::open(&path, Gallery::new(Picker::halfblocks(), &path));
        assert_eq!(opened.mode_word(), "grammar-off");

        if let Some(held) = held {
            state::remember_mode(&held);
        }
        forget(&path);
    }

    #[test]
    fn says_which_mode_the_writer_is_in() {
        let path = document("modes", "A document.\n");
        let mut app = app(&path);

        assert!(app.footer(40).is_empty(), "the checker has said nothing yet");
        app.act(Action::ToggleGrammar);
        assert_eq!(app.footer(40), vec!["grammar off".to_string()]);
        app.act(Action::ToggleReading);
        assert_eq!(app.footer(40), vec!["reading".to_string()]);
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
}
