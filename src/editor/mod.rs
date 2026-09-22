//! The document: the blocks it is held as, the block being edited, what the checker
//! makes of it, what the search is looking at, and the undo behind all of it. This is
//! what the Qt front end held minus the Qt, so none of it knows there is a terminal.

mod file;
mod findings;
mod markup;
mod motion;
mod sections;
mod surgery;
mod undo;

pub use findings::{Field, LintState, SearchState};
pub use motion::Motion;

use crate::active::{Active, Step};
use crate::blocks::{self, Span};
use crate::marks;
use crate::parse;
use crate::style;
use crate::text::length;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
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
    /// What the undos took back, newest last, for a writer who has changed their mind
    /// twice. Emptied by the next edit.
    redo: Vec<Undo>,
    revision: u64,
    saved_revision: u64,
    /// The kind of repeated edit still being added to the newest undo state.
    edit_run: Option<EditRun>,
    /// Whether the typing has stopped. Nothing is said about a block while it is being
    /// typed into — a word half-written is not a word spelled wrong.
    settled: bool,
    /// Whether a picture the writer left among the words has been broken out into a
    /// paragraph of its own since the screen last looked. The foot of the screen says so.
    hoisted: bool,
    path: PathBuf,
    /// Set when the file would not read. The document is empty because of that and not
    /// because the file is, so saving it would write the emptiness over the writer's
    /// text. Nothing is saved until the editor is pointed at a file it can read.
    unreadable: bool,
    /// When the file was last seen — at open, and at every save. A file whose time has
    /// moved on since has been written by somebody else, and saving would go over them.
    seen: Option<SystemTime>,
    /// Whether the writer has said to write over the changes somebody else made. It
    /// holds until that save has gone through, and then the file is watched again.
    insisted: bool,
    pub error: Option<String>,
    pub lint: LintState,
    pub search: SearchState,
}

impl Editor {
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

    /// Whether a picture has been broken out of the words around it since this was last
    /// asked. The foot of the screen hears about it once, and shows it until the writer
    /// presses the next key.
    pub fn take_hoisted(&mut self) -> bool {
        std::mem::take(&mut self.hoisted)
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
        self.take(step, Active::delete);
    }

    /// The same, a word at a time: Ctrl with Backspace and Delete. A block whose edge
    /// the cursor is standing on has no word left to give, so it merges as it does under
    /// the plain keys.
    pub fn delete_word(&mut self, step: Step) {
        self.take(step, Active::delete_word);
    }

    fn take(&mut self, step: Step, from_block: impl Fn(&mut Active, Step) -> bool) {
        if self.take_spanning_selection("") {
            return;
        }
        let selection = self.active.selection().is_some();
        if from_block(&mut self.active, step) {
            if selection {
                self.record_edit();
            } else {
                self.record_run(EditRun::Deleting(step));
            }
            return;
        }
        match step < 0 {
            true => self.merge_with_previous(),
            false => self.merge_with_next(),
        }
    }

    /// Enter: a line break inside the paragraph, and a second press where the first left
    /// the cursor ends the block. A writer means the break far more often than the break
    /// in the document, and the press that ends the block is the one they have already
    /// made in every editor that asks for an empty line to end a list.
    ///
    /// What goes on by the line ends on the first press, because a break inside it is not
    /// a thing markdown has: a list opens the next item, a quote the next quoted line, a
    /// table the next row, a heading gives way to the paragraph under it, and a line left
    /// empty in any of them says that run is over. A fence has nothing Enter can end: a
    /// blank line in code is a blank line of code, so the block only ends where the cursor
    /// has come to rest past the fence that closes it.
    pub fn enter(&mut self) {
        if self.kind() == parse::Kind::Paragraph
            && self.active.item().is_none()
            && !self.active.ends_block()
        {
            return self.line_break();
        }
        self.end_block();
    }

    /// The half of Enter that ends the block: the second press in a paragraph, and the
    /// first everywhere the line break has no meaning of its own.
    fn end_block(&mut self) {
        if self.take_spanning_selection("") {
            return;
        }
        match self.kind() {
            parse::Kind::Code if !self.past_closing_fence() => return self.line_break(),
            parse::Kind::Table => return self.enter_row(),
            _ => {}
        }
        match self.active.item() {
            Some(item) if !item.empty => {
                self.active.insert(&format!("\n{}", item.next));
                self.record_edit();
                return;
            }
            // The empty item goes, and what is left of the block is broken off below it
            // the way any other Enter at the end of a line would break it.
            Some(item) => {
                self.active.end_item(&item);
                if !self.active.ends_block() {
                    self.record_edit();
                    return;
                }
            }
            None => {}
        }
        self.split_block();
    }

    /// Shift+Enter: a line break inside the block, which markdown reads as one line of a
    /// paragraph running on into the next.
    pub fn line_break(&mut self) {
        self.insert("\n");
    }

    /// Tab: a list item a level in or out, the next cell of a table, and a tab character
    /// anywhere else. Shift+Tab is the same key the other way, which types nothing.
    pub fn tab(&mut self, step: Step) {
        match self.kind() {
            parse::Kind::Table if self.active.step_cell(step) => self.record_cursor(),
            parse::Kind::List if self.active.indent_lines(step) => self.record_edit(),
            // A table with no cell that way and a list with no indent left to give back:
            // the key does nothing rather than typing into either of them.
            parse::Kind::Table | parse::Kind::List => {}
            _ if step > 0 => self.insert("\t"),
            _ => {}
        }
    }

    /// What the block being edited is, which is what says how Enter and Tab read.
    fn kind(&self) -> parse::Kind {
        parse::kind(self.active.text())
    }

    /// Another row under the one the cursor is in, with as many cells as that one and the
    /// cursor in the first of them. A row is not broken in the middle the way an item is:
    /// wherever the cursor stands in it, the new row goes under the whole of it. A row
    /// left empty says the table is done, the way an empty item ends a list.
    fn enter_row(&mut self) {
        self.active.to_row_end();
        let row = marks::row(self.active.line());
        if row.empty {
            self.active.end_item(&row);
            self.split_block();
            return;
        }
        let opened = self.active.cursor() + 1;
        self.active.insert(&format!("\n{}", row.next));
        self.active.place(opened);
        self.active.step_cell(1);
        self.record_edit();
    }

    /// Whether the cursor has come to rest past the fence that closes a code block, where
    /// there is no more code to write and Enter means the paragraph after it. A fence
    /// still being written has no closing line yet, and Enter in it is another line.
    fn past_closing_fence(&self) -> bool {
        let text = self.active.text();
        self.active.cursor() == length(text)
            && text.lines().count() > 1
            && text.lines().next_back().is_some_and(style::fences)
    }

    /// Put what has been typed into the block back into the document, so that saving and
    /// the undo history see it.
    fn store_active(&mut self) {
        if *self.blocks[self.index] != self.active.text() {
            self.blocks[self.index] = Arc::new(self.active.text().to_string());
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
