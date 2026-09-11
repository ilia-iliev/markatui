//! The event loop and the state that only the screen cares about: how far down the
//! document is scrolled, what the writer is being asked, and the column a run of up and
//! down movement is aiming for.

pub mod clipboard;
pub mod config;
pub mod images;
pub mod keys;
mod mouse;
pub mod open;
pub mod probe;
mod scroll;
mod status;
mod terminal;
pub mod theme;
pub mod view;

use crate::active::Step;
use crate::editor::{Editor, Field};
use crate::lint;
use crate::parse;
use crate::state;
use crate::storage;
use crate::tui::clipboard::{Clipboard, Paste};
use crate::tui::images::Gallery;
use crate::tui::keys::Action;
use crate::tui::probe::Keyboard;
use crossterm::event::{self, Event};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Frame, Terminal};
use std::fs;
use std::io::{self, Stdout, Write};
use std::path::{Component, Path, PathBuf};
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
    /// The check the writer has asked to be rid of, waiting on them to say they mean it.
    /// The name is held here rather than read back off the cursor when they answer: the
    /// checker finishes with a block on a thread of its own, and what is under the cursor
    /// can change between the question and the answer.
    Muting(String),
}

struct PastedPicture {
    path: PathBuf,
    reference: String,
    ordinal: usize,
    saved: bool,
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
    /// What the terminal answers about its keyboard, which says whether Shift+Enter
    /// arrives as a key of its own or as a plain Enter.
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
    let capabilities = probe::ask();
    let background = theme::terminal_background();
    let mut terminal = terminal::start(capabilities, background)?;
    // The picker is asked for once the screen is ours: the terminal answers the question
    // by writing to it, and this way it is our screen that gets written on.
    let mut app = App::open(path, Gallery::new(probe::pictures(), path), capabilities.keyboard);
    // Whatever the config could not read goes first: it is the older news of the two, and
    // the writer will want to hear it before anything the document did on the way in.
    app.notice.splice(..0, notice);

    let result = app.loop_until_quit(&mut terminal);
    app.discard_pictures();
    terminal::stop(capabilities, background)?;
    result
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
        if let Some(word) = state::recall_mode() {
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
        state::remember_mode(self.mode_word());
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

    /// Send one frame, with the caret off the screen while it goes.
    ///
    /// A frame does not arrive all at once. It goes out in pieces as the buffer behind
    /// stdout fills, and the terminal draws each piece as it comes; the caret is the
    /// terminal's own and sits wherever the writing has got to. A short frame is written
    /// and done with before that can be seen, but a screenful resent under a picture is
    /// not short — and a gif makes one of those every time it turns, ten times a second.
    /// The caret is then watched wandering off into the document and coming back.
    ///
    /// Ratatui puts it back where it belongs, but only once the last piece has gone out,
    /// which is too late to not have been seen. So it is taken away first; ratatui's own
    /// draw is what gives it back, at the end, where it was always going to.
    fn send<W: Write>(&mut self, terminal: &mut Terminal<CrosstermBackend<W>>) -> io::Result<()> {
        terminal.hide_cursor()?;
        terminal.draw(|frame| self.draw(frame))?;
        Ok(())
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
        let action = match self.mode {
            Mode::Editing => keys::editing(key),
            Mode::Searching => keys::searching(key),
            Mode::Quitting => keys::quitting(key),
            Mode::Muting(_) => keys::muting(key),
        };
        self.act(action);
    }

    fn act(&mut self, action: Action) {
        // Whatever the wheel did to the window, a key takes hold of it again: the writer
        // is typing, and what they are typing has to be on the screen.
        self.follow = true;
        if !matches!(action, Action::Row(..)) {
            self.goal = None;
        }
        match self.mode {
            Mode::Searching => self.act_searching(action),
            Mode::Quitting => self.act_quitting(action),
            Mode::Muting(_) => self.act_muting(action),
            Mode::Editing => self.act_editing(action),
        }
        self.note_hoisted();
    }

    /// Enter, which ends the block. A terminal that cannot tell Shift+Enter from Enter
    /// leaves the writer one key for the two things, so there it keeps the older rule: a
    /// first press leaves a line break and a second ends the block.
    fn enter(&mut self) {
        match self.keyboard {
            Keyboard::Kitty => self.edit(Editor::enter),
            Keyboard::Legacy => self.edit(Editor::enter_or_break),
        }
    }

    fn act_editing(&mut self, action: Action) {
        match action {
            Action::Type(text) => self.edit(|editor| editor.insert(&text)),
            Action::Delete(step) => self.edit(|editor| editor.delete(step)),
            Action::Enter => self.enter(),
            Action::LineBreak => self.edit(Editor::line_break),
            Action::Tab(step) => self.edit(|editor| editor.tab(step)),
            Action::Surround(marker) => self.edit(|editor| editor.surround(marker)),
            Action::Tag(tag) => self.edit(|editor| editor.wrap(tag)),
            Action::Link(prefix) => self.edit(|editor| editor.insert_link(prefix)),
            Action::OpenOrLink => self.open_or_link(),
            Action::Mark(mark) => self.edit(|editor| editor.mark(mark)),
            Action::Fence => self.edit(Editor::fence),
            Action::Rule => self.edit(Editor::insert_rule),
            Action::Table => self.edit(Editor::insert_table),
            Action::Align(align) => self.edit(|editor| editor.align(align)),
            Action::Cut => self.cut(),
            Action::Undo => self.edit(Editor::undo),
            Action::Redo => self.edit(Editor::redo),
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
                self.save();
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
        let mut typed = self.editor.field().to_string();
        match action {
            Action::Type(text) => typed.push_str(&text),
            Action::Delete(_) => {
                typed.pop();
            }
            Action::Tab(_) => return self.editor.switch_field(),
            Action::CycleSearch(step) => return self.editor.cycle_search(step),
            // Enter in the half holding the word says it is typed; in the half holding
            // the replacement it is the swap being asked for.
            Action::Enter if self.editor.search.field == Field::Replacement => {
                return self.edit(Editor::replace_found);
            }
            Action::ReplaceAll => return self.edit(Editor::replace_all),
            Action::Enter | Action::CloseSearch => {
                self.editor.close_search();
                self.mode = Mode::Editing;
                return;
            }
            _ => return,
        }
        self.editor.type_into_field(&typed);
    }

    fn act_quitting(&mut self, action: Action) {
        match action {
            // A failed save keeps the editor open rather than losing the text.
            Action::SaveAndQuit => self.quit = self.save(),
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

    /// Ctrl+K, which is both things a writer does with a link: standing in one, it is
    /// followed; standing anywhere else, one is started.
    fn open_or_link(&mut self) {
        let Some(url) = self.editor.link_at_cursor() else {
            return self.edit(|editor| editor.insert_link(""));
        };
        self.editor.error = open::url(&url).err();
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
        let selected = self.editor.selected_text();
        if selected.is_empty() {
            return;
        }
        self.clipboard.copy(selected);
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

    /// A picture, which markdown has no way of holding: it is written beside the document
    /// as a PNG of its own, and what goes into the block is the line naming that file.
    fn paste_picture(&mut self, png: &[u8]) {
        match storage::write_picture(self.editor.path(), png) {
            Ok(file) => {
                self.editor.error = None;
                self.edit(|editor| editor.insert_picture(&file));
                let ordinal = parse::image_paths(&self.editor.source())
                    .iter()
                    .position(|reference| reference == &file)
                    .expect("the picture reference was just inserted");
                let path =
                    self.editor.path().parent().unwrap_or_else(|| Path::new(".")).join(&file);
                self.pictures.push(PastedPicture { path, reference: file, ordinal, saved: false });
            }
            Err(error) => self.editor.error = Some(format!("Could not save the picture: {error}")),
        }
    }

    /// Save the document and make the pasted files agree with its image references. This
    /// is the only time a rename touches the filesystem, so typing a filename stays cheap.
    fn save(&mut self) -> bool {
        let desired = self.picture_references();
        let mut targets = Vec::with_capacity(desired.len());
        for (picture, reference) in self.pictures.iter().zip(&desired) {
            let target = match reference {
                Some(reference) => match picture_target(self.editor.path(), reference) {
                    Ok(target) => Some(target),
                    Err(_) => {
                        self.editor.error =
                            Some(format!("Could not rename the picture to {reference}"));
                        return false;
                    }
                },
                None => None,
            };
            if let Some(target) = &target
                && target != &picture.path
                && target.exists()
            {
                self.editor.error = Some(format!(
                    "Could not rename the picture: {} already exists",
                    target.display()
                ));
                return false;
            }
            targets.push(target);
        }

        let mut renamed = Vec::new();
        for (picture, target) in self.pictures.iter().zip(&targets) {
            let Some(target) = target else { continue };
            if target == &picture.path {
                continue;
            }
            if let Err(error) = fs::rename(&picture.path, target) {
                for (from, to) in renamed.iter().rev() {
                    let _ = fs::rename(from, to);
                }
                self.editor.error = Some(format!("Could not rename the picture: {error}"));
                return false;
            }
            renamed.push((target.clone(), picture.path.clone()));
        }

        if !self.editor.save() {
            for (from, to) in renamed.iter().rev() {
                let _ = fs::rename(from, to);
            }
            return false;
        }

        for ((picture, reference), target) in self.pictures.iter_mut().zip(&desired).zip(targets) {
            if let (Some(reference), Some(target)) = (reference, target) {
                picture.path = target;
                picture.reference = reference.clone();
                picture.saved = true;
            }
        }
        for (picture, reference) in self.pictures.iter().zip(&desired) {
            if reference.is_none()
                && let Err(error) = fs::remove_file(&picture.path)
                && error.kind() != io::ErrorKind::NotFound
            {
                self.editor.error =
                    Some(format!("Could not remove the unreferenced picture: {error}"));
            }
        }
        let mut index = 0;
        self.pictures.retain(|_| {
            let keep = desired[index].is_some();
            index += 1;
            keep
        });
        true
    }

    /// Match a pasted picture by its current name first, then by the image's position.
    /// The latter is what makes editing only the destination count as a rename.
    fn picture_references(&self) -> Vec<Option<String>> {
        let references = parse::image_paths(&self.editor.source());
        let mut claimed = vec![false; references.len()];
        let mut desired = vec![None; self.pictures.len()];

        for (index, picture) in self.pictures.iter().enumerate() {
            let found = references
                .iter()
                .enumerate()
                .filter(|(at, reference)| {
                    !reference.is_empty() && !claimed[*at] && *reference == &picture.reference
                })
                .min_by_key(|(at, _)| at.abs_diff(picture.ordinal));
            if let Some((at, reference)) = found {
                claimed[at] = true;
                desired[index] = Some(reference.clone());
            }
        }
        for (index, picture) in self.pictures.iter().enumerate() {
            if desired[index].is_none()
                && picture.ordinal < references.len()
                && !references[picture.ordinal].is_empty()
                && !claimed[picture.ordinal]
            {
                claimed[picture.ordinal] = true;
                desired[index] = Some(references[picture.ordinal].clone());
            }
        }
        desired
    }

    /// Pictures introduced since the last successful save belong to discarded edits.
    fn discard_pictures(&mut self) {
        for picture in &self.pictures {
            if !picture.saved {
                let _ = fs::remove_file(&picture.path);
            }
        }
        self.pictures.retain(|picture| picture.saved);
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

fn picture_target(document: &Path, reference: &str) -> io::Result<PathBuf> {
    let relative = Path::new(reference);
    let mut components = relative.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "picture names must be filenames"));
    }
    Ok(document.parent().unwrap_or_else(|| Path::new(".")).join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Motion;
    use ratatui::backend::TestBackend;
    use ratatui::{TerminalOptions, Viewport};
    use ratatui_image::picker::Picker;
    use std::sync::{Arc, Mutex};

    /// A document of its own, so that a test writing into it disturbs nothing else, with
    /// the sample picture beside it for the block that names one.
    fn document(name: &str, source: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("markatui-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a temporary directory");
        let path = directory.join("post.md");
        std::fs::write(&path, source).expect("a document");
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/image.png");
        std::fs::copy(sample, directory.join("image.png")).expect("a picture beside it");
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

    /// Enter ends the block, and Shift+Enter is the line break inside it. A terminal that
    /// cannot tell the two apart leaves the writer one key for both, so there the older
    /// rule stands: the first press breaks the line and the second ends the block.
    #[test]
    fn ends_the_block_on_enter_and_breaks_the_line_where_the_terminal_can_say_so() {
        let path = document("enter", "one two");
        let mut app = app(&path);
        app.editor.activate(0, 3);
        app.act(Action::LineBreak);
        assert_eq!(app.editor.block(0), "one\n two");
        app.act(Action::Enter);
        assert_eq!(app.editor.blocks().len(), 2);
        assert_eq!(app.editor.block(1), " two");

        let mut legacy =
            App::open(&path, Gallery::new(Picker::halfblocks(), &path), Keyboard::Legacy);
        legacy.set_mode("plain");
        legacy.editor.activate(0, 3);
        legacy.act(Action::Enter);
        assert_eq!(legacy.editor.block(0), "one\n two", "the first press broke the block");
        legacy.act(Action::Enter);
        assert_eq!(legacy.editor.blocks().len(), 2);
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
    /// So: nothing of a frame goes out while the caret is on the screen, and it comes back
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

        assert_eq!(at(&sent, HIDE), Some(0), "the caret is off before a byte of it goes out");
        let shown = at(&sent, SHOW).expect("and back on once it has gone");
        // What is left after it comes back is putting it where it belongs, and nothing
        // else: anything more would be drawn with the caret on it.
        let tail = &sent[shown + SHOW.len()..];
        assert!(tail.starts_with(b"\x1b["), "only the caret being put back comes after");
        assert!(tail.len() <= 11, "{} bytes drawn with the caret on them", tail.len());
    }

    #[test]
    fn page_keys_scroll_and_move_the_cursor() {
        let source = (0..20).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n\n");
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

        app.page(1);
        frame(&mut app, &mut terminal);
        assert!(app.scroll > 0, "PageDown left the image in place");

        app.page(-1);
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
        let held = state::recall_mode();

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
            state::remember_mode(&held);
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
    const WITH_A_LINK: &str =
        "# Title\n\nA paragraph with [a link](https://example.com/x) in it.\n";

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
        app.point(pointer(
            event::MouseEventKind::Drag(event::MouseButton::Left),
            app.column.x + 8,
            3,
        ));

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
        app.point(pointer(
            event::MouseEventKind::Drag(event::MouseButton::Left),
            app.column.x + 1,
            3,
        ));

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

    /// Quitting over a file somebody else has written asks again rather than going quiet
    /// and taking the answer: the editor stays up with what happened at the foot of it.
    #[test]
    fn a_quit_over_a_changed_file_asks_a_second_time() {
        let path = document("quit-changed", "A document.");
        let mut app = app(&path);
        app.act(Action::Type("X".into()));
        std::fs::write(&path, "Somebody else.").expect("the file changes under the editor");
        app.mode = Mode::Quitting;

        app.act(Action::SaveAndQuit);
        assert!(!app.quit, "the editor quit over somebody else's work");
        assert!(app.footer(90)[0].contains("changed on disk"), "{:?}", app.footer(90));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Somebody else.");

        app.act(Action::SaveAndQuit);
        assert!(app.quit);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "XA document.");
        forget(&path);
    }
}
