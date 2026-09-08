//! A table drawn as a table. Reached only for a block that is one and does not hold the
//! cursor: nothing here maps back to the source, so a table being edited is shown as the
//! markdown it was typed as instead.

use super::{Layout, Row, glyph, padding};
use crate::style;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A table drawn as a table: the columns padded to the widest cell in each, and the pipes
/// and the dash row replaced by box drawing. Nothing here maps back to the source, which
/// is why a table under the cursor is shown as markdown instead.
///
/// A table wider than the column runs over rather than being mangled to fit: that it does
/// not fit is the useful thing for the writer to see.
pub(super) fn table(text: &str) -> Layout {
    let rows: Vec<Vec<String>> = text
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches('|')
                .trim_end_matches('|')
                .split('|')
                .map(|column| column.trim().to_string())
                .collect()
        })
        .filter(|columns: &Vec<String>| !divider(columns))
        .collect();
    let Some(columns) = rows.iter().map(Vec::len).max() else {
        return Layout::default();
    };
    let widths: Vec<u16> = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|text| text.width() as u16)
                .max()
                .unwrap_or(0)
        })
        .collect();

    let mut drawn = vec![border(&widths, "┌", "┬", "┐")];
    for (index, row) in rows.iter().enumerate() {
        drawn.push(table_row(row, &widths, index == 0));
        if index == 0 {
            drawn.push(border(&widths, "├", "┼", "┤"));
        }
    }
    drawn.push(border(&widths, "└", "┴", "┘"));
    Layout { rows: drawn, caret: None }
}

fn divider(columns: &[String]) -> bool {
    !columns.is_empty()
        && columns
            .iter()
            .all(|column| !column.is_empty() && column.chars().all(|c| c == '-' || c == ':'))
}

fn border(widths: &[u16], left: &str, join: &str, right: &str) -> Row {
    let mut cells = vec![glyph(left, style::MARKER)];
    for (index, width) in widths.iter().enumerate() {
        cells.extend((0..width + 2).map(|_| glyph("─", style::MARKER)));
        cells.push(glyph(if index + 1 == widths.len() { right } else { join }, style::MARKER));
    }
    Row { cells, bits: 0, slots: Vec::new() }
}

fn table_row(row: &[String], widths: &[u16], head: bool) -> Row {
    let bits = if head { style::BOLD } else { 0 };
    let mut cells = vec![glyph("│", style::MARKER)];
    for (index, width) in widths.iter().enumerate() {
        let text = row.get(index).map(String::as_str).unwrap_or("");
        cells.push(glyph(" ", 0));
        cells.extend(text.graphemes(true).map(|part| glyph(part, bits)));
        cells.extend(padding(width.saturating_sub(text.width() as u16) + 1));
        cells.push(glyph("│", style::MARKER));
    }
    Row { cells, bits: 0, slots: Vec::new() }
}
