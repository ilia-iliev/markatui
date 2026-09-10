//! A block turned into rows of styled cells at a given width. One entry point for every
//! block, whether it holds the cursor or not: the cursor changes only which markers are
//! revealed and where the caret goes.
//!
//! A cell is one grapheme cluster wide as the terminal measures it, so an emoji takes two
//! columns. Cells remember where in the block's source they came from, and some of them
//! came from nowhere at all — a bullet, a quote bar, a rule.

mod table;
mod wrap;

use crate::parse::{self, Kind};
use crate::style::{self, Cursor, Marker, Prefix};
use crate::text::length;
use std::ops::Range;
use table::table;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use wrap::{caret_at, wrap};

/// The glyph a hidden list bullet is drawn as, and the bar that stands in for a quote's
/// angle bracket. Both are the width of the markdown they replace, so the words do not
/// shift sideways as the cursor comes and goes.
const BULLET: &str = "•";
const QUOTE_BAR: &str = "▎";
/// What stands where a picture will go once there is one to draw.
const IMAGE: &str = "▣";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// One grapheme cluster, or a glyph the layout put there itself.
    pub text: String,
    pub width: u16,
    pub bits: u16,
    /// Where in the block's source this stands, in characters. Cells the layout made up
    /// came from nowhere and cannot be typed over.
    pub source: Option<usize>,
}

/// A place the caret can stand on a row: a column, and the source offset it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub column: u16,
    pub source: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// Bits the whole row carries, whatever its cells say: a code block's ground.
    pub bits: u16,
    /// Every column of this row the caret can stand at, in order, with the last standing
    /// past the final character. An empty line has that one place and nothing else. A row
    /// drawn rather than written — a rule, a fence, a table — has none at all.
    pub slots: Vec<Slot>,
}

impl Row {
    /// The source offset the caret means at `column`, or the nearest one this row has.
    pub fn source_at(&self, column: u16) -> Option<usize> {
        let nearest = self.slots.iter().min_by_key(|slot| slot.column.abs_diff(column))?;
        Some(nearest.source)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub rows: Vec<Row>,
    /// Where the caret is drawn, as a row and a column. Only a block holding the cursor
    /// has one.
    pub caret: Option<(usize, u16)>,
}

/// What to lay out: a block's source, where its cursor is if it has one, how wide the
/// column is, and what the checker took exception to inside it.
pub struct Request<'a> {
    pub text: &'a str,
    pub cursor: Cursor,
    /// Whether markdown around the cursor is exposed for editing.
    pub reveal: bool,
    pub width: u16,
    /// Spans of the block the checker objected to, in characters.
    pub lints: &'a [Range<usize>],
    /// How many rows the picture of a lone image takes, where there is one to draw. The
    /// terminal decides that, so the caller is asked rather than the layout deciding.
    pub picture: Option<u16>,
}

pub fn block(request: Request) -> Layout {
    let Request { text, cursor, reveal, width, lints, picture } = request;
    let kind = parse::kind(text);
    // A table and an image have no cursor mapping worth having: the columns of one and
    // the picture of the other stand where no character does. Under the cursor they open
    // up into their markdown unless reading mode has asked for rendered text throughout.
    let rendered = cursor.is_none() || !reveal;
    if kind == Kind::Table && rendered {
        return table(text);
    }
    if kind == Kind::Image && rendered {
        return image(text, picture);
    }

    let visible_cursor = reveal.then_some(cursor).flatten();
    let mask = style::mask(text, visible_cursor);
    let mut rows = Vec::new();
    for line in lines(text) {
        rows.extend(row_for(&line, kind, &mask, visible_cursor, width, lints));
    }
    if rows.is_empty() {
        rows.push(Row::default());
    }
    let caret = cursor.map(|at| caret_at(&rows, at));
    Layout { rows, caret }
}

/// One source line: where it starts in the block, in characters, and its text.
struct Line {
    at: usize,
    text: String,
    /// Whether it is the first or last line of the block, which is what tells a fence
    /// from three backticks a writer happened to type.
    edge: bool,
}

fn lines(text: &str) -> Vec<Line> {
    let split: Vec<&str> = text.split('\n').collect();
    let last = split.len() - 1;
    let mut at = 0;
    split
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let line = Line { at, text: line.to_string(), edge: index == 0 || index == last };
            at += line.text.chars().count() + 1;
            line
        })
        .collect()
}

/// The rows one source line takes: its structure drawn or revealed, its words wrapped to
/// the width left over.
fn row_for(
    line: &Line,
    kind: Kind,
    mask: &[u16],
    cursor: Cursor,
    width: u16,
    lints: &[Range<usize>],
) -> Vec<Row> {
    let prefix = style::prefix(&line.text, kind, line.edge);
    // The reveal rule, one line at a time: the structure of the line the cursor is on is
    // shown as the writer typed it, and every other line is drawn as it reads.
    let revealed = cursor.is_some_and(|at| at >= line.at && at <= line.at + length(&line.text));
    let (lead, hanging) = decoration(&prefix, line, revealed);

    if matches!(prefix.marker, Marker::Fence | Marker::Whole) && !revealed {
        return vec![Row { cells: lead, bits: block_bits(kind), slots: Vec::new() }];
    }

    let body = content(line, &prefix, mask, lints);
    wrap(lead, hanging, body, line.at + prefix.len, width, block_bits(kind))
}

/// Bits every row of a block carries: a fenced block is drawn on its own ground from top
/// to bottom, fences and all.
fn block_bits(kind: Kind) -> u16 {
    if kind == Kind::Code { style::CODE } else { 0 }
}

/// What stands in front of a line's words: the markdown itself where the cursor is on the
/// line, and what it reads as everywhere else. The second list is what a wrapped row gets
/// instead, so that a list item's second line hangs under its first.
fn decoration(prefix: &Prefix, line: &Line, revealed: bool) -> (Vec<Cell>, Vec<Cell>) {
    if revealed {
        // Shown as it was typed, muted, so that what is being edited is the source.
        let cells = source_cells(line, 0..prefix.len, style::MARKER, &vec![0; prefix.len], &[]);
        let hanging = padding(cells.iter().map(|cell| cell.width).sum());
        return (cells, hanging);
    }

    let mut lead = Vec::new();
    for _ in 0..prefix.quote {
        lead.push(glyph(QUOTE_BAR, style::MARKER));
        lead.push(glyph(" ", 0));
    }
    let bars = lead.clone();
    lead.extend(padding(prefix.indent as u16));

    match &prefix.marker {
        // The hashes go; what is left is a heading's words, which the mask has marked.
        Marker::Heading(_) => {}
        Marker::Bullet => {
            lead.push(glyph(BULLET, style::MARKER));
            lead.push(glyph(" ", 0));
        }
        // A number is how the line reads, not punctuation: it stays as it was written.
        Marker::Numbered(number) => {
            lead.extend(number.graphemes(true).map(|part| glyph(part, style::MARKER)));
            lead.push(glyph(" ", 0));
        }
        Marker::Fence => lead.extend(fence_border(line)),
        Marker::Whole => lead.extend(rule()),
        Marker::None => {}
    }

    let mut hanging = bars;
    hanging.extend(padding(prefix.indent as u16 + marker_width(&prefix.marker)));
    (lead, hanging)
}

fn marker_width(marker: &Marker) -> u16 {
    match marker {
        Marker::Bullet => 2,
        Marker::Numbered(number) => number.width() as u16 + 1,
        _ => 0,
    }
}

/// A fence drawn rather than shown: a line of rule with the language written into it, so
/// that a block of code says what it is without the backticks.
fn fence_border(line: &Line) -> Vec<Cell> {
    let language = line.text.trim_start().trim_start_matches(['`', '~']).trim();
    if language.is_empty() {
        return Vec::new();
    }
    language.graphemes(true).map(|part| glyph(part, style::MARKER)).collect()
}

/// A rule, drawn as the one character the row will be filled out with.
fn rule() -> Vec<Cell> {
    vec![Cell { text: "─".into(), width: 1, bits: style::MARKER | FILL, source: None }]
}

/// A cell the layout made up: a bullet, a bar, a space, a piece of rule.
fn glyph(text: &str, bits: u16) -> Cell {
    Cell { text: text.to_string(), width: text.width().max(1) as u16, bits, source: None }
}

fn padding(width: u16) -> Vec<Cell> {
    (0..width).map(|_| glyph(" ", 0)).collect()
}

/// A cell repeated to fill the rest of the column: a rule, and the border of a fenced
/// block. Kept out of [`style`] because it says how to draw, not what a character is —
/// and kept clear of every bit [`style`] uses, which the test below stands guard over.
pub const FILL: u16 = 1 << 13;

/// The words of a line, as cells, with what is in front of them left off unless the
/// cursor is on the line.
fn content(line: &Line, prefix: &Prefix, mask: &[u16], lints: &[Range<usize>]) -> Vec<Cell> {
    source_cells(line, prefix.len..length(&line.text), 0, mask, lints)
        .into_iter()
        // A marker inside the words is only drawn where the mask says it is on screen,
        // and a hard break's two trailing spaces are never worth a column.
        .filter(|cell| cell.bits & style::HIDDEN == 0)
        .collect()
}

/// Characters `range` of a line, as cells, carrying the bits the mask gave them and any
/// wash the checker asked for. `extra` is added to every one of them.
fn source_cells(
    line: &Line,
    range: Range<usize>,
    extra: u16,
    mask: &[u16],
    lints: &[Range<usize>],
) -> Vec<Cell> {
    let taken: String = line.text.chars().skip(range.start).take(range.len()).collect();
    let mut at = line.at + range.start;
    taken
        .graphemes(true)
        .map(|part| {
            let source = at;
            at += part.chars().count();
            let washed = lints.iter().any(|span| span.contains(&source));
            Cell {
                text: part.to_string(),
                width: part.width().max(1) as u16,
                bits: mask.get(source).copied().unwrap_or(0)
                    | extra
                    | if washed { style::LINT } else { 0 },
                source: Some(source),
            }
        })
        .collect()
}

/// A lone image: rows left empty for the terminal to draw the picture into, where there
/// is one; and until there is — while the file is being read, or for good where there is
/// no file to read — what it is of and where it is kept, in one muted line.
fn image(text: &str, picture: Option<u16>) -> Layout {
    if let Some(rows) = picture {
        return Layout { rows: vec![Row::default(); rows as usize], caret: None };
    }
    let path = parse::lone_image(text).unwrap_or_default();
    let alt: String =
        text.trim().trim_start_matches("![").split(']').next().unwrap_or_default().to_string();
    let line = format!("{IMAGE} {alt}  {path}");
    let cells = line.graphemes(true).map(|part| glyph(part, style::MARKER)).collect();
    Layout { rows: vec![Row { cells, bits: 0, slots: Vec::new() }], caret: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laid_out(text: &str, cursor: Cursor, width: u16) -> Layout {
        block(Request { text, cursor, reveal: true, width, lints: &[], picture: None })
    }

    /// What the rows read as, one string per row, with the caret drawn in.
    fn drawn(layout: &Layout) -> Vec<String> {
        layout
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let mut text = String::new();
                let mut column = 0;
                for cell in &row.cells {
                    if layout.caret == Some((index, column)) {
                        text.push('|');
                    }
                    text.push_str(&cell.text);
                    column += cell.width;
                }
                if layout.caret == Some((index, column)) {
                    text.push('|');
                }
                text
            })
            .collect()
    }

    #[test]
    fn draws_a_paragraph_and_hides_the_markers_away_from_the_cursor() {
        assert_eq!(laid_out("a **bold** b", None, 40).rows.len(), 1);
        assert_eq!(drawn(&laid_out("a **bold** b", None, 40)), ["a bold b"]);
        assert_eq!(drawn(&laid_out("a **bold** b", Some(4), 40)), ["a **|bold** b"]);
    }

    #[test]
    fn takes_the_hashes_off_a_heading_until_the_cursor_is_in_it() {
        assert_eq!(drawn(&laid_out("## Title", None, 40)), ["Title"]);
        assert_eq!(drawn(&laid_out("## Title", Some(4), 40)), ["## T|itle"]);
        let bits = laid_out("## Title", None, 40).rows[0].cells[0].bits;
        assert!(bits & style::HEADING != 0);
    }

    #[test]
    fn keeps_markdown_rendered_when_reveal_is_off() {
        let layout = block(Request {
            text: "## A **bold** heading",
            cursor: Some(8),
            reveal: false,
            width: 30,
            lints: &[],
            picture: None,
        });
        assert!(!drawn(&layout)[0].contains('#'));
        assert!(!drawn(&layout)[0].contains('*'));
    }

    #[test]
    fn draws_a_bullet_where_the_markdown_was() {
        assert_eq!(drawn(&laid_out("- one\n- two", None, 40)), ["• one", "• two"]);
        // The cursor's own line opens up; the rest stay as they read.
        assert_eq!(drawn(&laid_out("- one\n- two", Some(8), 40)), ["• one", "- |two"]);
    }

    #[test]
    fn indents_a_nested_item_under_the_one_it_hangs_from() {
        assert_eq!(drawn(&laid_out("- one\n  - deep", None, 40)), ["• one", "  • deep"]);
    }

    #[test]
    fn keeps_the_number_of_a_numbered_item() {
        assert_eq!(drawn(&laid_out("1. one\n2. two", None, 40)), ["1. one", "2. two"]);
    }

    #[test]
    fn puts_a_bar_in_the_gutter_of_a_quote() {
        assert_eq!(drawn(&laid_out("> one\n> two", None, 40)), ["▎ one", "▎ two"]);
    }

    #[test]
    fn draws_a_rule_as_a_rule() {
        let rows = laid_out("---", None, 40).rows;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cells[0].text, "─");
        assert!(rows[0].cells[0].bits & FILL != 0);
    }

    #[test]
    fn takes_the_fences_off_a_code_block_and_keeps_its_language() {
        let layout = laid_out("```rust\nlet x = 1;\n```", None, 40);
        assert_eq!(drawn(&layout), ["rust", "let x = 1;", ""]);
        assert!(layout.rows.iter().all(|row| row.bits & style::CODE != 0));
        // The cursor on a fence line shows it as it was typed.
        assert_eq!(drawn(&laid_out("```rust\nlet x = 1;\n```", Some(0), 40))[0], "|```rust");
    }

    #[test]
    fn wraps_at_a_space_and_hangs_the_rest_under_the_bullet() {
        assert_eq!(
            drawn(&laid_out("- one two three four", None, 10)),
            ["• one two", "  three", "  four"]
        );
    }

    #[test]
    fn cuts_a_word_too_long_for_the_column_rather_than_losing_it() {
        assert_eq!(drawn(&laid_out("abcdefghij", None, 4)), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn gives_a_wide_character_two_columns() {
        let row = &laid_out("🙂a", None, 40).rows[0];
        assert_eq!(row.cells[0].width, 2);
        assert_eq!(
            row.slots,
            [
                Slot { column: 0, source: 0 },
                Slot { column: 2, source: 1 },
                Slot { column: 3, source: 2 }
            ]
        );
    }

    #[test]
    fn puts_the_caret_where_the_cursor_is_even_at_the_end_of_a_line() {
        assert_eq!(drawn(&laid_out("one\ntwo", Some(3), 40)), ["one|", "two"]);
        assert_eq!(drawn(&laid_out("one\ntwo", Some(4), 40)), ["one", "|two"]);
        assert_eq!(drawn(&laid_out("one\ntwo", Some(7), 40)), ["one", "two|"]);
    }

    /// Enter opens a line under the one being typed, and the caret goes down to it there
    /// and then — it does not wait out in the column past the words above until something
    /// is typed on the new line.
    #[test]
    fn puts_the_caret_on_the_line_enter_has_just_opened() {
        assert_eq!(drawn(&laid_out("one\n", Some(4), 40)), ["one", "|"]);
        assert_eq!(laid_out("one\n", Some(4), 40).caret, Some((1, 0)));
    }

    /// And a line left blank between two that are written on can be stood on, which is
    /// what walking down through a fenced block over one asks for.
    #[test]
    fn gives_a_blank_line_between_two_written_ones_a_place_to_stand() {
        assert_eq!(drawn(&laid_out("one\n\ntwo", Some(4), 40)), ["one", "|", "two"]);
        assert_eq!(laid_out("one\n\ntwo", Some(4), 40).rows[1].source_at(0), Some(4));
    }

    #[test]
    fn takes_a_column_back_from_a_row_to_the_source_it_came_from() {
        let layout = laid_out("one two three", None, 8);
        assert_eq!(drawn(&layout), ["one two", "three"]);
        assert_eq!(layout.rows[1].source_at(2), Some(10));
    }

    #[test]
    fn draws_a_table_as_a_table_and_as_markdown_under_the_cursor() {
        let source = "| a | bb |\n| - | -- |\n| 1 | 2 |";
        assert_eq!(
            drawn(&laid_out(source, None, 40)),
            ["┌───┬────┐", "│ a │ bb │", "├───┼────┤", "│ 1 │ 2  │", "└───┴────┘"]
        );
        assert_eq!(drawn(&laid_out(source, Some(0), 40))[0], "|| a | bb |");
    }

    /// The fill bit rides alongside the style bits on the same number, so it must not be
    /// one of them: inline code carries `UNCHECKED`, and a collision would have every
    /// backticked word reach across the column.
    #[test]
    fn keeps_the_fill_bit_clear_of_every_style_bit() {
        let used = style::BOLD
            | style::ITALIC
            | style::CODE
            | style::STRIKE
            | style::LINK
            | style::MARKER
            | style::HIDDEN
            | style::HEADING
            | style::UNDERLINE
            | style::UNCHECKED
            | style::LINT;
        assert_eq!(FILL & used, 0);
    }

    #[test]
    fn draws_inline_code_as_words_rather_than_as_a_rule() {
        assert_eq!(drawn(&laid_out("a `x` b", None, 20)), ["a x b"]);
    }

    #[test]
    fn says_what_an_image_is_of_and_where_it_is_kept() {
        assert_eq!(drawn(&laid_out("![A picture](pic.png)", None, 40)), ["▣ A picture  pic.png"]);
        // Under the cursor it is markdown like anything else.
        assert_eq!(
            drawn(&laid_out("![A picture](pic.png)", Some(0), 40)),
            ["|![A picture](pic.png)"]
        );
    }

    /// A picture the terminal is going to draw leaves the rows empty and stands out of
    /// the way: what goes in them is not made of characters.
    #[test]
    fn leaves_room_for_a_picture_where_there_is_one_to_draw() {
        let layout = block(Request {
            text: "![A picture](pic.png)",
            cursor: None,
            reveal: true,
            width: 40,
            lints: &[],
            picture: Some(6),
        });
        assert_eq!(layout.rows.len(), 6);
        assert!(layout.rows.iter().all(|row| row.cells.is_empty()));
    }

    #[test]
    fn washes_what_the_checker_objected_to() {
        let marks = vec![2..5, 7..7];
        let layout = block(Request {
            text: "a bad b",
            cursor: None,
            reveal: true,
            width: 40,
            lints: &marks,
            picture: None,
        });
        let bits: Vec<bool> =
            layout.rows[0].cells.iter().map(|cell| cell.bits & style::LINT != 0).collect();
        assert_eq!(bits, [false, false, true, true, true, false, false]);
    }

    #[test]
    fn gives_an_empty_block_a_row_to_put_the_caret_on() {
        let layout = laid_out("", Some(0), 40);
        assert_eq!(layout.rows.len(), 1);
        assert_eq!(layout.caret, Some((0, 0)));
    }
}
