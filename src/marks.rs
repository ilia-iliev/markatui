//! What stands at the head of a line — hashes, bullets, numbers, quote markers — put on
//! and taken off, and the row of dashes that says which way a table's columns read.
//!
//! Everything here works on lines of markdown and knows nothing about blocks, cursors or
//! terminals, so what a formatting key does to the source can be read in one place. What
//! a line already carries is [`style::prefix`]'s answer, asked here rather than worked
//! out a second time.

use crate::parse::Kind;
use crate::style::{self, Marker, Prefix};
use crate::text::{byte_offset, length};

/// What a formatting key puts at the head of the lines it is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// A heading of this many hashes. Zero is a plain paragraph, and takes off whatever
    /// was there.
    Heading(usize),
    Quote,
    Bullet,
    Numbered,
    Task,
}

/// Which way a table's column reads, as the row of dashes under the headings writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
    Right,
}

impl Align {
    /// The cell this alignment is written as. Three dashes is the shortest that is still
    /// a divider row on its own, so a column set back to plain reads the same as one
    /// that was never set at all.
    fn cell(self) -> &'static str {
        match self {
            Align::Left => ":---",
            Align::Centre => ":---:",
            Align::Right => "---:",
        }
    }
}

/// The lines with `mark` put on them, or taken off them where every one already carries
/// it — which is what makes one key both apply a style and undo it.
pub fn toggle(lines: &[&str], mark: Mark) -> Vec<String> {
    if lines.iter().all(|line| carries(line, mark)) {
        return lines.iter().map(|line| bare(line, mark)).collect();
    }
    lines.iter().enumerate().map(|(at, line)| marked(line, mark, at + 1)).collect()
}

/// Whether the line already reads as `mark`. A paragraph is what a line comes to rather
/// than something it carries, so nothing ever answers yes to it.
fn carries(line: &str, mark: Mark) -> bool {
    let prefix = lead(line);
    match mark {
        Mark::Heading(0) => false,
        Mark::Heading(level) => prefix.marker == Marker::Heading(level),
        Mark::Quote => prefix.quote > 0,
        Mark::Bullet => prefix.marker == Marker::Bullet && box_len(line, &prefix) == 0,
        Mark::Numbered => matches!(prefix.marker, Marker::Numbered(_)),
        Mark::Task => prefix.marker == Marker::Bullet && box_len(line, &prefix) > 0,
    }
}

/// The line with `mark` off it. Only the marker goes: the indent in front of it and any
/// quotes over it are the line's own.
fn bare(line: &str, mark: Mark) -> String {
    if mark == Mark::Quote {
        return unquoted(line);
    }
    let prefix = lead(line);
    format!("{}{}", head(line, &prefix), body(line, &prefix))
}

/// The line with `mark` on it, whatever it carried before. `number` is which item of the
/// run this is, which only an ordered list has any use for.
fn marked(line: &str, mark: Mark, number: usize) -> String {
    if mark == Mark::Quote {
        return quoted(line);
    }
    let prefix = lead(line);
    let marker = match mark {
        Mark::Heading(0) => String::new(),
        Mark::Heading(level) => format!("{} ", "#".repeat(level)),
        Mark::Bullet => "- ".to_string(),
        Mark::Numbered => format!("{number}. "),
        Mark::Task => "- [ ] ".to_string(),
        Mark::Quote => unreachable!("a quote is put on above"),
    };
    format!("{}{marker}{}", head(line, &prefix), body(line, &prefix))
}

/// What is in front of a line's words: its indent, the quotes over it, and the marker
/// under those. The kind is the one that lets every marker through — a line is being
/// asked about on its own here, not as part of a block that has already been named.
fn lead(line: &str) -> Prefix {
    style::prefix(line, Kind::Paragraph, false)
}

/// The indent and quote markers, which stay whatever the key does.
fn head<'a>(line: &'a str, prefix: &Prefix) -> &'a str {
    &line[..byte_offset(line, prefix.len - prefix.marker_len())]
}

/// The words, with the marker and any task box off them.
fn body<'a>(line: &'a str, prefix: &Prefix) -> &'a str {
    &line[byte_offset(line, prefix.len + box_len(line, prefix))..]
}

/// How much of the words is the `[ ]` a task list puts after its bullet.
fn box_len(line: &str, prefix: &Prefix) -> usize {
    if prefix.marker != Marker::Bullet {
        return 0;
    }
    let rest = &line[byte_offset(line, prefix.len)..];
    const BOXES: [&str; 3] = ["[ ] ", "[x] ", "[X] "];
    BOXES.iter().find(|ticked| rest.starts_with(**ticked)).map_or(0, |ticked| length(ticked))
}

/// The line one quote deeper, with the marker going in after the indent so that what was
/// indented under the quote stays indented under it.
fn quoted(line: &str) -> String {
    let at = line.len() - line.trim_start().len();
    format!("{}> {}", &line[..at], &line[at..])
}

/// The line one quote shallower. Only the outermost marker goes, so a quote inside a
/// quote takes two presses to unwind, the way it took two to build.
fn unquoted(line: &str) -> String {
    let at = line.len() - line.trim_start().len();
    let Some(rest) = line[at..].strip_prefix('>') else { return line.to_string() };
    format!("{}{}", &line[..at], rest.strip_prefix(' ').unwrap_or(rest))
}

/// The marker to carry onto the line Enter opens, for a line that is a list item.
pub struct Item {
    /// What goes at the head of the new line: the same indent and quotes, and the marker
    /// that follows this one.
    pub next: String,
    /// How much of this line the marker takes, in characters.
    pub len: usize,
    /// Whether the item has nothing in it, which is how a writer says the list is over.
    pub empty: bool,
}

/// The item `line` is, if it is one. A heading or a plain line is not: Enter in those
/// does what Enter has always done.
pub fn item(line: &str) -> Option<Item> {
    let prefix = lead(line);
    let ticked = box_len(line, &prefix);
    let marker = match &prefix.marker {
        Marker::Bullet if ticked > 0 => "- [ ] ".to_string(),
        Marker::Bullet => "- ".to_string(),
        Marker::Numbered(number) => next_number(number),
        _ => return None,
    };
    let len = prefix.len + ticked;
    Some(Item {
        next: format!("{}{marker}", head(line, &prefix)),
        len,
        empty: body(line, &prefix).trim().is_empty(),
    })
}

/// `3.` after `2.`, keeping whichever of `.` and `)` the writer was using. A number too
/// long to count with starts again at one rather than overflowing.
fn next_number(number: &str) -> String {
    let (digits, punctuation) = number.split_at(number.len() - 1);
    format!("{}{punctuation} ", digits.parse::<u32>().unwrap_or(0) + 1)
}

/// Whether the line is a table's row of dashes: the one row that says how the columns
/// read rather than what is in them.
pub fn divider(line: &str) -> bool {
    let cells = split_row(line);
    !cells.is_empty()
        && cells.iter().all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':'))
}

/// The cells of one table row, trimmed, with the pipes at either edge off.
pub fn split_row(line: &str) -> Vec<&str> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    line.trim_start_matches('|').trim_end_matches('|').split('|').map(str::trim).collect()
}

/// The table with the column the cursor is in set to read `align`, and where the cursor
/// stands in it afterwards. `None` where the block is not a table, or the cursor is in
/// no column of one — which is the key doing nothing rather than mangling a paragraph.
pub fn aligned(table: &str, cursor: usize, align: Align) -> Option<(String, usize)> {
    let column = column_at(table, cursor)?;
    let mut lines: Vec<String> = table.lines().map(str::to_string).collect();
    let at = lines.iter().position(|line| divider(line))?;

    let mut cells: Vec<String> =
        split_row(&lines[at]).iter().map(|cell| cell.to_string()).collect();
    *cells.get_mut(column)? = align.cell().to_string();
    let was = length(&lines[at]);
    lines[at] = format!("| {} |", cells.join(" | "));

    // The cursor only moves where the row that changed is above it: the rows are lines
    // of one block, and a line growing pushes everything after it along.
    let moved = length(&lines[at]) as isize - was as isize;
    let after = length(&table.lines().take(at + 1).collect::<Vec<_>>().join("\n"));
    let cursor = if cursor > after { (cursor as isize + moved).max(0) as usize } else { cursor };
    Some((lines.join("\n"), cursor))
}

/// Which column of the table the cursor stands in, counted by the pipes in front of it on
/// its own line.
fn column_at(table: &str, cursor: usize) -> Option<usize> {
    let mut seen = 0;
    for line in table.lines() {
        let end = seen + length(line);
        if cursor <= end {
            let before = &line[..byte_offset(line, cursor - seen)];
            let pipes = before.matches('|').count();
            return match line.trim_start().starts_with('|') {
                // Before the opening pipe is not yet in a column.
                true => pipes.checked_sub(1),
                false => Some(pipes),
            };
        }
        seen = end + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puts_a_marker_on_and_takes_it_off_again() {
        assert_eq!(toggle(&["one", "two"], Mark::Bullet), ["- one", "- two"]);
        assert_eq!(toggle(&["- one", "- two"], Mark::Bullet), ["one", "two"]);
        assert_eq!(toggle(&["one", "two"], Mark::Numbered), ["1. one", "2. two"]);
        assert_eq!(toggle(&["one"], Mark::Task), ["- [ ] one"]);
        assert_eq!(toggle(&["- [x] one"], Mark::Task), ["one"]);
    }

    /// A run where only some lines carry the marker gets it put on the rest rather than
    /// taken off the ones that have it: the key is being asked for a list, not a swap.
    #[test]
    fn puts_it_on_where_only_some_of_the_lines_have_it() {
        assert_eq!(toggle(&["- one", "two"], Mark::Bullet), ["- one", "- two"]);
    }

    #[test]
    fn swaps_one_kind_of_marker_for_another() {
        assert_eq!(toggle(&["- one"], Mark::Numbered), ["1. one"]);
        assert_eq!(toggle(&["3) one"], Mark::Bullet), ["- one"]);
        assert_eq!(toggle(&["## Title"], Mark::Bullet), ["- Title"]);
        assert_eq!(toggle(&["- one"], Mark::Heading(2)), ["## one"]);
    }

    #[test]
    fn keeps_the_indent_and_the_quotes_a_line_already_had() {
        assert_eq!(toggle(&["  - one"], Mark::Numbered), ["  1. one"]);
        assert_eq!(toggle(&["> - one"], Mark::Task), ["> - [ ] one"]);
    }

    #[test]
    fn counts_a_heading_by_its_hashes() {
        assert_eq!(toggle(&["one"], Mark::Heading(1)), ["# one"]);
        // The same key again takes it back to a paragraph.
        assert_eq!(toggle(&["# one"], Mark::Heading(1)), ["one"]);
        // A different level changes it rather than taking it off.
        assert_eq!(toggle(&["# one"], Mark::Heading(3)), ["### one"]);
        assert_eq!(toggle(&["### one"], Mark::Heading(0)), ["one"]);
    }

    #[test]
    fn takes_one_quote_off_at_a_time() {
        assert_eq!(toggle(&["one"], Mark::Quote), ["> one"]);
        assert_eq!(toggle(&["> > one"], Mark::Quote), ["> one"]);
        assert_eq!(toggle(&["  one"], Mark::Quote), ["  > one"]);
    }

    #[test]
    fn carries_a_list_marker_onto_the_next_line() {
        assert_eq!(item("- one").expect("an item").next, "- ");
        assert_eq!(item("2) one").expect("an item").next, "3) ");
        assert_eq!(item("  - [x] one").expect("an item").next, "  - [ ] ");
        assert_eq!(item("  - [x] one").expect("an item").len, 8);
        assert!(item("plain").is_none());
        assert!(item("## Heading").is_none());
    }

    #[test]
    fn knows_an_item_with_nothing_in_it() {
        assert!(item("- ").expect("an item").empty);
        assert!(!item("- one").expect("an item").empty);
        assert!(item("- [ ] ").expect("an item").empty);
    }

    #[test]
    fn sets_the_column_the_cursor_is_in() {
        let table = "| a | b |\n| --- | --- |\n| 1 | 2 |";
        let (aligned_table, _) = aligned(table, 7, Align::Right).expect("a table");
        assert_eq!(aligned_table.lines().nth(1), Some("| --- | ---: |"));
        let (aligned_table, _) = aligned(table, 2, Align::Centre).expect("a table");
        assert_eq!(aligned_table.lines().nth(1), Some("| :---: | --- |"));
    }

    /// The row of dashes grows, and the cursor below it comes along rather than being
    /// left pointing a cell to the left of where the writer put it.
    #[test]
    fn carries_the_cursor_past_the_row_that_grew() {
        let table = "| a | b |\n| - | - |\n| 1 | 2 |";
        let (_, cursor) = aligned(table, 22, Align::Centre).expect("a table");
        assert_eq!(cursor, 26);
        let (_, cursor) = aligned(table, 2, Align::Centre).expect("a table");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn leaves_a_block_that_is_not_a_table_alone() {
        assert!(aligned("just words", 3, Align::Left).is_none());
        assert!(aligned("| a | b |\n| 1 | 2 |", 3, Align::Left).is_none());
    }

    #[test]
    fn knows_the_row_of_dashes_from_the_rows_of_words() {
        assert!(divider("| --- | :-: |"));
        assert!(divider("|---|---|"));
        assert!(!divider("| a | b |"));
        assert!(!divider(""));
    }
}
