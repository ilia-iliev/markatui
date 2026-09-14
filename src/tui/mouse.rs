//! The mouse: where a click lands in the document, which links Ctrl with a click
//! follows, and which way the wheel takes the window. Nothing here knows what a block looks like —
//! the rows and the columns are the layout's own, the very ones the arrow keys move
//! through, so a click lands where the caret would have.

use super::{App, Mode};
use crate::link;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::time::{Duration, Instant};

/// How soon after a click a second one in the same cell is a double click rather than
/// two clicks. The pair selects the word under the pointer.
const DOUBLE: Duration = Duration::from_millis(400);
/// How many rows one notch of the wheel takes the window.
const NOTCH: isize = 3;

impl App {
    /// What the pointer did. The wheel turns whatever the writer is being asked, being
    /// only the window moving; a click moves the cursor, so it waits until there is no
    /// question on the screen to move it behind.
    pub(super) fn point(&mut self, event: MouseEvent) {
        let editing = self.mode == Mode::Editing;
        match event.kind {
            MouseEventKind::ScrollUp => self.wheel(-NOTCH),
            MouseEventKind::ScrollDown => self.wheel(NOTCH),
            MouseEventKind::Down(MouseButton::Left) if editing => self.click(event),
            MouseEventKind::Drag(MouseButton::Left) if editing => self.drag(event),
            _ => {}
        }
    }

    /// The window moves and the cursor stays where it is, so a writer can read on and
    /// come back to the line they were writing. The caret goes off the screen with it,
    /// until the next keystroke brings the window back to it.
    fn wheel(&mut self, rows: isize) {
        let last = self.document.height().saturating_sub(self.viewport);
        self.scroll = self.scroll.saturating_add_signed(rows).min(last);
        self.follow = false;
    }

    fn click(&mut self, event: MouseEvent) {
        // Whatever the config could not read has been read by the writer by now: a click
        // is as much an answer to a notice as a keystroke is.
        self.notice.clear();
        self.follow = true;
        if let Some(url) = self.clicked_link(event) {
            self.editor.error = link::url(&url).err();
            return;
        }
        let Some((index, within, column)) = self.landing(event) else { return };
        if event.modifiers.contains(KeyModifiers::SHIFT) {
            return self.extend(index, within, column);
        }
        if self.double_clicked(event) {
            return self.select_word(index, within, column);
        }
        let at = self.document.rows(index)[within].source_at(column);
        self.editor.activate(index, at.unwrap_or(0));
    }

    /// Dragging with the button down takes the selection with it, which is the one way a
    /// mouse has of saying what to copy.
    fn drag(&mut self, event: MouseEvent) {
        self.follow = true;
        let Some((index, within, column)) = self.landing(event) else { return };
        self.extend(index, within, column);
    }

    /// Which of the document the pointer was over: the block, which of its rows, and how
    /// far into the row. A pointer in the air between two blocks belongs to the nearest
    /// row above it, and one below the last block to the last row there is.
    fn landing(&self, event: MouseEvent) -> Option<(usize, usize, u16)> {
        let column = self.column;
        if event.row < column.y || event.row >= column.bottom() {
            return None;
        }
        let row = self.scroll + (event.row - column.y) as usize;
        let (index, within) = self
            .nearest(0..row + 1, -1)
            .or_else(|| self.nearest(row..self.document.height(), 1))?;
        // Beside the column of text rather than in it: the near edge of the row, which is
        // where the writer was pointing.
        let at = event.column.saturating_sub(column.x).min(column.width.saturating_sub(1));
        Some((index, within, at))
    }

    /// The link Ctrl with a click means to follow, if the pointer was on one. A plain
    /// click never opens anything: it puts the caret where it landed, which is what a
    /// writer editing the words of a link is after, and Ctrl is how they say they meant
    /// the link itself.
    ///
    /// Whether the block is drawn as it reads or opened up as markdown makes no
    /// difference: the pointer has to be on a character of the link — its words either
    /// way, and its address where that is on show — and never out in the blank past the
    /// end of a row, which is what asking the row for a cell rules out.
    pub(super) fn clicked_link(&self, event: MouseEvent) -> Option<String> {
        if !event.modifiers.contains(KeyModifiers::CONTROL) {
            return None;
        }
        let (index, within, column) = self.landing(event)?;
        let cell = self.document.rows(index)[within].cell_at(column)?;
        self.editor.link_in(index, cell.source?)
    }

    fn extend(&mut self, index: usize, within: usize, column: u16) {
        // A table or a rendered picture has no place for a caret, and so no end for a
        // selection to be taken to: the drag passes over it rather than snapping to it.
        let Some(at) = self.document.rows(index)[within].source_at(column) else { return };
        self.editor.extend_to(index, at);
    }

    fn select_word(&mut self, index: usize, within: usize, column: u16) {
        let Some(at) = self.document.rows(index)[within].source_at(column) else { return };
        self.editor.activate(index, at);
        self.editor.select_word(at);
    }

    /// Whether this click is the second of a double click: soon after the last one and in
    /// the same cell. A third click in the same place starts a fresh pair rather than
    /// counting as another one.
    fn double_clicked(&mut self, event: MouseEvent) -> bool {
        let at = (event.column, event.row);
        let double = self.clicked.is_some_and(|(when, cell)| cell == at && when.elapsed() < DOUBLE);
        self.clicked = (!double).then(|| (Instant::now(), at));
        double
    }
}
