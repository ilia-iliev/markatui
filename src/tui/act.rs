//! What every command the writer can press does, one arm to a mode.
//!
//! The keymap says which command a keystroke is; this says what the command comes to.
//! The two are kept apart because a mode is the whole answer to what a key does — Enter
//! ends a block while the writer is in the document and closes the search while they are
//! in it — and the split is along the modes rather than along the keys.

use crate::editor::{Editor, Field};
use crate::tui::keys::Action;
use crate::tui::probe::Keyboard;
use crate::tui::{App, Mode};

impl App {
    /// Enter, which ends the block. A terminal that cannot tell Shift+Enter from Enter
    /// leaves the writer one key for the two things, so there it keeps the older rule: a
    /// first press leaves a line break and a second ends the block.
    fn enter(&mut self) {
        match self.keyboard {
            Keyboard::Kitty => self.edit(Editor::enter),
            Keyboard::Legacy => self.edit(Editor::enter_or_break),
        }
    }

    pub(super) fn act_editing(&mut self, action: Action) {
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

    pub(super) fn act_searching(&mut self, action: Action) {
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

    pub(super) fn act_quitting(&mut self, action: Action) {
        match action {
            // A failed save keeps the editor open rather than losing the text.
            Action::SaveAndQuit => self.quit = self.save(),
            Action::DiscardAndQuit => self.quit = true,
            Action::Cancel => self.mode = Mode::Editing,
            _ => {}
        }
    }

    pub(super) fn act_muting(&mut self, action: Action) {
        match action {
            Action::MuteCheck => self.mute_check(),
            Action::Cancel => self.mode = Mode::Editing,
            _ => {}
        }
    }
}
