//! The document as rows on a screen: every block laid out, stacked with a blank row
//! between them, and painted into a column down the middle. Nothing here decides what a
//! block looks like — [`crate::layout`] does that — only where it goes and what colour it is.

use crate::editor::Editor;
use crate::layout::{self, Layout, Request};
use crate::lint;
use crate::parse;
use crate::text::char_at;
use crate::tui::images::{Gallery, Placed};
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
    /// The file each block is a picture of, where it is one, kept alongside the layout so
    /// that a block nothing has touched is not parsed again to find out.
    paths: Vec<Option<String>>,
    /// The blocks that have rows reserved for a picture, and what to draw in them.
    pictures: Vec<Placed>,
    /// How far the active block moved down the screen on the last rebuild.
    shift: isize,
    width: u16,
    /// The checker's generation the wash was drawn from.
    generation: u64,
    active: usize,
    grammar: bool,
    reading: bool,
}

impl Document {
    /// Set the display toggles. A change invalidates layouts which may contain revealed
    /// markdown or checker marks.
    pub fn configure(&mut self, grammar: bool, reading: bool) {
        if self.grammar != grammar || self.reading != reading {
            self.grammar = grammar;
            self.reading = reading;
            self.sources.clear();
        }
    }

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

    /// The pictures to draw, and which block each belongs to.
    pub fn pictures(&self) -> &[Placed] {
        &self.pictures
    }

    /// How far the active block moved down the screen when the document was last laid out
    /// again — rows appearing above it as a picture arrives, and nothing else. The window
    /// follows it, so what the writer is looking at stays where it was.
    pub fn shift(&self) -> isize {
        self.shift
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
    pub fn rebuild(&mut self, editor: &Editor, width: u16, gallery: &mut Gallery) {
        let generation = lint::generation();
        // Asked first and on its own: a picture that has just been read changes how many
        // rows its block takes, so the rows kept from the last frame are no use — and
        // taking it in is not something to leave to whether the other two are true.
        let arrived = gallery.settle();
        let fresh = arrived || width != self.width || generation != self.generation;
        let blocks = editor.blocks();
        // Where the active block stood before, so that rows appearing above it can be
        // taken off the scroll rather than shoving the writer's line down the screen.
        let was =
            (self.active == editor.index()).then(|| self.tops.get(self.active).copied()).flatten();
        let mut layouts = Vec::with_capacity(blocks.len());
        let mut sources = Vec::with_capacity(blocks.len());
        let mut paths = Vec::with_capacity(blocks.len());
        let mut pictures = Vec::new();

        for (index, source) in blocks.iter().enumerate() {
            let active = index == editor.index();
            let kept = (!fresh && !active && index != self.active)
                .then(|| self.reuse(index, source))
                .flatten();
            let path = match &kept {
                Some((_, path)) => path.clone(),
                None => parse::lone_image(editor.block(index)),
            };
            // A block holding the cursor is markdown, picture and all — except in reading
            // mode. Its picture is still asked after because asking is what keeps it.
            let rows = path.as_deref().and_then(|path| gallery.rows(path, width));
            let picture = rows.filter(|_| !active || self.reading);
            if let Some(path) = path.clone().filter(|_| picture.is_some()) {
                pictures.push(Placed { index, path });
            }
            layouts.push(match kept {
                Some((layout, _)) => layout,
                None => self.lay_out(editor, index, width, picture),
            });
            sources.push(source.clone());
            paths.push(path);
        }

        self.tops = stacked(&layouts);
        self.shift = was.map_or(0, |before| self.tops[editor.index()] as isize - before as isize);
        self.layouts = layouts;
        self.sources = sources;
        self.paths = paths;
        self.pictures = pictures;
        self.width = width;
        self.generation = generation;
        self.active = editor.index();
    }

    /// One block laid out afresh.
    fn lay_out(&self, editor: &Editor, index: usize, width: u16, picture: Option<u16>) -> Layout {
        let active = index == editor.index();
        let text = editor.block(index);
        // Nothing is said about a block while it is being typed into: a word half-written
        // is not a word spelled wrong.
        let marks = if !self.grammar || active && !editor.settled() {
            Vec::new()
        } else {
            lint::marks(text)
        };
        layout::block(Request {
            text,
            cursor: active.then(|| editor.active().cursor()),
            reveal: !self.reading,
            width,
            lints: &marks,
            picture,
        })
    }

    /// The layout block `index` already had, and the file it is a picture of, if it was
    /// built from this very source. The blocks either side of an edit shift along, so the
    /// source is matched rather than the position: a paragraph typed above does not
    /// re-lay out the ten below it.
    fn reuse(&self, index: usize, source: &Arc<String>) -> Option<(Layout, Option<String>)> {
        let cached = self.sources.get(index)?;
        Arc::ptr_eq(cached, source)
            .then(|| (self.layouts[index].clone(), self.paths[index].clone()))
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
    draw_with_caret(frame, area, editor, document, scroll, true);
}

/// Draw with either the terminal's caret or one painted into the cells. The latter keeps
/// an immediate terminal's native caret from being watched following the renderer around
/// the frame.
pub(super) fn draw_with_caret(
    frame: &mut Frame,
    area: Rect,
    editor: &Editor,
    document: &Document,
    scroll: usize,
    native_caret: bool,
) {
    let column = column(area);
    let selection = Selection::of(editor);

    for line in 0..area.height {
        let row = scroll + line as usize;
        let Some((index, within)) = document.at(row) else { continue };
        paint(
            frame,
            Rect { y: area.y + line, height: 1, ..column },
            &document.rows(index)[within],
            index,
            &selection,
        );
    }

    // A caret is a cell with its two colours swapped, whichever of the two draws it, and
    // a selected cell is already those two colours swapped: the character the caret is
    // against would come back out in the page's own colours and read as the one character
    // of the selection that was left out of it. So while a selection is running the caret
    // comes off, and the shape is left whole with its own edge saying where the caret is.
    if let Some((row, at)) = document.caret().filter(|_| !selection.running)
        && row >= scroll
        && row < scroll + area.height as usize
    {
        // A line that fills the column has no cell of its own left for the caret: the
        // place past its last character is the first column of the margin, which is
        // where the next character would go and still a place on the screen. Only a
        // terminal no wider than the column has no such place, and there the caret
        // stands on the last cell rather than off the edge.
        let x = (column.x + at).min(area.right().saturating_sub(1));
        let position = Position::new(x, area.y + (row - scroll) as u16);
        if native_caret {
            frame.set_cursor_position(position);
        } else {
            let style = frame.buffer_mut()[position].style();
            frame.buffer_mut()[position].set_style(theme::selected(style));
        }
    }
}

/// The column everything is drawn in: as wide as a line of prose should be, down the
/// middle of whatever room the terminal gives.
pub fn column(area: Rect) -> Rect {
    column_for(area, theme::content_width())
}

pub(crate) fn column_for(area: Rect, content_width: u16) -> Rect {
    let width = content_width.min(area.width);
    Rect { x: area.x + (area.width - width) / 2, width, ..area }
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

/// How many rows a footer message occupies in its left-aligned column.
pub fn footer_height(area: Rect, text: &str) -> u16 {
    Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .line_count(footer_column(area).width)
        .try_into()
        .unwrap_or(u16::MAX)
        .max(1)
}

/// The foot of the screen: the checker's message, or the search bar, or the quit prompt.
/// A band right across, with the words against its left edge.
pub fn footer(frame: &mut Frame, area: Rect, text: &str) {
    let style = theme::prompt();
    let column = footer_column(area);
    let buffer = frame.buffer_mut();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            buffer[Position::new(x, y)].set_symbol(" ").set_style(style);
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text.to_string(), style)))
            .wrap(Wrap { trim: false }),
        column,
    );
}

fn footer_column(area: Rect) -> Rect {
    Rect { width: theme::content_width().min(area.width), ..area }
}
