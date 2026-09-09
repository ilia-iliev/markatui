//! The document: the blocks it is held as, the block being edited, what the checker
//! makes of it, what the search is looking at, and the undo behind all of it. This is
//! what the Qt front end held minus the Qt, so none of it knows there is a terminal.

mod findings;
mod motion;
mod undo;

pub use findings::{LintState, SearchState};
pub use motion::Motion;

use crate::active::{Active, Step};
use crate::blocks::{self, Span};
use crate::parse;
use crate::state;
use crate::storage;
use std::collections::VecDeque;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use undo::Undo;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditRun {
    Typing,
    Deleting(Step),
}

/// What the checker has to say about where the cursor is standing.
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
    /// The kind of repeated edit still being added to the newest undo state.
    edit_run: Option<EditRun>,
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
            Err(error) => {
                (parse::segments(""), Some(format!("Could not open {}: {error}", path.display())))
            }
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
            edit_run: None,
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
        if index == self.index { self.active.text() } else { &self.blocks[index] }
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
        let selection = self.active.selection().is_some();
        if self.active.delete(step) {
            if selection {
                self.record_edit();
            } else {
                self.record_run(EditRun::Deleting(step));
            }
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
        self.record_run(EditRun::Deleting(-1));
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
    use crate::lint;

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
            edit_run: None,
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
    fn undoes_a_run_of_backspaces_as_one_thing() {
        let mut editor = document("one two");
        editor.activate(0, 7);
        for _ in 0..3 {
            editor.delete(-1);
        }
        assert_eq!(texts(&editor), ["one "]);
        editor.undo();
        assert_eq!(texts(&editor), ["one two"]);
    }

    #[test]
    fn changing_delete_direction_starts_a_new_undo_step() {
        let mut editor = document("abcdef");
        editor.activate(0, 3);
        editor.delete(-1);
        editor.delete(-1);
        editor.delete(1);
        editor.delete(1);
        assert_eq!(texts(&editor), ["af"]);

        editor.undo();
        assert_eq!(texts(&editor), ["adef"]);
        editor.undo();
        assert_eq!(texts(&editor), ["abcdef"]);
    }

    #[test]
    fn backspacing_across_a_block_boundary_stays_in_the_run() {
        let mut editor = document("one\n\ntwo");
        editor.activate(1, 1);
        editor.delete(-1);
        editor.delete(-1);
        assert_eq!(texts(&editor), ["onewo"]);
        editor.undo();
        assert_eq!(texts(&editor), ["one", "two"]);
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
        lint::preload(&lint::Checks::new());
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
