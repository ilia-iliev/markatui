//! Rows out of cells: how a line's worth of cells is broken to the width of the column,
//! where the caret can stand on each row that comes out, and where it goes for a given
//! cursor. Nothing here knows what a cell is of — only how wide it is and whether it came
//! from the source.

use super::{Cell, Row, Slot};

/// Break `body` into rows no wider than `width`, at a space where there is one. The first
/// row carries `lead`; the rest hang under it on `hanging`.
pub(super) fn wrap(lead: Vec<Cell>, hanging: Vec<Cell>, body: Vec<Cell>, width: u16, bits: u16) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut cells = lead;
    let mut room = width.saturating_sub(cells.iter().map(|cell| cell.width).sum());
    let mut rest = body.as_slice();

    loop {
        let fits = fitting(rest, room.max(1));
        cells.extend_from_slice(&rest[..fits]);
        rest = &rest[fits..];
        rows.push(finish(cells, bits));
        if rest.is_empty() {
            break;
        }
        // A row that breaks at a space leaves it behind rather than opening the next one.
        let dropped = rest.iter().take_while(|cell| cell.text == " ").count();
        rest = &rest[dropped..];
        if rest.is_empty() {
            break;
        }
        cells = hanging.clone();
        room = width.saturating_sub(cells.iter().map(|cell| cell.width).sum());
    }
    rows
}

/// How many cells of `rest` fit in `room`, breaking at the last space before the edge
/// where there is one and cutting a long word where there is not.
fn fitting(rest: &[Cell], room: u16) -> usize {
    let mut used = 0u16;
    let mut fits = 0;
    let mut last_space = None;
    for (index, cell) in rest.iter().enumerate() {
        if used + cell.width > room {
            break;
        }
        used += cell.width;
        fits = index + 1;
        if cell.text == " " {
            last_space = Some(index);
        }
    }
    if fits == rest.len() {
        return fits;
    }
    // The space the row broke at belongs to neither row; the caller drops it.
    match last_space {
        Some(index) if index > 0 => index,
        _ => fits.max(1),
    }
}

/// Close a row off: where the caret can stand on it, worked out from the cells that came
/// from somewhere.
fn finish(cells: Vec<Cell>, bits: u16) -> Row {
    let mut slots = Vec::new();
    let mut column = 0u16;
    let mut last = None;
    for cell in &cells {
        if let Some(source) = cell.source {
            slots.push(Slot { column, source });
            last = Some(source + cell.text.chars().count());
        }
        column += cell.width;
    }
    // One place past the end of the row, so a cursor at the end of a line has a column.
    if let Some(source) = last {
        slots.push(Slot { column, source });
    }
    Row { cells, bits, slots }
}

/// Where the caret goes for a cursor at `at`: the slot that stands exactly there, or the
/// first one past it for a cursor sitting where nothing is drawn.
pub(super) fn caret_at(rows: &[Row], at: usize) -> (usize, u16) {
    let mut fallback = (0, 0);
    for (index, row) in rows.iter().enumerate() {
        for slot in &row.slots {
            if slot.source == at {
                return (index, slot.column);
            }
            if slot.source < at {
                fallback = (index, slot.column + 1);
            }
        }
    }
    fallback
}
