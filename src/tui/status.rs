//! What the foot of the screen says. Only the words: the band they are drawn in, and the
//! rows it takes, are the view's.

use super::{App, Mode};
use crate::tui::theme;

impl App {
    /// What the foot of the screen says, from the top of the pile down: the question a
    /// quit asks, the search bar, a file that would not open or save, and what the
    /// checker makes of where the cursor is standing. Every line of it is drawn in the
    /// prompt colours, which is what makes the band read as one.
    pub(super) fn footer(&self, width: u16) -> Vec<String> {
        if self.mode == Mode::Quitting {
            let question = match &self.editor.error {
                Some(error) => format!("Save changes?  {error}"),
                None => "Save changes?".to_string(),
            };
            return vec![question, "[y] [n] [esc]".to_string()];
        }
        if self.mode == Mode::Searching {
            return vec![self.search_line(width)];
        }
        if !self.notice.is_empty() {
            // A config with a great deal wrong with it is not worth the whole screen.
            const SHOWN: usize = 3;
            let mut lines: Vec<String> = self.notice.iter().take(SHOWN).cloned().collect();
            if let Some(rest) = self.notice.len().checked_sub(SHOWN).filter(|rest| *rest > 0) {
                lines.push(format!("and {rest} more"));
            }
            return lines;
        }
        if let Some(error) = &self.editor.error {
            return vec![error.clone()];
        }
        if !self.grammar {
            return Vec::new();
        }
        self.lint_line().into_iter().collect()
    }

    /// `find <word>` with which occurrence of how many at the far edge of the column, so
    /// that the count stays put while the word is typed. Asking for another occurrence of
    /// a word that has only the one moves nothing, and that is the answer given instead.
    fn search_line(&self, width: u16) -> String {
        let search = &self.editor.search;
        let counter = match () {
            _ if search.needle.is_empty() => String::new(),
            _ if search.alone => "only one".into(),
            _ => match search.choice {
                Some(choice) => format!("{}/{}", choice + 1, search.count),
                None => "no matches".into(),
            },
        };
        let typed = format!("find {}", search.needle);
        let room = (theme::content_width().min(width) as usize)
            .saturating_sub(typed.chars().count() + counter.chars().count());
        format!("{typed}{}{counter}", " ".repeat(room.max(2)))
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
