//! The undo history: what a state of the document is, when one is taken, and what walking
//! back through them puts back. Every edit makes a state; a run of typing or of repeated
//! deletion adds to the newest one rather than making its own, so one undo takes back the
//! word rather than the letter.

use super::{EditRun, Editor};
use crate::active::Active;

use std::sync::Arc;

/// How many states of the document are kept.
const UNDO_LIMIT: usize = 512;

#[derive(Clone)]
pub(super) struct Undo {
    // Blocks and gaps are shared with the live document. An undo point is made on every
    // edit; cloning every allocation on every keystroke quickly dwarfs the document.
    blocks: Vec<Arc<String>>,
    gaps: Vec<Arc<String>>,
    index: usize,
    cursor: usize,
    anchor: Option<(usize, usize)>,
    revision: u64,
}

impl Editor {
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

    /// An edit that stands on its own — a block split or a selection deleted. The whole
    /// of it is one thing to undo.
    pub(super) fn record_edit(&mut self) {
        self.edit_run = None;
        self.push_undo();
    }

    /// A run of typing or repeated deletion is one thing to undo rather than one thing
    /// per character. Changing direction, moving, or pausing starts a new run.
    pub(super) fn record_typing(&mut self) {
        self.record_run(EditRun::Typing);
    }

    pub(super) fn record_run(&mut self, run: EditRun) {
        let open = self.edit_run == Some(run);
        self.edit_run = Some(run);
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
        self.refresh_lint();
    }

    /// Something that is not an edit happened where the cursor is: the newest state is
    /// brought up to date rather than added to, and any edit run is over.
    pub(super) fn record_cursor(&mut self) {
        self.edit_run = None;
        self.refresh_newest();
    }

    fn refresh_newest(&mut self) {
        let state = self.snapshot();
        match self.undo.back_mut() {
            Some(last) => *last = state,
            None => self.undo.push_back(state),
        }
        self.refresh_lint();
    }

    pub fn undo(&mut self) {
        self.edit_run = None;
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
        self.refresh_lint();
    }
}
