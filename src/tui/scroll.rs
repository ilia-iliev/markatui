//! Where the window sits over the document, and where a keystroke that moves by rows
//! rather than by characters lands. The rows are the layout's; what is here is only which
//! of them the writer is looking at and which one the caret goes to.

use super::{App, MARGIN};
use crate::active::Step;
use crate::editor::Motion;
use std::ops::Range;

impl App {
    /// Up and down, which move by the rows on the screen rather than by the lines of the
    /// source: a wrapped paragraph is several rows and one line. At the top and bottom of
    /// a block the cursor leaves it, which is the rule the Qt editor had.
    pub(super) fn step_row(&mut self, step: Step, extend: bool) {
        let Some((row, column)) = self.caret_within_block() else {
            return self.editor.move_cursor(Motion::Line(step), extend);
        };
        let goal = *self.goal.get_or_insert(column);
        let rows = self.document.rows(self.editor.index());
        let landing = row.checked_add_signed(step as isize).filter(|at| *at < rows.len());
        match landing.and_then(|at| rows[at].source_at(goal)) {
            Some(at) => self.editor.place_cursor(at, extend),
            None => self.editor.move_cursor(Motion::Line(step), extend),
        }
    }

    fn caret_within_block(&self) -> Option<(usize, u16)> {
        let (row, column) = self.document.caret()?;
        Some((row - self.document.top(self.editor.index()), column))
    }

    /// Page through the document a screenful at a time. The window and caret travel
    /// together, so the next screenful replaces this one rather than merely bringing its
    /// first block into view.
    pub(super) fn page(&mut self, step: Step) {
        // Rendered images and tables deliberately have no caret mapping. Their first row
        // still anchors a page movement, so reading mode can page away from them.
        let (row, column) =
            self.document.caret().unwrap_or_else(|| (self.document.top(self.editor.index()), 0));
        let height = self.document.height();
        let target = row
            .saturating_add_signed(step as isize * self.viewport as isize)
            .min(height.saturating_sub(1));
        // A target can be in the air between blocks. Look on past it first, then back
        // from the edge of the document when there is no more text that way.
        let onwards = if step > 0 { target..height } else { 0..target.saturating_add(1) };
        let back = if step > 0 { 0..target } else { target.saturating_add(1)..height };
        let landing = self.nearest(onwards, step).or_else(|| self.nearest(back, -step));
        let Some((index, within)) = landing else {
            return;
        };
        let at = self.document.rows(index)[within].source_at(column).unwrap_or(0);

        let distance = step as isize * self.viewport as isize;
        let last_page = height.saturating_sub(self.viewport);
        self.scroll = self.scroll.saturating_add_signed(distance).min(last_page);
        self.editor.activate(index, at);
    }

    /// The first row of `range` with text on it, scanned from the end a movement of
    /// `step` arrives at: downwards from its start, upwards from its end.
    pub(super) fn nearest(&self, range: Range<usize>, step: Step) -> Option<(usize, usize)> {
        if step > 0 {
            range.into_iter().find_map(|row| self.document.at(row))
        } else {
            range.into_iter().rev().find_map(|row| self.document.at(row))
        }
    }

    /// Keep the caret on the screen, and a few rows clear of the bottom of it.
    pub(super) fn follow_the_caret(&mut self, height: usize) {
        let Some((row, _)) = self.document.caret() else { return };
        let margin = MARGIN.min(height / 4);
        if row < self.scroll + margin {
            self.scroll = row.saturating_sub(margin);
        }
        if row + margin >= self.scroll + height {
            self.scroll = row + margin + 1 - height;
        }
    }
}
