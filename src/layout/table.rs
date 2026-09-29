//! A table drawn as a table. Reached for a block that is one and does not hold the
//! cursor, or does in reading mode. The words in its cells map back to the source, so the
//! caret has somewhere to stand; the columns do not, so a table being edited is shown as
//! the markdown it was typed as instead.

use super::wrap::{caret_at, finish};
use super::{Cell, Layout, Line, Row, glyph, lines, padding};
use crate::marks;
use crate::style::{self, Cursor};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A table drawn as a table: the columns padded to the widest cell in each, and the pipes
/// and the dash row replaced by box drawing. Only the words map back to the source, which
/// is why a table under the cursor is shown as markdown instead, outside reading mode.
///
/// A table wider than the column runs over rather than being mangled to fit: that it does
/// not fit is the useful thing for the writer to see.
pub(super) fn table(text: &str, cursor: Cursor) -> Layout {
    // The row of dashes says how the columns read, not what is in them, and a terminal
    // draws every cell the same way: it is dropped rather than drawn.
    let rows: Vec<(usize, Vec<(String, usize)>)> = lines(text)
        .iter()
        .filter(|line| !marks::divider(&line.text))
        .map(|line| (line.at, cells(line)))
        .collect();
    let Some(columns) = rows.iter().map(|(_, row)| row.len()).max() else {
        return Layout::default();
    };
    let widths: Vec<u16> = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|(_, row)| row.get(column))
                .map(|(text, _)| text.width() as u16)
                .max()
                .unwrap_or(0)
        })
        .collect();

    let mut drawn = vec![border(&widths, "┌", "┬", "┐")];
    for (index, (at, row)) in rows.iter().enumerate() {
        drawn.push(table_row(row, *at, &widths, index == 0));
        if index == 0 {
            drawn.push(border(&widths, "├", "┼", "┤"));
        }
    }
    drawn.push(border(&widths, "└", "┴", "┘"));
    let caret = cursor.map(|at| caret_at(&drawn, at));
    Layout { rows: drawn, caret }
}

/// The cells of a source line, each with where its words start in the block.
fn cells(line: &Line) -> Vec<(String, usize)> {
    marks::split_row(&line.text)
        .into_iter()
        .map(|cell| {
            // Every cell is a slice of the line it was split from.
            let offset = cell.as_ptr() as usize - line.text.as_ptr() as usize;
            (cell.to_string(), line.at + line.text[..offset].chars().count())
        })
        .collect()
}

fn border(widths: &[u16], left: &str, join: &str, right: &str) -> Row {
    let mut cells = vec![glyph(left, style::MARKER)];
    for (index, width) in widths.iter().enumerate() {
        cells.extend((0..width + 2).map(|_| glyph("─", style::MARKER)));
        cells.push(glyph(if index + 1 == widths.len() { right } else { join }, style::MARKER));
    }
    Row { cells, bits: 0, slots: Vec::new() }
}

fn table_row(row: &[(String, usize)], at: usize, widths: &[u16], head: bool) -> Row {
    let bits = if head { style::BOLD } else { 0 };
    let mut cells = vec![glyph("│", style::MARKER)];
    for (index, width) in widths.iter().enumerate() {
        let (text, start) =
            row.get(index).map_or(("", at), |(text, start)| (text.as_str(), *start));
        cells.push(glyph(" ", 0));
        cells.extend(written(text, start, bits));
        cells.extend(padding(width.saturating_sub(text.width() as u16) + 1));
        cells.push(glyph("│", style::MARKER));
    }
    finish(cells, 0, at)
}

/// The words of a table cell, each character remembering where it was typed.
fn written(text: &str, start: usize, bits: u16) -> impl Iterator<Item = Cell> {
    text.graphemes(true).scan(start, move |source, part| {
        let mut cell = glyph(part, bits);
        cell.source = Some(*source);
        *source += part.chars().count();
        Some(cell)
    })
}
