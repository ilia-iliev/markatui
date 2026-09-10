//! Where the cursor goes, and what a selection does as it goes there. The rules at the
//! edges of a block are the Qt editor's: up and down leave it, and left and right only do
//! so while a selection is being drawn. Nothing here changes the text.

use super::Editor;
use crate::active::{Active, Step};

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

impl Editor {
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
    pub(super) fn go_to(&mut self, target: usize, extend: bool) {
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
        if let Some((_, at)) = self.anchor.filter(|(block, _)| *block == self.index) {
            self.anchor = None;
            self.active.pin(at);
        }
    }

    /// Open block `target` for editing, with the cursor at its far edge for the caller to
    /// move where it wants.
    pub(super) fn settle_in(&mut self, target: usize) {
        self.index = target.min(self.blocks.len() - 1);
        self.active = Active::new(&self.blocks[self.index], usize::MAX);
        self.edit_run = None;
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

    /// Take the selection out to `at` characters into block `target`, from wherever it
    /// started: a drag with the mouse, and Shift with a click, which is the same thing
    /// done in one movement.
    pub fn extend_to(&mut self, target: usize, at: usize) {
        self.active.start_selection();
        self.go_to(target.min(self.blocks.len() - 1), true);
        self.active.place(at);
        self.record_cursor();
    }

    /// Select the word `at` stands in, in the block the cursor is already in, which is
    /// what a double click asks for.
    pub fn select_word(&mut self, at: usize) {
        self.clear_selection();
        let (start, end) = self.active.word(at);
        self.active.select(start, end);
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
}
