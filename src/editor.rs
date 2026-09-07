//! The document: the blocks it is held as, the block being edited, what the checker
//! makes of it, what the search is looking at, and the undo behind all of it. This is
//! what the Qt front end held minus the Qt, so none of it knows there is a terminal.

use crate::active::{Active, Step};
use crate::blocks::{self, Span};
use crate::lint;
use crate::parse;
use crate::search;
use crate::state;
use crate::storage;
use std::collections::VecDeque;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const UNDO_LIMIT: usize = 512;

/// A way of moving the cursor. Everything that is not a plain step is here so that the
/// key layer above names an intention and nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Character(Step),
    Word(Step),
    Line(Step),
    LineEdge(Step),
    Block(Step),
    Document(Step),
}

/// What the checker has to say about where the cursor is standing.
#[derive(Default)]
pub struct LintState {
    pub message: String,
    /// Every suggestion offered, of which `choice` names the one on show.
    pub replacements: Vec<String>,
    pub choice: usize,
    /// Where a suggestion would go, in characters, or nowhere when none was offered.
    pub at: Option<usize>,
    pub len: usize,
    /// The misspelled word, where that is what the checker objected to. Empty for a turn
    /// of phrase, which is nothing a dictionary has an opinion about.
    pub word: String,
    /// The block the message above was worked out for.
    block: Option<usize>,
}

impl LintState {
    pub fn suggestion(&self) -> Option<&str> {
        self.replacements.get(self.choice).map(String::as_str)
    }
}

/// What the search is looking at, as the foot of the screen needs to read it.
#[derive(Default)]
pub struct SearchState {
    pub open: bool,
    pub needle: String,
    pub count: usize,
    /// Which occurrence is on show, or nowhere when the word is not in the document.
    pub choice: Option<usize>,
    /// Whether the writer asked for another occurrence of a word that has only the one.
    /// Nothing moves, so the foot of the screen says why.
    pub alone: bool,
    found: search::Search,
}

#[derive(Clone)]
struct Undo {
    // Blocks and gaps are shared with the live document. An undo point is made on every
    // edit; cloning every allocation on every keystroke quickly dwarfs the document.
    blocks: Vec<Arc<String>>,
    gaps: Vec<Arc<String>>,
    index: usize,
    cursor: usize,
    anchor: Option<(usize, usize)>,
    revision: u64,
}

pub struct Editor {
    blocks: Vec<Arc<String>>,
    /// Source around the blocks: before, between, and after. Keeping it separately lets
    /// the view stay block-oriented without normalizing the file on save.
    gaps: Vec<Arc<String>>,
    /// The block under the cursor, as text that can be typed into.
    active: Active,
    index: usize,
    /// Where a selection that has left the block it started in is pinned: the block, and
    /// how far into it. A selection inside one block is the active block's own.
    anchor: Option<(usize, usize)>,
    undo: VecDeque<Undo>,
    revision: u64,
    saved_revision: u64,
    /// Whether the newest undo state is a run of typing that is still being added to.
    typing: bool,
    /// Whether the typing has stopped. Nothing is said about a block while it is being
    /// typed into — a word half-written is not a word spelled wrong.
    settled: bool,
    path: PathBuf,
    pub error: Option<String>,
    pub lint: LintState,
    pub search: SearchState,
}

impl Editor {
    /// Open `path`, resolved against the working directory. A path that does not exist
    /// yet starts an empty document that [`Editor::save`] will create.
    pub fn open(path: &Path) -> Self {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        };
        let (segments, error) = match fs::read_to_string(&path) {
            Ok(source) => (parse::segments(&source), None),
            Err(error) if error.kind() == ErrorKind::NotFound => (parse::segments(""), None),
            Err(error) => (
                parse::segments(""),
                Some(format!("Could not open {}: {error}", path.display())),
            ),
        };

        let blocks: Vec<Arc<String>> = segments.blocks.into_iter().map(Arc::new).collect();
        let last = blocks.len() - 1;
        // Pick up where the last session left off in this file, or at its end.
        let index = state::recall(&path).unwrap_or(last).min(last);
        let mut editor = Editor {
            active: Active::new(&blocks[index], usize::MAX),
            blocks,
            gaps: segments.gaps.into_iter().map(Arc::new).collect(),
            index,
            anchor: None,
            undo: VecDeque::new(),
            revision: 0,
            saved_revision: 0,
            typing: false,
            settled: true,
            path,
            error,
            lint: LintState::default(),
            search: SearchState::default(),
        };
        editor.record_cursor();
        editor
    }

    // ---- what the view reads ---------------------------------------------------

    pub fn blocks(&self) -> &[Arc<String>] {
        &self.blocks
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn active(&self) -> &Active {
        &self.active
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// The source of block `index` as the view should draw it: the block being edited is
    /// whatever has been typed into it, which the stored copy only catches up with on
    /// the next keystroke.
    pub fn block(&self, index: usize) -> &str {
        if index == self.index {
            self.active.text()
        } else {
            &self.blocks[index]
        }
    }

    /// Which blocks a selection covers, and how much of the first and last. `None` where
    /// nothing is selected.
    pub fn selection(&self) -> Option<Span> {
        match self.anchor {
            Some((block, at)) => {
                blocks::span(&self.blocks, block, at, self.index, self.active.cursor())
            }
            None => {
                let (start, end) = self.active.selection()?;
                blocks::span(&self.blocks, self.index, start, self.index, end)
            }
        }
    }

    /// The markdown between the two ends of the selection, as it would be written to disk.
    pub fn selected_text(&self) -> String {
        match self.selection() {
            Some(span) => blocks::selected_text(&self.blocks, &self.gaps, span),
            None => String::new(),
        }
    }

    // ---- moving ----------------------------------------------------------------

    /// Move the cursor, carrying a selection along with it where `extend` says so.
    /// The rules at the edges of a block are the Qt editor's: up and down leave it, and
    /// left and right only do so while a selection is being drawn.
    pub fn move_cursor(&mut self, motion: Motion, extend: bool) {
        if extend {
            self.active.start_selection();
        } else {
            self.clear_selection();
        }
        match motion {
            Motion::Character(step) => self.step_character(step, extend),
            Motion::Word(step) => self.active.step_word(step),
            Motion::Line(step) => self.step_line(step, extend),
            Motion::LineEdge(step) => self.active.to_line_edge(step),
            Motion::Block(step) => self.active.to_block_edge(step),
            Motion::Document(step) => self.go_to_document_edge(step, extend),
        }
        self.record_cursor();
    }

    fn step_character(&mut self, step: Step, extend: bool) {
        let at_edge = self.active.cursor() == if step > 0 { self.active.length() } else { 0 };
        // The plain arrows have always stopped at a block's ends; only a selection
        // being drawn carries on into the next one.
        if at_edge && extend {
            self.leave(step, extend);
            return;
        }
        self.active.step(step);
    }

    fn step_line(&mut self, step: Step, extend: bool) {
        if !self.active.step_line(step) {
            self.leave(step, extend);
        }
    }

    fn go_to_document_edge(&mut self, step: Step, extend: bool) {
        let target = if step > 0 { self.blocks.len() - 1 } else { 0 };
        self.go_to(target, extend);
        self.active.to_block_edge(step);
    }

    /// Move into the neighbouring block, keeping to the edge the cursor comes in by, so
    /// that one press of an arrow takes in one line rather than a whole block.
    fn leave(&mut self, step: Step, extend: bool) {
        let Some(target) = self.neighbour(step) else { return };
        self.go_to(target, extend);
        self.active.to_block_edge(-step);
    }

    fn neighbour(&self, step: Step) -> Option<usize> {
        if step > 0 {
            (self.index + 1 < self.blocks.len()).then(|| self.index + 1)
        } else {
            self.index.checked_sub(1)
        }
    }

    /// Put the cursor in block `target`. A selection being drawn is pinned where it
    /// started before the block it started in is left behind.
    fn go_to(&mut self, target: usize, extend: bool) {
        if target == self.index {
            return;
        }
        if !extend {
            let delta = self.commit();
            let target =
                if self.index < target { target.saturating_add_signed(delta) } else { target };
            self.settle_in(target);
            return;
        }
        // Nothing re-parses under a selection: the block being left keeps its shape, so
        // the rows the selection covers stay where they were while it grows.
        self.anchor
            .get_or_insert((self.index, self.active.anchor().unwrap_or(self.active.cursor())));
        self.store_active();
        self.settle_in(target);
        // Back in the block the selection is pinned in, it is that block's own again,
        // pinned where it started rather than at the edge the cursor came in by.
        if let Some((block, at)) = self.anchor.filter(|(block, _)| *block == self.index) {
            let _ = block;
            self.anchor = None;
            self.active.pin(at);
        }
    }

    /// Open block `target` for editing, with the cursor at its far edge for the caller to
    /// move where it wants.
    fn settle_in(&mut self, target: usize) {
        self.index = target.min(self.blocks.len() - 1);
        self.active = Active::new(&self.blocks[self.index], usize::MAX);
        self.typing = false;
    }

    /// Take the cursor to block `target` and put it `at` characters in, which is what a
    /// search occurrence and a restored position both want.
    pub fn activate(&mut self, target: usize, at: usize) {
        self.clear_selection();
        self.go_to(target.min(self.blocks.len() - 1), false);
        self.active.place(at);
        self.record_cursor();
    }

    /// Put the cursor somewhere else in the block it is already in, which is what up and
    /// down do once the rows on the screen have said where that is.
    pub fn place_cursor(&mut self, at: usize, extend: bool) {
        if extend {
            self.active.start_selection();
        } else {
            self.clear_selection();
        }
        self.active.place(at);
        self.record_cursor();
    }

    pub fn select_all(&mut self) {
        self.store_active();
        self.anchor = Some((0, 0));
        self.go_to(self.blocks.len() - 1, true);
        self.active.to_block_edge(1);
        self.record_cursor();
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
        self.active.drop_selection();
    }

    // ---- editing ---------------------------------------------------------------

    /// Type `text` where the cursor is, over whatever is selected.
    pub fn insert(&mut self, text: &str) {
        if self.take_spanning_selection(text) {
            return;
        }
        self.active.insert(text);
        self.record_typing();
    }

    /// Take out the character the cursor stands against, or the selection where there is
    /// one. At the start of a block with nothing selected, the block merges into the one
    /// before it.
    pub fn delete(&mut self, step: Step) {
        if self.take_spanning_selection("") {
            return;
        }
        if self.active.delete(step) {
            self.record_edit();
            return;
        }
        if step < 0 {
            self.merge_with_previous();
        }
    }

    /// Break the block in two, or add a line to it. A second Enter ends the block rather
    /// than leaving a blank line in it — the newline the first one left is taken back out.
    pub fn enter(&mut self) {
        if self.take_spanning_selection("") {
            return;
        }
        if !self.active.ends_block() {
            self.insert("\n");
            return;
        }
        let (before, after) = self.active.split();
        let (mut split, mut separators) = blocks::replacement(&before);
        let head = split.len();
        let (tail, tail_separators) = blocks::replacement(&after);
        separators.push("\n\n".to_string());
        separators.extend(tail_separators);
        split.extend(tail);

        self.replace_block(self.index, split, separators);
        self.index += head;
        self.active = Active::new(&self.blocks[self.index], 0);
        self.record_edit();
    }

    fn merge_with_previous(&mut self) {
        let Some(previous) = self.index.checked_sub(1) else { return };
        let cursor = crate::text::length(&self.blocks[previous]);
        let tail = self.active.text().to_string();

        Arc::make_mut(&mut self.blocks[previous]).push_str(&tail);
        // The source between the two blocks goes with the seam; what followed the second
        // one now follows the joined block.
        self.gaps[self.index] = self.gaps[self.index + 1].clone();
        self.remove_blocks(self.index, 1);
        self.index = previous;
        self.active = Active::new(&self.blocks[previous], cursor);
        self.record_edit();
    }

    /// Replace a selection running through more than one block with `insert`, joining
    /// what is left of the blocks at its two ends into one. `false` where the selection
    /// is inside a single block, which the block sees to itself.
    fn take_spanning_selection(&mut self, insert: &str) -> bool {
        let Some(span) = self.anchor.and(self.selection()).filter(|span| span.first != span.last)
        else {
            return false;
        };
        let (kept, cursor) = blocks::spliced(&self.blocks, span, insert);
        self.blocks[span.first] = Arc::new(kept);
        self.gaps[span.first + 1] = self.gaps[span.last + 1].clone();
        self.remove_blocks(span.first + 1, span.last - span.first);
        self.anchor = None;
        self.index = span.first;
        self.active = Active::new(&self.blocks[span.first], cursor);
        self.record_edit();
        true
    }

    pub fn surround(&mut self, marker: &str) {
        self.clear_spanning_selection();
        self.active.surround(marker);
        self.record_edit();
    }

    pub fn insert_link(&mut self, prefix: &str) {
        self.clear_spanning_selection();
        self.active.insert_link(prefix);
        self.record_edit();
    }

    /// A selection that has left this block is let go before an edit that only makes
    /// sense inside one: there is no wrapping a marker round several blocks.
    fn clear_spanning_selection(&mut self) {
        if self.anchor.is_some() {
            self.anchor = None;
            self.active.drop_selection();
        }
    }

    /// Re-read the block being edited now that the cursor is leaving it, splitting it
    /// where the writer has typed a blank line. Returns the change in the block count.
    fn commit(&mut self) -> isize {
        self.store_active();
        let block = self.blocks[self.index].clone();
        // A block emptied out disappears, unless it is all that is left.
        if block.trim().is_empty() {
            if self.blocks.len() == 1 {
                return 0;
            }
            self.remove_blocks(self.index, 1);
            return -1;
        }
        let (replacement, separators) = blocks::replacement(&block);
        if replacement.len() == 1 && replacement[0] == *block {
            return 0;
        }
        self.replace_block(self.index, replacement, separators)
    }

    /// Swap block `index` for the blocks it re-parsed into.
    fn replace_block(
        &mut self,
        index: usize,
        replacement: Vec<String>,
        separators: Vec<String>,
    ) -> isize {
        let delta = replacement.len() as isize - 1;
        let mut replacement = replacement.into_iter();
        self.blocks[index] = Arc::new(replacement.next().unwrap_or_default());
        for (offset, block) in replacement.enumerate() {
            self.blocks.insert(index + 1 + offset, Arc::new(block));
        }
        for (offset, separator) in separators.into_iter().enumerate() {
            self.gaps.insert(index + 1 + offset, Arc::new(separator));
        }
        delta
    }

    fn remove_blocks(&mut self, first: usize, count: usize) {
        self.blocks.drain(first..first + count);
        // Keep the separator before the removed span and discard the rest of the source
        // occupied by it. Callers may have replaced that kept separator first.
        self.gaps.drain(first + 1..first + count + 1);
    }

    /// Put what has been typed into the block back into the document, so that saving and
    /// the undo history see it.
    fn store_active(&mut self) {
        if *self.blocks[self.index] != self.active.text() {
            self.blocks[self.index] = Arc::new(self.active.text().to_string());
        }
    }

    // ---- undo ------------------------------------------------------------------

    fn snapshot(&self) -> Undo {
        Undo {
            blocks: self.blocks.clone(),
            gaps: self.gaps.clone(),
            index: self.index,
            cursor: self.active.cursor(),
            anchor: self.anchor,
            revision: self.revision,
        }
    }

    /// An edit that stands on its own — a block split, a merge, a selection deleted.
    /// The whole of it is one thing to undo.
    fn record_edit(&mut self) {
        self.typing = false;
        self.push_undo();
    }

    /// A keystroke. A run of them is one thing to undo rather than one per letter: the
    /// writer means a word, not the letters of it. The run stays open until the typing
    /// stops, or until anything that is not typing happens.
    fn record_typing(&mut self) {
        let open = self.typing;
        self.typing = true;
        if open {
            self.store_active();
            self.revision += 1;
            self.refresh_newest();
            return;
        }
        self.push_undo();
    }

    fn push_undo(&mut self) {
        self.store_active();
        self.revision += 1;
        self.settled = false;
        let state = self.snapshot();
        self.undo.push_back(state);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.pop_front();
        }
        self.update_lint();
    }

    /// Something that is not an edit happened where the cursor is: the newest state is
    /// brought up to date rather than added to, and a run of typing is over.
    fn record_cursor(&mut self) {
        self.typing = false;
        self.refresh_newest();
    }

    fn refresh_newest(&mut self) {
        let state = self.snapshot();
        match self.undo.back_mut() {
            Some(last) => *last = state,
            None => self.undo.push_back(state),
        }
        self.update_lint();
    }

    pub fn undo(&mut self) {
        self.typing = false;
        if self.undo.len() < 2 {
            return;
        }
        self.undo.pop_back();
        let state = self.undo.back().expect("a history has an opening state").clone();
        self.blocks = state.blocks;
        self.gaps = state.gaps;
        self.index = state.index.min(self.blocks.len() - 1);
        self.anchor = state.anchor;
        self.revision = state.revision;
        self.active = Active::new(&self.blocks[self.index], state.cursor);
        self.update_lint();
    }

    // ---- the checker -----------------------------------------------------------

    /// Whether the typing has stopped. The checker has its say once the writer pauses.
    pub fn settle(&mut self, settled: bool) {
        if self.settled == settled {
            return;
        }
        self.settled = settled;
        self.typing = self.typing && !settled;
        self.update_lint();
    }

    pub fn settled(&self) -> bool {
        self.settled
    }

    /// Look again at where the cursor is standing. The checker comes up a moment after
    /// the first frame does, and a word taken into the dictionary changes what it would
    /// say about every block at once.
    pub fn refresh_lint(&mut self) {
        self.lint.block = None;
        self.update_lint();
    }

    fn update_lint(&mut self) {
        let found = self
            .settled
            .then(|| lint::at(self.active.text(), self.active.cursor()))
            .flatten();
        self.lint.block = Some(self.index);
        match found {
            Some(found) => {
                self.lint.message = found.message;
                self.lint.word = found.word;
                self.lint.len = found.len;
                // A lint with nothing to suggest has nothing to accept either, and says
                // so by having no span to put anything in.
                self.lint.at = (!found.replacements.is_empty()).then_some(found.at);
                self.lint.replacements = found.replacements;
            }
            None => self.lint = LintState { block: Some(self.index), ..LintState::default() },
        }
        self.lint.choice = 0;
    }

    /// Show the next suggestion for what the cursor is standing in, or the one before it.
    /// They wrap around; only one is ever shown.
    pub fn cycle_lint(&mut self, step: Step) {
        let count = self.lint.replacements.len();
        if count < 2 {
            return;
        }
        self.lint.choice = (self.lint.choice as isize + step as isize).rem_euclid(count as isize) as usize;
    }

    /// Put the suggestion on show where the checker objected.
    pub fn accept_lint(&mut self) {
        let (Some(at), Some(replacement)) = (self.lint.at, self.lint.suggestion().map(str::to_string))
        else {
            return;
        };
        self.active.accept(at, self.lint.len, &replacement);
        self.record_edit();
    }

    /// Take the misspelled word under the cursor into the writer's own dictionary. It is
    /// spelled right from here on, in this document and the next.
    pub fn learn(&mut self) {
        if self.lint.word.is_empty() {
            return;
        }
        lint::learn(&self.lint.word);
        self.refresh_lint();
    }

    // ---- the search ------------------------------------------------------------

    /// Open the search bar. The block being edited is re-read first: where a word turns
    /// up is worked out over the blocks as they will be once it is rendered again, so
    /// that walking to an occurrence never finds the document has moved underneath it.
    pub fn open_search(&mut self) {
        self.store_active();
        self.commit();
        self.index = self.index.min(self.blocks.len() - 1);
        self.active = Active::new(&self.blocks[self.index], self.active.cursor());
        self.clear_selection();
        self.search.open = true;
        self.search.alone = false;
        let needle = self.search.needle.clone();
        self.search_for(&needle);
    }

    /// The occurrence walked to is left selected: it is usually the very thing the
    /// writer opened the search to type over.
    pub fn close_search(&mut self) {
        self.search.found.forget();
        self.search.open = false;
        self.search.count = 0;
        self.search.choice = None;
        self.search.alone = false;
    }

    pub fn search_for(&mut self, needle: &str) {
        self.search.needle = needle.to_string();
        let found = self.search.found.look_for(&self.blocks, needle);
        self.search.alone = false;
        self.show_occurrence(found);
    }

    /// A word that turns up once has nowhere to walk to. Nothing moves, and the flag is
    /// what the foot of the screen says so with.
    pub fn cycle_search(&mut self, step: Step) {
        match self.search.found.walk(step) {
            Some(found) => self.show_occurrence(Some(found)),
            None => self.search.alone = self.search.found.count() == 1,
        }
    }

    /// Put `found` under the cursor, selected from its start to its end: a word found is
    /// a word to be typed over.
    fn show_occurrence(&mut self, found: Option<search::Occurrence>) {
        self.search.count = self.search.found.count() as usize;
        self.search.choice = usize::try_from(self.search.found.choice()).ok();
        let Some(found) = found else { return };
        self.anchor = None;
        self.go_to(found.block, false);
        self.active.select(found.at, found.end);
        self.record_cursor();
    }

    // ---- the file --------------------------------------------------------------

    /// The document as it would be written out.
    pub fn source(&self) -> String {
        let mut blocks = self.blocks.clone();
        blocks[self.index] = Arc::new(self.active.text().to_string());
        blocks::source(&blocks, &self.gaps)
    }

    pub fn save(&mut self) -> bool {
        self.store_active();
        if let Err(error) = storage::write_atomic(&self.path, self.source().as_bytes()) {
            let message = format!("Could not save {}: {error}", self.path.display());
            eprintln!("markatui: {message}");
            self.error = Some(message);
            return false;
        }
        self.error = None;
        self.remember_position();
        self.saved_revision = self.revision;
        true
    }

    /// Note where the cursor is so the next session can pick it up.
    pub fn remember_position(&self) {
        state::remember(&self.path, self.index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document made without touching the disk. The path is never written to.
    fn document(source: &str) -> Editor {
        let segments = parse::segments(source);
        let blocks: Vec<Arc<String>> = segments.blocks.into_iter().map(Arc::new).collect();
        let mut editor = Editor {
            active: Active::new(&blocks[0], 0),
            blocks,
            gaps: segments.gaps.into_iter().map(Arc::new).collect(),
            index: 0,
            anchor: None,
            undo: VecDeque::new(),
            revision: 0,
            saved_revision: 0,
            typing: false,
            settled: false,
            path: PathBuf::from("/nowhere/post.md"),
            error: None,
            lint: LintState::default(),
            search: SearchState::default(),
        };
        editor.record_cursor();
        editor
    }

    fn texts(editor: &Editor) -> Vec<String> {
        (0..editor.blocks.len()).map(|index| editor.block(index).to_string()).collect()
    }

    #[test]
    fn types_into_the_block_the_cursor_is_in() {
        let mut editor = document("one\n\ntwo");
        editor.activate(1, 0);
        editor.insert("X");
        assert_eq!(texts(&editor), ["one", "Xtwo"]);
        assert_eq!(editor.source(), "one\n\nXtwo");
    }

    #[test]
    fn breaks_a_block_in_two_on_the_second_enter() {
        let mut editor = document("one two");
        editor.activate(0, 3);
        editor.enter();
        assert_eq!(texts(&editor), ["one\n two"]);
        editor.enter();
        assert_eq!(texts(&editor), ["one", " two"]);
        assert_eq!(editor.index(), 1);
        assert_eq!(editor.source(), "one\n\n two");
    }

    #[test]
    fn merges_a_block_into_the_one_before_it() {
        let mut editor = document("one\n\ntwo");
        editor.activate(1, 0);
        editor.delete(-1);
        assert_eq!(texts(&editor), ["onetwo"]);
        assert_eq!(editor.active().cursor(), 3);
        assert_eq!(editor.source(), "onetwo");
    }

    #[test]
    fn leaves_the_first_block_alone_at_its_start() {
        let mut editor = document("one");
        editor.activate(0, 0);
        editor.delete(-1);
        assert_eq!(texts(&editor), ["one"]);
    }

    #[test]
    fn re_reads_a_block_typed_into_several_when_the_cursor_leaves_it() {
        let mut editor = document("one\n\ntwo");
        editor.activate(0, 3);
        editor.insert("\n\nhalf");
        editor.activate(1, 0);
        assert_eq!(texts(&editor), ["one", "half", "two"]);
        assert_eq!(editor.source(), "one\n\nhalf\n\ntwo");
    }

    #[test]
    fn drops_a_block_emptied_out() {
        let mut editor = document("one\n\ntwo");
        editor.activate(0, 3);
        for _ in 0..3 {
            editor.delete(-1);
        }
        editor.activate(1, 0);
        assert_eq!(texts(&editor), ["two"]);
    }

    #[test]
    fn carries_a_selection_across_blocks_and_reads_it_with_its_gaps() {
        let mut editor = document("alpha\n\nbeta\n\ngamma");
        editor.activate(0, 2);
        editor.move_cursor(Motion::Line(1), true);
        editor.move_cursor(Motion::Line(1), true);
        assert_eq!(editor.selected_text(), "pha\n\nbeta\n\n");
    }

    #[test]
    fn takes_out_a_selection_that_runs_through_several_blocks() {
        let mut editor = document("alpha\n\nbeta\n\ngamma");
        editor.activate(0, 2);
        editor.move_cursor(Motion::Line(1), true);
        editor.move_cursor(Motion::Line(1), true);
        editor.insert("X");
        // What was in front of the selection joined to what was behind it.
        assert_eq!(texts(&editor), ["alXgamma"]);
    }

    #[test]
    fn types_over_the_whole_document() {
        let mut editor = document("alpha\n\nbeta");
        editor.select_all();
        editor.insert("X");
        assert_eq!(texts(&editor), ["X"]);
    }

    /// A selection drawn out of a block and back into it is the block's own again, pinned
    /// where it started: what it covers is what it always covered.
    #[test]
    fn brings_a_selection_back_into_the_block_it_was_pinned_in() {
        let mut editor = document("alpha\n\nbeta");
        editor.activate(0, 2);
        editor.move_cursor(Motion::Line(1), true);
        assert_eq!(editor.selected_text(), "pha\n\n");
        editor.move_cursor(Motion::Line(-1), true);
        assert_eq!(editor.selected_text(), "pha");
        // And it is the block's own, so typing over it replaces exactly that much.
        editor.insert("X");
        assert_eq!(texts(&editor), ["alX", "beta"]);
    }

    #[test]
    fn selects_the_whole_document_and_reads_it_back() {
        let mut editor = document("alpha\n\nbeta");
        editor.select_all();
        assert_eq!(editor.selected_text(), "alpha\n\nbeta");
    }

    #[test]
    fn undoes_a_run_of_typing_as_one_thing() {
        let mut editor = document("one");
        editor.activate(0, 3);
        for letter in [" ", "t", "w", "o"] {
            editor.insert(letter);
        }
        assert_eq!(texts(&editor), ["one two"]);
        editor.undo();
        assert_eq!(texts(&editor), ["one"]);
    }

    #[test]
    fn ends_a_run_of_typing_when_the_writer_pauses() {
        let mut editor = document("");
        editor.insert("a");
        editor.settle(true);
        editor.settle(false);
        editor.insert("b");
        editor.undo();
        assert_eq!(texts(&editor), ["a"]);
    }

    #[test]
    fn undoes_a_split_and_a_merge_whole() {
        let mut editor = document("one two");
        editor.activate(0, 3);
        editor.enter();
        editor.enter();
        assert_eq!(texts(&editor), ["one", " two"]);
        editor.undo();
        assert_eq!(texts(&editor), ["one\n two"]);
    }

    #[test]
    fn is_dirty_only_while_it_differs_from_what_was_saved() {
        let mut editor = document("one");
        assert!(!editor.dirty());
        editor.insert("X");
        assert!(editor.dirty());
        editor.undo();
        assert!(!editor.dirty());
    }

    #[test]
    fn wraps_a_selection_in_a_marker() {
        let mut editor = document("bold word");
        editor.activate(0, 0);
        editor.move_cursor(Motion::Word(1), true);
        editor.surround("**");
        assert_eq!(texts(&editor), ["**bold** word"]);
    }

    #[test]
    fn walks_the_occurrences_of_a_word_and_selects_each_one() {
        let mut editor = document("a marker\n\nb marker");
        editor.search_for("marker");
        assert_eq!(editor.search.count, 2);
        assert_eq!(editor.index(), 0);
        assert_eq!(editor.active().selected_text(), "marker");
        editor.cycle_search(1);
        assert_eq!(editor.index(), 1);
        assert_eq!(editor.active().selected_text(), "marker");
    }

    #[test]
    fn says_when_a_word_it_found_has_nowhere_to_walk_to() {
        let mut editor = document("only once");
        editor.search_for("once");
        editor.cycle_search(1);
        assert!(editor.search.alone);
    }

    /// The checker loads on threads of its own; this waits for it once.
    fn checker() {
        lint::preload();
        while !lint::ready() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn takes_the_checkers_suggestion_in_one_key_and_gives_it_back_in_one() {
        checker();
        let mut editor = document("I recieve mail.");
        editor.settle(true);
        editor.activate(0, 4);
        assert_eq!(editor.lint.word, "recieve");
        assert_eq!(editor.lint.suggestion(), Some("receive"));

        editor.accept_lint();
        assert_eq!(texts(&editor), ["I receive mail."]);
        editor.undo();
        assert_eq!(texts(&editor), ["I recieve mail."]);
    }

    #[test]
    fn walks_the_suggestions_both_ways_and_comes_back_round() {
        checker();
        let mut editor = document("I recieve mail.");
        editor.settle(true);
        editor.activate(0, 4);
        let offered = editor.lint.replacements.len();
        assert!(offered > 1, "the dictionary has more than one guess");

        editor.cycle_lint(1);
        assert_eq!(editor.lint.choice, 1);
        editor.cycle_lint(-1);
        assert_eq!(editor.lint.choice, 0);
        editor.cycle_lint(-1);
        assert_eq!(editor.lint.choice, offered - 1);
    }

    /// Nothing is said about a block while it is being typed into: a word half-written is
    /// not a word spelled wrong.
    #[test]
    fn says_nothing_about_a_block_still_being_typed_into() {
        checker();
        let mut editor = document("I recieve mail.");
        editor.settle(true);
        editor.activate(0, 4);
        assert!(!editor.lint.message.is_empty());

        editor.settle(false);
        assert!(editor.lint.message.is_empty());
        editor.settle(true);
        assert!(!editor.lint.message.is_empty());
    }

    #[test]
    fn keeps_the_source_it_was_given_byte_for_byte() {
        let editor = document("# Title\n\n\nBody  \n\n[home]: https://example.com\n");
        assert_eq!(editor.source(), "# Title\n\n\nBody  \n\n[home]: https://example.com\n");
    }
}
