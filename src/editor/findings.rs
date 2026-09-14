//! What the checker and the search make of the document, as the foot of the screen needs
//! to read it. Both put the cursor somewhere and offer the writer something to take; what
//! they have in common is that neither is an edit until the writer says so.

use super::Editor;
use crate::active::{Active, Step};
use crate::lint;
use crate::search;
use crate::text::byte_offset;
use std::sync::Arc;

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
    /// Which of the checker's rules objected, for a writer who never wants to hear from
    /// it again. Empty for a misspelling, which is no rule of the checker's.
    pub rule: String,
}

impl LintState {
    pub fn suggestion(&self) -> Option<&str> {
        self.replacements.get(self.choice).map(String::as_str)
    }
}

/// Which half of the search bar the writer is typing into. Tab moves between them.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    #[default]
    Needle,
    Replacement,
}

/// What the search is looking at, as the foot of the screen needs to read it.
#[derive(Default)]
pub struct SearchState {
    pub open: bool,
    pub needle: String,
    /// What the writer is putting in place of the word. Like the word, it is gone the
    /// next time the bar is opened: a search is one piece of work, start to finish.
    pub replacement: String,
    pub field: Field,
    pub count: usize,
    /// Which occurrence is on show, or nowhere when the word is not in the document.
    pub choice: Option<usize>,
    /// Whether the writer asked for another occurrence of a word that has only the one.
    /// Nothing moves, so the foot of the screen says why.
    pub alone: bool,
    found: search::Search,
}

impl Editor {
    /// Whether the typing has stopped. The checker has its say once the writer pauses.
    pub fn settle(&mut self, settled: bool) {
        if self.settled == settled {
            return;
        }
        self.settled = settled;
        if settled {
            self.edit_run = None;
        }
        self.refresh_lint();
    }

    pub fn settled(&self) -> bool {
        self.settled
    }

    /// Look again at where the cursor is standing. The checker comes up a moment after
    /// the first frame does, and a word taken into the dictionary changes what it would
    /// say about every block at once.
    pub fn refresh_lint(&mut self) {
        let found =
            self.settled.then(|| lint::at(self.active.text(), self.active.cursor())).flatten();
        match found {
            Some(found) => {
                self.lint.message = found.message;
                self.lint.word = found.word;
                self.lint.rule = found.rule;
                self.lint.len = found.len;
                // A lint with nothing to suggest has nothing to accept either, and says
                // so by having no span to put anything in.
                self.lint.at = (!found.replacements.is_empty()).then_some(found.at);
                self.lint.replacements = found.replacements;
            }
            None => self.lint = LintState::default(),
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
        self.lint.choice =
            (self.lint.choice as isize + step as isize).rem_euclid(count as isize) as usize;
    }

    /// Put the suggestion on show where the checker objected.
    pub fn accept_lint(&mut self) {
        let (Some(at), Some(replacement)) =
            (self.lint.at, self.lint.suggestion().map(str::to_string))
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

    /// Open the search bar, empty: what was looked for last time is not what is wanted
    /// now, and a bar that comes up holding it is a bar to be cleared before it is used.
    /// The block being edited is re-read first: where a word turns up is worked out over
    /// the blocks as they will be once it is rendered again, so that walking to an
    /// occurrence never finds the document has moved underneath it.
    pub fn open_search(&mut self) {
        self.store_active();
        self.commit();
        self.index = self.index.min(self.blocks.len() - 1);
        self.active = Active::new(&self.blocks[self.index], self.active.cursor());
        self.clear_selection();
        self.search.open = true;
        self.search.alone = false;
        self.search.field = Field::Needle;
        self.search.replacement.clear();
        self.search_for("");
    }

    /// The occurrence walked to is left selected: it is usually the very thing the
    /// writer opened the search to type over.
    pub fn close_search(&mut self) {
        self.search.found.forget();
        self.search.open = false;
        self.search.count = 0;
        self.search.choice = None;
        self.search.alone = false;
        self.search.field = Field::Needle;
    }

    /// Swap the two halves of the bar. Both keep what was typed into them.
    pub fn switch_field(&mut self) {
        self.search.field = match self.search.field {
            Field::Needle => Field::Replacement,
            Field::Replacement => Field::Needle,
        };
    }

    /// What the writer is typing into now, to be handed back changed.
    pub fn field(&self) -> &str {
        match self.search.field {
            Field::Needle => &self.search.needle,
            Field::Replacement => &self.search.replacement,
        }
    }

    /// Take what was typed into the half of the bar the writer is in. Only the word being
    /// looked for sends the search off again; the replacement is not looked for.
    pub fn type_into_field(&mut self, text: &str) {
        match self.search.field {
            Field::Needle => self.search_for(text),
            Field::Replacement => self.search.replacement = text.to_string(),
        }
    }

    /// Put the replacement over the occurrence on show and walk to the next one. The
    /// document has moved underneath the search, so it looks again before it walks.
    pub fn replace_found(&mut self) {
        if self.search.choice.is_none() {
            return;
        }
        let replacement = self.search.replacement.clone();
        self.active.insert(&replacement);
        self.record_edit();
        self.look_again();
    }

    /// Every occurrence of the word swapped for the replacement, in one edit. Worked out
    /// over the blocks as they stand and applied from the back of each, so that the
    /// offsets ahead of a swap are still the offsets the search found.
    pub fn replace_all(&mut self) {
        self.store_active();
        let found = search::occurrences(&self.blocks, &self.search.needle);
        if found.is_empty() {
            return;
        }
        for occurrence in found.iter().rev() {
            let block = Arc::make_mut(&mut self.blocks[occurrence.block]);
            let range = byte_offset(block, occurrence.at)..byte_offset(block, occurrence.end);
            block.replace_range(range, &self.search.replacement);
        }
        self.index = self.index.min(self.blocks.len() - 1);
        self.active = Active::new(&self.blocks[self.index], self.active.cursor());
        self.anchor = None;
        self.record_edit();
        self.look_again();
    }

    /// Look for the word again over the document as it now stands, and put the first
    /// occurrence still in it under the cursor.
    fn look_again(&mut self) {
        self.store_active();
        let needle = self.search.needle.clone();
        self.search_for(&needle);
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
        self.search.count = self.search.found.count();
        self.search.choice = self.search.found.choice();
        let Some(found) = found else { return };
        self.anchor = None;
        self.go_to(found.block, false);
        self.active.select(found.at, found.end);
        self.record_cursor();
    }
}
