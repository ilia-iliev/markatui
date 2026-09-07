//! The event loop and the state that only the screen cares about: how far down the
//! document is scrolled, what the writer is being asked, and the column a run of up and
//! down movement is aiming for.

pub mod keys;
pub mod probe;
pub mod theme;
pub mod view;

use crate::active::Step;
use crate::editor::{Editor, Motion};
use crate::lint;
use crate::tui::keys::Action;
use crate::tui::probe::{Capabilities, Keyboard};
use crossterm::clipboard::CopyToClipboard;
use crossterm::event::{
    self, Event, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, terminal};
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

/// What the writer is being asked, if anything. The document is behind all of them.
#[derive(PartialEq, Eq)]
enum Mode {
    Editing,
    Searching,
    Quitting,
}

struct App {
    editor: Editor,
    document: view::Document,
    mode: Mode,
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
    quit: bool,
}

pub fn run(path: &Path) -> io::Result<()> {
    // Before the terminal, so that reading the dictionaries happens alongside bringing
    // it up rather than after.
    lint::preload();
    let capabilities = probe::ask();
    let mut terminal = start(capabilities)?;
    let mut app = App {
        editor: Editor::open(path),
        document: view::Document::default(),
        mode: Mode::Editing,
        scroll: 0,
        goal: None,
        typed_at: None,
        generation: lint::generation(),
        viewport: 1,
        quit: false,
    };

    let result = app.loop_until_quit(&mut terminal);
    stop(capabilities)?;
    result
}

impl App {
    fn loop_until_quit(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
        while !self.quit {
            terminal.draw(|frame| self.draw(frame))?;
            self.wait()?;
        }
        self.editor.remember_position();
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
        let action = match self.mode {
            Mode::Editing => keys::editing(key),
            Mode::Searching => keys::searching(key),
            Mode::Quitting => keys::quitting(key),
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
            Action::AcceptLint => self.edit(Editor::accept_lint),
            Action::Learn => self.editor.learn(),
            Action::CycleLint(step) => self.cycle_lint(step),
            Action::Move(motion, extend) => self.editor.move_cursor(motion, extend),
            Action::Row(step, extend) => self.step_row(step, extend),
            Action::Page(step) => self.page(step),
            Action::SelectAll => self.editor.select_all(),
            Action::Copy => self.copy(),
            Action::Save => {
                self.editor.save();
            }
            Action::OpenSearch => {
                self.editor.open_search();
                self.mode = Mode::Searching;
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

    /// Up and down, which move by the rows on the screen rather than by the lines of the
    /// source: a wrapped paragraph is several rows and one line. At the top and bottom of
    /// a block the cursor leaves it, which is the rule the Qt editor had.
    fn step_row(&mut self, step: Step, extend: bool) {
        let Some((row, column)) = self.caret_within_block() else {
            return self.editor.move_cursor(Motion::Line(step), extend);
        };
        let goal = *self.goal.get_or_insert(column);
        let rows = self.document.rows(self.editor.index());
        let landing = row.checked_add_signed(step as isize).filter(|at| *at < rows.len());
        match landing.and_then(|at| rows[at].source_at(goal)) {
            Some(at) => self.editor.place_cursor(at, extend),
            None => self.editor.move_cursor(Motion::Line(step), extend),
        }
    }

    fn caret_within_block(&self) -> Option<(usize, u16)> {
        let (row, column) = self.document.caret()?;
        Some((row - self.document.top(self.editor.index()), column))
    }

    /// Page through the document a screenful at a time. The block at the far edge lands
    /// on the near one, so nothing between the two screenfuls is missed.
    fn page(&mut self, step: Step) {
        let Some((row, _)) = self.document.caret() else { return };
        let target = row.saturating_add_signed(step as isize * self.viewport as isize);
        let target = target.min(self.document.height().saturating_sub(1));
        let Some((index, _)) = self.document.at(target).or_else(|| self.document.at(target.saturating_sub(1)))
        else {
            return;
        };
        self.editor.activate(index, 0);
    }

    /// The selection, handed to the terminal over OSC 52. A terminal with it turned off
    /// ignores it silently; there is nothing to detect and nothing else is lost.
    fn copy(&mut self) {
        let selected = self.editor.selected_text();
        if selected.is_empty() {
            return;
        }
        let _ = execute!(io::stdout(), CopyToClipboard::to_clipboard_from(selected));
    }

    /// Ctrl+Q. A document with unsaved work asks first, with the three answers the Qt
    /// window's close prompt had.
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
        let width = theme::CONTENT_WIDTH.min(area.width);
        self.document.rebuild(&self.editor, width);

        let footer = self.footer();
        let text = Rect { height: area.height.saturating_sub(footer.len() as u16), ..area };
        self.viewport = text.height as usize;
        self.follow_the_caret(self.viewport);
        view::draw(frame, text, &self.editor, &self.document, self.scroll);

        for (line, (message, style)) in footer.iter().enumerate() {
            let row = Rect { y: text.y + text.height + line as u16, height: 1, ..area };
            view::footer(frame, row, message, *style);
        }
    }

    /// Keep the caret on the screen, and a few rows clear of the bottom of it.
    fn follow_the_caret(&mut self, height: usize) {
        let Some((row, _)) = self.document.caret() else { return };
        let margin = MARGIN.min(height / 4);
        if row < self.scroll + margin {
            self.scroll = row.saturating_sub(margin);
        }
        if row + margin >= self.scroll + height {
            self.scroll = row + margin + 1 - height;
        }
    }

    /// What the foot of the screen says, from the top of the pile down: the question a
    /// quit asks, the search bar, a file that would not open or save, and what the
    /// checker makes of where the cursor is standing.
    fn footer(&self) -> Vec<(String, ratatui::style::Style)> {
        if self.mode == Mode::Quitting {
            let question = match &self.editor.error {
                Some(error) => format!("Save changes?  {error}"),
                None => "Save changes?".to_string(),
            };
            return vec![(question, theme::prompt()), ("[y] [n] [esc]".into(), theme::prompt())];
        }
        if self.mode == Mode::Searching {
            return vec![(self.search_line(), theme::prompt())];
        }
        if let Some(error) = &self.editor.error {
            return vec![(error.clone(), theme::prompt())];
        }
        match self.lint_line() {
            Some(line) => vec![(line, theme::prompt())],
            None => Vec::new(),
        }
    }

    /// `find <word>` with which occurrence of how many at the far edge of the column, so
    /// that the count stays put while the word is typed. Asking for another occurrence of
    /// a word that has only the one moves nothing, and that is the answer given instead.
    fn search_line(&self) -> String {
        let search = &self.editor.search;
        let counter = match () {
            _ if search.needle.is_empty() => String::new(),
            _ if search.alone => "only one".into(),
            _ => match search.choice {
                Some(choice) => format!("{}/{}", choice + 1, search.count),
                None => "no matches".into(),
            },
        };
        let typed = format!("find {}", search.needle);
        let room = (theme::CONTENT_WIDTH as usize)
            .saturating_sub(typed.chars().count() + counter.chars().count());
        format!("{typed}{}{counter}", " ".repeat(room.max(2)))
    }

    /// The checker's objection, the one suggestion on show, and how many others there
    /// are — the count being there to say that Ctrl with the arrows has somewhere to go.
    fn lint_line(&self) -> Option<String> {
        let lint = &self.editor.lint;
        if lint.message.is_empty() {
            return None;
        }
        let suggestion = match lint.suggestion() {
            None => String::new(),
            // A suggestion with nothing in it is a suggestion to take the words out.
            Some("") => "  →  delete".into(),
            Some(replacement) => format!("  →  {replacement}"),
        };
        let counter = match lint.replacements.len() {
            0 | 1 => String::new(),
            options => format!("  {}/{options}", lint.choice + 1),
        };
        Some(format!("{}{suggestion}{counter}", lint.message))
    }
}

/// Raw mode, a screen of our own, bracketed paste, and the kitty keyboard flags where the
/// terminal answers for them. The panic hook puts every one of them back: flags left
/// pushed after a crash leave the writer's shell with odd keys.
fn start(capabilities: Capabilities) -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, event::EnableBracketedPaste)?;
    if capabilities.keyboard == Keyboard::Kitty {
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let _ = stop(capabilities);
        previous(panic);
    }));
    Terminal::new(CrosstermBackend::new(io::stdout()))
}

fn stop(capabilities: Capabilities) -> io::Result<()> {
    if capabilities.keyboard == Keyboard::Kitty {
        execute!(io::stdout(), PopKeyboardEnhancementFlags)?;
    }
    execute!(io::stdout(), event::DisableBracketedPaste, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()
}
