//! The event loop and the state that only the screen cares about: how far down the
//! document is scrolled, what the writer is being asked, and the column a run of up and
//! down movement is aiming for.

mod act;
pub(crate) mod clipboard;
pub mod config;
pub mod images;
pub(crate) mod keys;
mod link;
mod mouse;
mod pictures;
pub(crate) mod probe;
mod scroll;
mod status;
mod terminal;
pub mod theme;
pub mod view;

use crate::active::Step;
use crate::editor::{Editor, Motion};
use crate::lint;
use crate::storage;
use crate::tui::clipboard::{Clipboard, Paste};
use crate::tui::images::Gallery;
use crate::tui::keys::Action;
use crate::tui::pictures::PastedPicture;
use crate::tui::probe::Keyboard;
use crossterm::event::{self, Event};
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use crossterm::{execute, queue};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Frame, Terminal};
use std::io::{self, Stdout, Write};
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
/// How long the scrollbar stays up after the window last moved.
const SCROLLBAR: Duration = Duration::from_secs(2);

/// What the foot of the screen says when a picture the writer left among the words has
/// been broken out into a paragraph of its own.
const HOISTED: &str = "Inline images are not supported: moved to a paragraph";

/// The three modes the writer can be in, each with the word it is written down as between
/// runs and the two flags it means. The plain mode is first, and so is what an unreadable
/// word or a first run comes to.
const MODES: [(&str, bool, bool); 3] =
    [("plain", true, false), ("grammar-off", false, false), ("reading", false, true)];

/// What the writer is being asked, if anything. The document is behind all of them.
#[derive(PartialEq, Eq)]
enum Mode {
    Editing,
    Searching,
    Quitting,
    /// Somebody else has written the file since it was opened, waiting on the writer to
    /// say whether to go over them. The flag is whether the save was on the way out, so
    /// that saying yes finishes the quit that asked for it.
    Overwriting(bool),
    /// The check the writer has asked to be rid of, waiting on them to say they mean it.
    /// The name is held here rather than read back off the cursor when they answer: the
    /// checker finishes with a block on a thread of its own, and what is under the cursor
    /// can change between the question and the answer.
    Muting(String),
}

impl Mode {
    /// What a keystroke means here. A mode is the whole answer to what a key does: Enter
    /// ends a block while the writer is in the document, and closes the search while they
    /// are in the bar.
    fn means(&self, key: event::KeyEvent) -> Action {
        match self {
            Mode::Editing => keys::editing(key),
            Mode::Searching => keys::searching(key),
            Mode::Quitting => keys::prompt(key, Action::SaveAndQuit, Action::DiscardAndQuit),
            Mode::Overwriting(_) => keys::prompt(key, Action::Save, Action::Cancel),
            Mode::Muting(_) => keys::prompt(key, Action::MuteCheck, Action::Cancel),
        }
    }

    /// What carries the action out here, kept beside [`Mode::means`]: a mode added to the
    /// one and not the other is a mode the compiler asks about.
    fn handler(&self) -> fn(&mut App, Action) {
        match self {
            Mode::Editing => App::act_editing,
            Mode::Searching => App::act_searching,
            Mode::Quitting => App::act_quitting,
            Mode::Overwriting(_) => App::act_overwriting,
            Mode::Muting(_) => App::act_muting,
        }
    }
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
    /// The row the last frame had at the top, and when the window last moved off it:
    /// the scrollbar is up only for a while after that.
    drawn_scroll: usize,
    scrolled_at: Option<Instant>,
    /// Whether the window keeps the caret in view. The wheel lets it go, so a writer can
    /// read on without the line they were writing pulling the screen back; the next thing
    /// they do with the keyboard takes hold of it again.
    follow: bool,
    /// The column of the screen the document is drawn in, as the last frame laid it out,
    /// which is what turns where the pointer is into where in the document it is.
    column: Rect,
    /// When and where the last click landed, so that a second one in the same cell can be
    /// told from two clicks in the same place.
    clicked: Option<(Instant, (u16, u16))>,
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
    /// What the terminal answers about its keyboard, which says whether the shifted
    /// bindings arrive as keys of their own and whether a frame can be sent in one piece.
    keyboard: Keyboard,
    pictures: Vec<PastedPicture>,
    quit: bool,
}

pub fn run(path: &Path) -> io::Result<()> {
    // First of all: the palette, the column and the keys are all read from it.
    let notice = config::load();
    // Before the terminal, so that reading the dictionaries happens alongside bringing
    // it up rather than after.
    lint::preload(&config::get().checks);
    let keyboard = probe::ask();
    let background = theme::terminal_background();
    let mut terminal = terminal::start(keyboard, background, &name(path))?;
    // The picker is asked for once the screen is ours: the terminal answers the question
    // by writing to it, and this way it is our screen that gets written on.
    let mut app = App::open(path, Gallery::new(probe::pictures(), path), keyboard);
    // Whatever the config could not read goes first: it is the older news of the two, and
    // the writer will want to hear it before anything the document did on the way in.
    app.notice.splice(..0, notice);

    let result = app.loop_until_quit(&mut terminal);
    app.discard_pictures();
    terminal::stop(keyboard, background)?;
    result
}

/// What to call the file in the title bar: its name, or the whole path where it has no
/// name to give — a path ending in `..` has none.
fn name(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => path.display().to_string(),
    }
}

impl App {
    fn open(path: &Path, gallery: Gallery, keyboard: Keyboard) -> Self {
        let mut app = App {
            editor: Editor::open(path),
            document: view::Document::default(),
            gallery,
            clipboard: Clipboard::open(),
            mode: Mode::Editing,
            grammar: true,
            reading: false,
            scroll: 0,
            drawn_scroll: 0,
            scrolled_at: None,
            follow: true,
            column: Rect::ZERO,
            clicked: None,
            goal: None,
            typed_at: None,
            generation: lint::generation(),
            viewport: 1,
            notice: Vec::new(),
            keyboard,
            pictures: Vec::new(),
            quit: false,
        };
        // Open the way the last run closed. A writer who turned the checker off did not
        // mean only for that afternoon.
        if let Some(word) = storage::recall_mode() {
            app.set_mode(&word);
        }
        app.note_hoisted();
        app
    }

    /// A picture broken out of the words around it is the writer's document being changed
    /// under them, so the foot of the screen says so — until the next keystroke, which is
    /// how long every notice lasts.
    fn note_hoisted(&mut self) {
        if self.editor.take_hoisted() {
            self.notice.push(HOISTED.to_string());
        }
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
        let (_, grammar, reading) =
            MODES.iter().find(|(name, ..)| *name == word).unwrap_or(&MODES[0]);
        (self.grammar, self.reading) = (*grammar, *reading);
    }

    /// Note what the next run should pick up: where the cursor was left, and which mode.
    fn remember(&self) {
        self.editor.remember_position();
        storage::remember_mode(self.mode_word());
    }

    fn loop_until_quit(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> io::Result<()> {
        while !self.quit {
            self.send(terminal)?;
            self.wait()?;
        }
        self.remember();
        Ok(())
    }

    /// Send one frame as one synchronized update, with the caret off the screen inside it.
    ///
    /// A frame does not arrive all at once. It goes out in pieces as the buffer behind
    /// stdout fills, and the terminal draws each piece as it comes; the caret is the
    /// terminal's own and sits wherever the writing has got to. A short frame is written
    /// and done with before that can be seen, but a screenful resent under a picture is
    /// not short — and a gif makes one of those every time it turns, ten times a second.
    /// The caret is then watched wandering off into the document and coming back.
    ///
    /// Taking the caret away for the length of the frame is what stopped it wandering,
    /// and it cost the writer the caret itself: ratatui gives it back at the end of every
    /// frame, and a frame goes out each time the loop looks up from the keyboard — four
    /// times a second with nothing being typed, ten with a gif turning. Off and on, off
    /// and on, which is a caret that blinks whatever the terminal was told about blinking.
    ///
    /// So the frame is wrapped instead: between the two escapes below the terminal keeps
    /// presenting what it last presented, however much is sent it. The caret comes off
    /// only where that wrapping is known to work, so taking it off and putting it back
    /// both happen inside the update and never reach the screen. A terminal that cannot
    /// hold an update keeps its native caret off and gets one painted into the frame;
    /// otherwise either the hide and show or the renderer's cursor motion would be seen.
    fn send<W: Write>(&mut self, terminal: &mut Terminal<CrosstermBackend<W>>) -> io::Result<()> {
        if !probe::synchronized_updates(self.keyboard) {
            terminal.hide_cursor()?;
            terminal.draw(|frame| self.draw(frame))?;
            return Ok(());
        }
        queue!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
        terminal.hide_cursor()?;
        terminal.draw(|frame| self.draw(frame))?;
        execute!(terminal.backend_mut(), EndSynchronizedUpdate)
    }

    /// Take the next keystroke, or, where none comes, let the checker have its say. The
    /// deadline is what a Qt timer was: 400 ms after the typing stops, or sooner where a
    /// gif on the screen is due to turn — that one comes whether or not the writer
    /// touches the keyboard, and the soonest of the two is the one waited for.
    fn wait(&mut self) -> io::Result<()> {
        let settle = self.typed_at.map(|at| SETTLE.saturating_sub(at.elapsed()));
        let turn = self.gallery.due().map(|at| at.saturating_duration_since(Instant::now()));
        let deadline = settle.into_iter().chain(turn).min();
        if event::poll(deadline.unwrap_or(TICK))? {
            match event::read()? {
                Event::Key(key) => self.press(key),
                Event::Mouse(pointer) => self.point(pointer),
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
        // A key coming back up is the same keystroke over again: only the press is meant.
        if key.kind == event::KeyEventKind::Release {
            return;
        }
        self.act(self.mode.means(key));
    }

    fn act(&mut self, action: Action) {
        // Whatever the wheel did to the window, a key takes hold of it again: the writer
        // is typing, and what they are typing has to be on the screen.
        self.follow = true;
        if !matches!(action, Action::Row(..)) {
            self.goal = None;
        }
        (self.mode.handler())(self, action);
        self.note_hoisted();
    }

    /// Something that changes the text. The checker is told the typing has started, and
    /// hears again once it stops.
    fn edit(&mut self, change: impl FnOnce(&mut Editor)) {
        change(&mut self.editor);
        self.editor.settle(false);
        self.typed_at = Some(Instant::now());
    }

    /// Control with the up and down keys walks the checker's suggestions. Where the
    /// checker is off, or has offered none, there is nothing to walk and the keys walk
    /// the blocks, the way control with Left and Right walks the words.
    fn cycle_lint(&mut self, step: Step) {
        if !self.grammar || self.editor.lint.replacements.is_empty() {
            self.editor.move_cursor(Motion::Block(step), false);
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

    /// Ctrl+K, which is both things a writer does with a link: standing in one, it is
    /// followed; standing anywhere else, one is started.
    fn open_or_link(&mut self) {
        let Some(url) = self.editor.link_at_cursor() else {
            return self.edit(|editor| editor.insert_link(""));
        };
        self.editor.error = link::url(&url).err();
    }

    /// Copy and delete in one, which is what every editor means by cut. A cursor with
    /// nothing selected has nothing to cut, and the key does nothing.
    fn cut(&mut self) {
        if self.editor.selection().is_none() {
            return;
        }
        self.copy();
        self.edit(|editor| editor.delete(1));
    }

    fn copy(&mut self) {
        let copied = self.copied();
        self.clipboard.copy(copied);
    }

    /// What copy puts on the clipboard: the selection, or the whole document where
    /// nothing is selected.
    fn copied(&self) -> String {
        match self.editor.selection() {
            Some(_) => self.editor.selected_text(),
            None => self.editor.source(),
        }
    }

    fn paste(&mut self) {
        let content = self.clipboard.content();
        self.paste_content(content);
    }

    /// Put in what the clipboard was holding. Words are typed, which is the path a
    /// terminal's own paste takes, so a selection under them is replaced either way.
    fn paste_content(&mut self, content: Paste) {
        match content {
            Paste::Words(words) => self.edit(|editor| editor.insert(&words)),
            Paste::Picture(png) => self.paste_picture(&png),
            Paste::Nothing => {}
        }
    }

    /// Save the document and make the pasted files agree with its image references. The
    /// renames go first and are put back where the document itself would not save: this is
    /// the only time a rename touches the filesystem, so typing a filename stays cheap.
    ///
    /// A file somebody else has written since it was opened is not gone over without
    /// asking. The question goes up at the foot of the screen and the save waits on the
    /// answer, which is the writer's to give.
    fn save(&mut self) -> bool {
        if self.editor.changed_on_disk() {
            self.mode = Mode::Overwriting(self.mode == Mode::Quitting);
            return false;
        }
        let Some(renamed) = self.rename_pictures() else {
            return false;
        };
        if !self.editor.save() {
            renamed.undo();
            return false;
        }
        self.settle_pictures(renamed);
        true
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

    /// Whether the window has moved lately. The loop looks up at least every [`TICK`], so
    /// the bar goes within one of those of its time being up.
    fn scrolling(&mut self) -> bool {
        if self.scroll != self.drawn_scroll {
            self.drawn_scroll = self.scroll;
            self.scrolled_at = Some(Instant::now());
        }
        self.scrolled_at.is_some_and(|at| at.elapsed() < SCROLLBAR)
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let width = theme::column_width(area.width);
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
        let footer_heights: Vec<_> =
            footer.iter().map(|message| view::footer_height(area, message)).collect();
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
        self.column = view::column(text);
        if self.follow {
            self.follow_the_caret(self.viewport);
        }
        view::draw_with_caret(
            frame,
            text,
            &self.editor,
            &self.document,
            self.scroll,
            probe::synchronized_updates(self.keyboard),
        );
        // Over the rows the layout left empty for them, and after the text: a picture is
        // drawn by the terminal itself, not out of the cells underneath it.
        self.gallery.draw(frame, text, &self.document, self.scroll);
        if self.scrolling() {
            view::scrollbar(frame, text, self.scroll, self.document.height());
        }

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
#[path = "tests.rs"]
mod tests;
