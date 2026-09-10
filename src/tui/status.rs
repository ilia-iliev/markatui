//! What the foot of the screen says. Only the words: the band they are drawn in, and the
//! rows it takes, are the view's.

use super::{App, Mode};
use crate::editor::Field;
use crate::tui::theme;

impl App {
    /// What the foot of the screen says, from the top of the pile down: the question a
    /// quit asks, the question turning a check off asks, the search bar, a file that
    /// would not open or save, the mode the writer is in where it is not the plain one,
    /// and what the checker makes of where the cursor is standing. Every line of it is
    /// drawn in the prompt colours, which is what makes the band read as one.
    pub(super) fn footer(&self, width: u16) -> Vec<String> {
        if self.mode == Mode::Quitting {
            let question = match &self.editor.error {
                Some(error) => format!("Save changes?  {error}"),
                None => "Save changes?".to_string(),
            };
            return vec![question, "[y] [n] [esc]".to_string()];
        }
        if let Mode::Muting(rule) = &self.mode {
            return vec![format!("Never show {rule} again?"), "[y] [n] [esc]".to_string()];
        }
        if self.mode == Mode::Searching {
            return self.search_lines(width);
        }
        if let Some(first) = self.notice.first() {
            // A config with a great deal wrong with it is not worth the whole screen: one
            // line of it is shown, and the rest are counted on the end of that line.
            let rest = self.notice.len() - 1;
            let line = match rest {
                0 => first.clone(),
                rest => format!("{first}  and {rest} more"),
            };
            return vec![line];
        }
        if let Some(error) = &self.editor.error {
            return vec![error.clone()];
        }
        if self.reading {
            return vec!["READING".to_string()];
        }
        if !self.grammar {
            return vec!["GRAMMAR OFF".to_string()];
        }
        self.lint_line().into_iter().collect()
    }

    /// The word being looked for and what is going in its place, one line each, with the
    /// half the writer is typing into carrying the caret. Which occurrence of how many
    /// sits at the far edge of the column, so that the count stays put while the word is
    /// typed; asking for another occurrence of a word that has only the one moves
    /// nothing, and that is the answer given instead.
    fn search_lines(&self, width: u16) -> Vec<String> {
        let search = &self.editor.search;
        let counter = match () {
            _ if search.needle.is_empty() => String::new(),
            _ if search.alone => "only one".into(),
            _ => match search.choice {
                Some(choice) => format!("{}/{}", choice + 1, search.count),
                None => "no matches".into(),
            },
        };
        vec![
            self.search_line("FIND", &search.needle, Field::Needle, &counter, width),
            self.search_line(
                "SWAP",
                &search.replacement,
                Field::Replacement,
                "[tab] [enter] [ctrl+a all]",
                width,
            ),
        ]
    }

    /// One half of the bar: what it is for, what has been typed into it, and what it has
    /// to say for itself at the far edge of the column.
    fn search_line(
        &self,
        label: &str,
        typed: &str,
        field: Field,
        note: &str,
        width: u16,
    ) -> String {
        let caret = if self.editor.search.field == field { "_" } else { "" };
        let left = format!("{label} {typed}{caret}");
        let room = (theme::content_width().min(width) as usize)
            .saturating_sub(left.chars().count() + note.chars().count());
        format!("{left}{}{note}", " ".repeat(room.max(2)))
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
