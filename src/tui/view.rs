//! The document as rows on a screen: every block laid out, stacked with a blank row
//! between them, and painted into a column down the middle. Nothing here decides what a
//! block looks like — [`crate::layout`] does that — only where it goes and what colour it is.

use crate::editor::Editor;
use crate::layout::{self, Layout, Request};
use crate::lint;
use crate::text::char_at;
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use std::sync::Arc;

/// A blank row between blocks, which is the line height the Qt front end had.
const BLOCK_SPACING: usize = 1;
/// Rows of air above the first block, so the writing does not start hard against the
/// top of the terminal.
const TOP_MARGIN: usize = 1;

/// Every block laid out, and where each one starts once they are stacked.
#[derive(Default)]
pub struct Document {
    layouts: Vec<Layout>,
    /// The screen row each block starts on, with the height of the whole below the last.
    tops: Vec<usize>,
    /// What each layout was built from, so that a block nothing has touched is not laid
    /// out again on the next keystroke.
    sources: Vec<Arc<String>>,
    width: u16,
    /// The checker's generation the wash was drawn from.
    generation: u64,
    active: usize,
}

impl Document {
    pub fn height(&self) -> usize {
        self.tops.last().copied().unwrap_or(0)
    }

    /// The rows of block `index`.
    pub fn rows(&self, index: usize) -> &[layout::Row] {
        &self.layouts[index].rows
    }

    /// Where the caret sits on the screen, as a row and a column into the column of text.
    pub fn caret(&self) -> Option<(usize, u16)> {
        let layout = self.layouts.get(self.active)?;
        let (row, column) = layout.caret?;
        Some((self.tops[self.active] + row, column))
    }

    /// The screen row block `index` starts on.
    pub fn top(&self, index: usize) -> usize {
        self.tops[index]
    }

    /// Which block a screen row belongs to, and which of that block's rows it is. A row
    /// in the gap between two blocks belongs to the one above it.
    pub fn at(&self, row: usize) -> Option<(usize, usize)> {
        let index = self.tops.iter().rposition(|top| *top <= row)?;
        let index = index.min(self.layouts.len().checked_sub(1)?);
        let within = row - self.tops[index];
        (within < self.layouts[index].rows.len()).then_some((index, within))
    }

    /// Lay out every block that has changed since the last frame, and stack them again.
    /// A block the writer has not touched keeps the rows it already had.
    pub fn rebuild(&mut self, editor: &Editor, width: u16) {
        let generation = lint::generation();
        let fresh = width != self.width || generation != self.generation;
        let blocks = editor.blocks();
        let mut layouts = Vec::with_capacity(blocks.len());
        let mut sources = Vec::with_capacity(blocks.len());

        for (index, source) in blocks.iter().enumerate() {
            let active = index == editor.index();
            let kept = (!fresh && !active && index != self.active)
                .then(|| self.reuse(index, source))
                .flatten();
            layouts.push(kept.unwrap_or_else(|| {
                let text = editor.block(index);
                // Nothing is said about a block while it is being typed into: a word
                // half-written is not a word spelled wrong.
                let marks = if active && !editor.settled() { Vec::new() } else { lint::marks(text) };
                layout::block(Request {
                    text,
                    cursor: active.then(|| editor.active().cursor()),
                    width,
                    lints: &marks,
                })
            }));
            sources.push(source.clone());
        }

        self.tops = stacked(&layouts);
        self.layouts = layouts;
        self.sources = sources;
        self.width = width;
        self.generation = generation;
        self.active = editor.index();
    }

    /// The layout block `index` already had, if it was built from this very source. The
    /// blocks either side of an edit shift along, so the source is matched rather than
    /// the position: a paragraph typed above does not re-lay out the ten below it.
    fn reuse(&self, index: usize, source: &Arc<String>) -> Option<Layout> {
        let cached = self.sources.get(index)?;
        Arc::ptr_eq(cached, source).then(|| self.layouts[index].clone())
    }
}

/// Where each block starts once they are stacked with a blank row between them.
fn stacked(layouts: &[Layout]) -> Vec<usize> {
    let mut tops = Vec::with_capacity(layouts.len() + 1);
    let mut row = TOP_MARGIN;
    for layout in layouts {
        tops.push(row);
        row += layout.rows.len() + BLOCK_SPACING;
    }
    tops.push(row);
    tops
}

/// Draw the column of text. `scroll` is the screen row at the top of the area.
pub fn draw(frame: &mut Frame, area: Rect, editor: &Editor, document: &Document, scroll: usize) {
    let width = theme::CONTENT_WIDTH.min(area.width);
    let left = area.x + (area.width - width) / 2;
    let selection = Selection::of(editor);

    for line in 0..area.height {
        let row = scroll + line as usize;
        let Some((index, within)) = document.at(row) else { continue };
        paint(frame, Rect { x: left, y: area.y + line, width, height: 1 },
              &document.rows(index)[within], index, &selection);
    }

    if let Some((row, column)) = document.caret()
        && row >= scroll
        && row < scroll + area.height as usize
        && column < width
    {
        frame.set_cursor_position(Position::new(left + column, area.y + (row - scroll) as u16));
    }
}

fn paint(frame: &mut Frame, area: Rect, row: &layout::Row, index: usize, selection: &Selection) {
    let buffer = frame.buffer_mut();
    let ground = theme::ground(row.bits);
    let mut column = 0u16;

    for cell in &row.cells {
        let style = shade(theme::of(cell, row.bits), cell.source, index, selection);
        // A rule and a fence border are one character told to reach the far edge.
        let repeat = if cell.bits & layout::FILL != 0 { area.width - column } else { cell.width };
        for step in 0..repeat.max(cell.width) {
            if column + step >= area.width {
                break;
            }
            let at = Position::new(area.x + column + step, area.y);
            let text = if step == 0 || cell.bits & layout::FILL != 0 { &cell.text } else { "" };
            buffer[at].set_symbol(text).set_style(style);
        }
        column += repeat.max(cell.width);
        if column >= area.width {
            return;
        }
    }
    for rest in column..area.width {
        buffer[Position::new(area.x + rest, area.y)].set_symbol(" ").set_style(ground);
    }
}

fn shade(style: Style, source: Option<usize>, index: usize, selection: &Selection) -> Style {
    match selection.covers(index, source) {
        true => theme::selected(style),
        false => style,
    }
}

/// Which of each block is selected, in characters. A block between the two ends of a
/// selection is covered whole, gaps and all, so that the shape reads as one.
struct Selection {
    first: usize,
    first_at: usize,
    last: usize,
    last_at: usize,
    running: bool,
}

impl Selection {
    fn of(editor: &Editor) -> Self {
        let Some(span) = editor.selection() else {
            return Selection { first: 0, first_at: 0, last: 0, last_at: 0, running: false };
        };
        Selection {
            first: span.first,
            first_at: char_at(editor.block(span.first), span.first_at),
            last: span.last,
            last_at: char_at(editor.block(span.last), span.last_at),
            running: true,
        }
    }

    /// Whether the character at `source` in block `index` falls inside the selection.
    /// A cell the layout made up is never selected: there is nothing of it to copy.
    fn covers(&self, index: usize, source: Option<usize>) -> bool {
        let Some(at) = source.filter(|_| self.running) else { return false };
        if index < self.first || index > self.last {
            return false;
        }
        (index > self.first || at >= self.first_at) && (index < self.last || at < self.last_at)
    }
}

/// The foot of the screen: the checker's message, or the search bar, or the quit prompt.
/// A band right across, with the words in the same column as the text above them.
pub fn footer(frame: &mut Frame, area: Rect, text: &str, style: Style) {
    let width = theme::CONTENT_WIDTH.min(area.width);
    let left = area.x + (area.width - width) / 2;
    let buffer = frame.buffer_mut();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            buffer[Position::new(x, y)].set_symbol(" ").set_style(style);
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text.to_string(), style))).wrap(Wrap { trim: false }),
        Rect { x: left, y: area.y, width, height: area.height },
    );
}
