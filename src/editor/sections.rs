//! Moving a section of the document up or down: which lines travel together, which may
//! travel at all, and what moves when nothing inside the block can.
//!
//! One key for the two things a writer means by it. Inside a list, a quote, a table or a
//! fence the lines are things in their own right, and the key moves the one the cursor is
//! on — a list item with whatever is nested under it. Everywhere else, and at the ends of
//! those, the whole block moves. So a press always moves something, and the thing it
//! moves is the one the writer is looking at.

use crate::active::{Active, Step};
use crate::editor::Editor;
use crate::editor::blocks;
use crate::marks;
use crate::parse::{self, Kind};
use crate::style;

use std::ops::Range;
use std::sync::Arc;

impl Editor {
    /// Move what the writer is standing in one place up or down. A selection that has
    /// left the block takes every block it touches along with it.
    pub fn move_section(&mut self, step: Step) {
        if let Some(span) = self.selection().filter(|span| span.first != span.last) {
            self.move_blocks(span.first, span.last, step);
            return;
        }
        if !self.move_lines(step) {
            self.move_blocks(self.index, self.index, step);
        }
    }

    /// The line the cursor is on, or the lines a selection covers, one place along inside
    /// the block. `false` where they have nowhere to go, which is the block's own turn to
    /// move.
    fn move_lines(&mut self, step: Step) -> bool {
        let (first, last) = self.active.touched_lines();
        let Some(moved) = moved(self.active.text(), self.kind(), first, last, step) else {
            return false;
        };
        let selected = self.active.selection().is_some();
        let (line, column) = self.active.position();
        self.anchor = None;
        self.active = Active::new(&moved.text, 0);
        match selected {
            // The lines that moved stay selected, so that the next press moves them on.
            true => {
                let (from, to) = (
                    self.active.offset(moved.first, 0),
                    self.active.offset(moved.last, usize::MAX),
                );
                self.active.select(from, to);
            }
            false => {
                let at = self.active.offset(line.saturating_add_signed(moved.shift), column);
                self.active.place(at);
            }
        }
        self.record_edit();
        true
    }

    /// The blocks `first..=last` one place along, with the cursor and the selection kept
    /// on them. At the ends of the document there is nowhere to go, and the key does
    /// nothing rather than something else.
    fn move_blocks(&mut self, first: usize, last: usize, step: Step) {
        self.store_active();
        let landing = match step < 0 {
            true => first.checked_sub(1),
            false => (last + 1 < self.blocks.len()).then_some(last + 1),
        };
        let Some(landing) = landing else { return };
        match step < 0 {
            true => self.blocks[landing..=last].rotate_left(1),
            false => self.blocks[first..=landing].rotate_right(1),
        }
        // The gaps stay where they are: the blank lines between the blocks belong to the
        // places rather than to what is standing in them. What is standing on either side
        // of them has changed, though, and a seam that no longer holds is opened.
        for seam in first.max(1)..=(landing + 1).min(self.blocks.len() - 1) {
            self.keep_apart(seam);
        }
        let shift = step as isize;
        self.index = self.index.saturating_add_signed(shift);
        if let Some((block, _)) = &mut self.anchor {
            *block = block.saturating_add_signed(shift);
        }
        let (cursor, anchor) = (self.active.cursor(), self.active.anchor());
        self.active = Active::new(&self.blocks[self.index], cursor);
        if let Some(at) = anchor {
            self.active.pin(at);
        }
        self.record_edit();
    }

    /// Shift with Backspace and Delete: take out what the key above would move — the list
    /// item with whatever is nested under it, the table row, the line of code, or the
    /// whole block. Backspace leaves the caret at the end of what was before it, Delete
    /// at the head of what was after.
    pub fn delete_section(&mut self, step: Step) {
        if let Some(span) = self.selection().filter(|span| span.first != span.last) {
            self.delete_blocks(span.first, span.last, step);
            return;
        }
        if !self.delete_lines(step) {
            self.delete_blocks(self.index, self.index, step);
        }
    }

    /// The run of lines the cursor or the selection touches, taken out of the block.
    /// `false` where that run is the whole block, or no section of it.
    fn delete_lines(&mut self, step: Step) -> bool {
        let (first, last) = self.active.touched_lines();
        let Some((text, first)) = deleted(self.active.text(), self.kind(), first, last) else {
            return false;
        };
        self.anchor = None;
        self.active = Active::new(&text, 0);
        let at = match first.checked_sub(1).filter(|_| step < 0) {
            Some(line) => self.active.offset(line, usize::MAX),
            None => self.active.offset(first, 0),
        };
        self.active.place(at);
        self.record_edit();
        true
    }

    /// Take out the blocks `first..=last`. The document always keeps one block to type
    /// into, and the source after the last block — the file's own ending — outlives it.
    fn delete_blocks(&mut self, first: usize, last: usize, step: Step) {
        self.store_active();
        self.anchor = None;
        let mut first = first;
        if first == 0 && last + 1 == self.blocks.len() {
            self.blocks[0] = Arc::default();
            first = 1;
        }
        if last + 1 == self.blocks.len() {
            self.gaps[first] = self.gaps[last + 1].clone();
        }
        self.remove_blocks(first, last + 1 - first);
        if first > 0 && first < self.blocks.len() {
            self.keep_apart(first);
        }
        let (index, at) = match first.checked_sub(1) {
            Some(before) if step < 0 || first == self.blocks.len() => (before, usize::MAX),
            _ => (first, 0),
        };
        self.index = index;
        self.active = Active::new(&self.blocks[index], at);
        self.record_edit();
    }

    /// Open the gap before block `at` where what is written there no longer keeps the two
    /// blocks either side of it apart. Markdown has pairs that need nothing between them
    /// one way round and a blank line the other — a list under a paragraph reads as a
    /// list, a paragraph under a list reads as more of the list — so a move that puts a
    /// new pair either side of a gap can be a move that joins them. The blank line is the
    /// least that can be written there, and it is written only at the seam that moved.
    fn keep_apart(&mut self, at: usize) {
        let (before, after) = (&self.blocks[at - 1], &self.blocks[at]);
        let joined = format!("{before}{}{after}", self.gaps[at]);
        if parse::segments(&joined).blocks.first() != Some(&**before) {
            self.gaps[at] = Arc::new(blocks::PARAGRAPH.to_string());
        }
    }
}

/// A run of lines that has moved: the block it left behind, the lines the run now covers,
/// and how far each of them travelled.
struct Moved {
    text: String,
    first: usize,
    last: usize,
    shift: isize,
}

/// The block with the lines `first..=last` moved one place in `step`'s direction. `None`
/// where there is nowhere inside the block for them to go: the head of a list, a row
/// against the one that says how a table's columns read, a line against a fence.
fn moved(text: &str, kind: Kind, first: usize, last: usize, step: Step) -> Option<Moved> {
    let block: Vec<&str> = text.split('\n').collect();
    let within = movable(&block, kind);
    let nests = nests(kind);
    let (first, last) = section(&block, kind, first, last)?;

    let mut lines = block.clone();
    if step < 0 {
        if first == within.start {
            return None;
        }
        let landing = match nests {
            true => head(&block, first - 1, &within),
            false => first - 1,
        };
        lines[landing..=last].rotate_left(first - landing);
        let shift = landing as isize - first as isize;
        return Some(Moved {
            text: rewritten(&lines, &block),
            first: landing,
            last: last.saturating_add_signed(shift),
            shift,
        });
    }
    if last + 1 >= within.end {
        return None;
    }
    let far = match nests {
        true => tail(&block, last + 1, &within),
        false => last + 1,
    };
    lines[first..=far].rotate_right(far - last);
    let shift = (far - last) as isize;
    Some(Moved {
        text: rewritten(&lines, &block),
        first: first.saturating_add_signed(shift),
        last: far,
        shift,
    })
}

/// The lines `first..=last` widened to the whole runs they touch. `None` where they reach
/// into the block's furniture, which is no section of it.
fn section(block: &[&str], kind: Kind, first: usize, last: usize) -> Option<(usize, usize)> {
    let within = movable(block, kind);
    let (first, last) = match nests(kind) {
        true => (head(block, first, &within), tail(block, head(block, last, &within), &within)),
        false => (first, last),
    };
    (first >= within.start && last < within.end && first <= last).then_some((first, last))
}

/// The block with the run touching lines `first..=last` taken out, and the line the run
/// started on. `None` where the run is the whole block, or no section of it.
fn deleted(text: &str, kind: Kind, first: usize, last: usize) -> Option<(String, usize)> {
    let block: Vec<&str> = text.split('\n').collect();
    let (first, last) = section(&block, kind, first, last)?;
    if first == 0 && last + 1 == block.len() {
        return None;
    }
    let mut lines = block.clone();
    lines.drain(first..=last);
    Some((rewritten(&lines, &block), first))
}

/// The block as it reads once the run has moved. The numbers of an ordered list stay with
/// the places rather than with the items, so a list still counts down the page.
fn rewritten(lines: &[&str], block: &[&str]) -> String {
    let numbered: Vec<String> =
        lines.iter().zip(block).map(|(line, was)| marks::renumbered(line, was)).collect();
    numbered.join("\n")
}

/// Which lines of the block may be reordered at all: a table's rows below the one that
/// says how its columns read, the lines inside a fence rather than the fences themselves,
/// and every line of anything else. What is left out is a block's furniture — moving it
/// would not move a section, it would take the block apart.
fn movable(block: &[&str], kind: Kind) -> Range<usize> {
    match kind {
        Kind::Table => match block.iter().position(|line| marks::divider(line)) {
            Some(divider) => divider + 1..block.len(),
            None => 0..block.len(),
        },
        Kind::Code => {
            let opened = usize::from(style::fences(block[0]));
            let closed = usize::from(block.len() > 1 && style::fences(block[block.len() - 1]));
            opened..block.len() - closed
        }
        _ => 0..block.len(),
    }
}

/// Whether the lines of this block travel as runs — an item with whatever is nested under
/// it — rather than one at a time.
fn nests(kind: Kind) -> bool {
    matches!(kind, Kind::List | Kind::Quote)
}

/// Where the run holding line `at` begins: the item at or above it, since a line that
/// carries on from an item belongs to it.
fn head(block: &[&str], at: usize, within: &Range<usize>) -> usize {
    (within.start..=at).rev().find(|line| marks::item(block[*line]).is_some()).unwrap_or(at)
}

/// Where the run opened at `head` ends: everything nested under it, and any line carrying
/// on from it, up to the next item standing level with it.
fn tail(block: &[&str], head: usize, within: &Range<usize>) -> usize {
    let depth = indent(block[head]);
    let mut end = head;
    while end + 1 < within.end && !level_with(block[end + 1], depth) {
        end += 1;
    }
    end
}

/// Whether the line opens an item of its own no deeper than `depth`, which is where the
/// run above it stops.
fn level_with(line: &str, depth: usize) -> bool {
    marks::item(line).is_some() && indent(line) <= depth
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

#[cfg(test)]
mod tests {
    use crate::editor::tests::{document, texts};

    /// The item the cursor is in walks its list, and the cursor walks with it.
    #[test]
    fn moves_a_list_item_among_its_neighbours() {
        let mut editor = document("- one\n- two\n- three");
        editor.activate(0, 8);
        editor.move_section(-1);
        assert_eq!(texts(&editor), ["- two\n- one\n- three"]);
        assert_eq!(editor.active().cursor(), 2);
        editor.move_section(1);
        assert_eq!(texts(&editor), ["- one\n- two\n- three"]);
        assert_eq!(editor.active().cursor(), 8);
    }

    /// An item takes what is nested under it, and steps over the whole of what it meets.
    #[test]
    fn carries_the_lines_nested_under_an_item() {
        let mut editor = document("- one\n  - one a\n- two");
        editor.activate(0, 2);
        editor.move_section(1);
        assert_eq!(texts(&editor), ["- two\n- one\n  - one a"]);
        assert_eq!(editor.active().cursor(), 8);
    }

    /// A child moved up passes the item it belongs to, which is what a writer promoting
    /// an item means by the key.
    #[test]
    fn moves_a_nested_item_past_the_one_it_sits_under() {
        let mut editor = document("- one\n  - one a\n  - one b");
        editor.activate(0, 8);
        editor.move_section(-1);
        assert_eq!(texts(&editor), ["  - one a\n- one\n  - one b"]);
    }

    /// The numbers count down the page whatever order the items end up in, and a list
    /// written all ones is left as the writer wrote it.
    #[test]
    fn leaves_a_numbered_list_counting_down_the_page() {
        let mut editor = document("1. one\n2. two\n3. three");
        editor.activate(0, 20);
        editor.move_section(-1);
        assert_eq!(texts(&editor), ["1. one\n2. three\n3. two"]);

        let mut ones = document("1. one\n1. two");
        ones.activate(0, 8);
        ones.move_section(-1);
        assert_eq!(texts(&ones), ["1. two\n1. one"]);
    }

    /// A quote goes on by the line, so its lines move one at a time.
    #[test]
    fn moves_a_line_of_a_quote() {
        let mut editor = document("> first\n> second");
        editor.activate(0, 10);
        editor.move_section(-1);
        assert_eq!(texts(&editor), ["> second\n> first"]);
    }

    /// The whole block moves where the item has nowhere left to go inside it: otherwise
    /// the list at the top of a document could never be moved at all.
    #[test]
    fn moves_the_block_from_the_ends_of_a_list() {
        let mut editor = document("intro\n\n- one\n- two");
        editor.activate(1, 2);
        editor.move_section(-1);
        assert_eq!(texts(&editor), ["- one\n- two", "intro"]);
        assert_eq!(editor.index(), 0);
        assert_eq!(editor.active().cursor(), 2);
        assert_eq!(editor.source(), "- one\n- two\n\nintro");
    }

    /// A paragraph is one thing, however many lines the writer broke it over.
    #[test]
    fn moves_a_paragraph_whole() {
        let mut editor = document("one\n\ntwo\n\nthree");
        editor.activate(1, 1);
        editor.move_section(1);
        assert_eq!(texts(&editor), ["one", "three", "two"]);
        assert_eq!(editor.index(), 2);
        assert_eq!(editor.active().cursor(), 1);
        editor.move_section(1);
        assert_eq!(texts(&editor), ["one", "three", "two"]);
    }

    /// A table's rows move among themselves. The headings and the row of dashes under
    /// them are the table's frame, and a key that moved those would break it.
    #[test]
    fn moves_a_table_row_and_leaves_its_headings_alone() {
        let table = "| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |";
        let mut editor = document(table);
        editor.activate(0, 36);
        editor.move_section(-1);
        assert_eq!(texts(&editor), ["| a | b |\n| --- | --- |\n| 3 | 4 |\n| 1 | 2 |"]);
        // The first row has nowhere above it to go, so the table itself would move.
        editor.activate(0, 26);
        editor.move_section(-1);
        assert_eq!(texts(&editor), ["| a | b |\n| --- | --- |\n| 3 | 4 |\n| 1 | 2 |"]);
    }

    /// A line of code moves inside the fence, and the fences stay where they are.
    #[test]
    fn moves_a_line_of_code_between_the_fences() {
        let mut editor = document("```\nfirst\nsecond\n```");
        editor.activate(0, 6);
        editor.move_section(1);
        assert_eq!(texts(&editor), ["```\nsecond\nfirst\n```"]);
        assert_eq!(editor.active().cursor(), 13);
        // Past the last line of code there is nothing but the fence: the block moves.
        editor.move_section(1);
        assert_eq!(texts(&editor), ["```\nsecond\nfirst\n```"]);
    }

    /// The lines a selection covers move together and stay selected, so that holding the
    /// key walks them up the document.
    #[test]
    fn moves_the_lines_a_selection_covers() {
        let mut editor = document("- one\n- two\n- three");
        editor.activate(0, 6);
        editor.place_cursor(18, true);
        editor.move_section(-1);
        assert_eq!(texts(&editor), ["- two\n- three\n- one"]);
        assert_eq!(editor.selected_text(), "- two\n- three");
    }

    /// A selection that has left the block takes every block it touches along.
    #[test]
    fn moves_every_block_a_selection_touches() {
        let mut editor = document("one\n\ntwo\n\nthree\n\nfour");
        editor.activate(1, 0);
        editor.extend_to(2, 5);
        editor.move_section(1);
        assert_eq!(texts(&editor), ["one", "four", "two", "three"]);
        assert_eq!(editor.selected_text(), "two\n\nthree");
    }

    /// A move that would join two blocks opens the seam between them instead: a list
    /// under a paragraph needs no blank line, and a paragraph under a list does.
    #[test]
    fn opens_a_seam_the_move_would_close() {
        let mut editor = document("text\n- a\n- b");
        editor.activate(0, 0);
        editor.move_section(1);
        assert_eq!(editor.source(), "- a\n- b\n\ntext");
    }

    /// A seam that still holds is left as the writer wrote it: a heading needs no blank
    /// line above it either way round.
    #[test]
    fn leaves_a_seam_that_holds_alone() {
        let mut editor = document("# Title\ntext");
        editor.activate(1, 0);
        editor.move_section(-1);
        assert_eq!(editor.source(), "text\n# Title");
    }

    /// The key moves the section rather than the words in it, so one press is one undo.
    #[test]
    fn takes_one_undo_to_put_back() {
        let mut editor = document("one\n\ntwo");
        editor.activate(1, 0);
        editor.move_section(-1);
        assert_eq!(texts(&editor), ["two", "one"]);
        editor.undo();
        assert_eq!(texts(&editor), ["one", "two"]);
    }

    /// Shift+Backspace leaves the caret at the end of the item above, Shift+Delete at the
    /// head of the one below.
    #[test]
    fn deletes_the_list_item_the_cursor_is_in() {
        let mut editor = document("- one\n- two\n- three");
        editor.activate(0, 8);
        editor.delete_section(-1);
        assert_eq!(texts(&editor), ["- one\n- three"]);
        assert_eq!(editor.active().cursor(), 5);

        let mut editor = document("- one\n- two\n- three");
        editor.activate(0, 8);
        editor.delete_section(1);
        assert_eq!(texts(&editor), ["- one\n- three"]);
        assert_eq!(editor.active().cursor(), 6);
    }

    #[test]
    fn deletes_an_item_with_what_is_nested_under_it() {
        let mut editor = document("- one\n  - one a\n- two");
        editor.activate(0, 2);
        editor.delete_section(-1);
        assert_eq!(texts(&editor), ["- two"]);
        assert_eq!(editor.active().cursor(), 0);
    }

    #[test]
    fn leaves_a_numbered_list_counting_after_a_delete() {
        let mut editor = document("1. one\n2. two\n3. three");
        editor.activate(0, 10);
        editor.delete_section(-1);
        assert_eq!(texts(&editor), ["1. one\n2. three"]);
    }

    #[test]
    fn deletes_a_table_row_and_never_its_headings() {
        let mut editor = document("| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |");
        editor.activate(0, 26);
        editor.delete_section(-1);
        assert_eq!(texts(&editor), ["| a | b |\n| --- | --- |\n| 3 | 4 |"]);
    }

    #[test]
    fn deletes_a_line_of_code_between_the_fences() {
        let mut editor = document("```\nfirst\nsecond\n```");
        editor.activate(0, 6);
        editor.delete_section(1);
        assert_eq!(texts(&editor), ["```\nsecond\n```"]);
    }

    /// Where the lines are not things of their own, the block goes. Backspace lands at the
    /// end of the block before, Delete at the head of the block after.
    #[test]
    fn deletes_a_paragraph_whole() {
        let mut editor = document("one\n\ntwo\n\nthree");
        editor.activate(1, 1);
        editor.delete_section(-1);
        assert_eq!(texts(&editor), ["one", "three"]);
        assert_eq!((editor.index(), editor.active().cursor()), (0, 3));
        assert_eq!(editor.source(), "one\n\nthree");

        let mut editor = document("one\n\ntwo\n\nthree");
        editor.activate(1, 1);
        editor.delete_section(1);
        assert_eq!((editor.index(), editor.active().cursor()), (1, 0));
    }

    /// The last item of a list is the whole list.
    #[test]
    fn deletes_the_block_of_a_list_with_one_item() {
        let mut editor = document("intro\n\n- one");
        editor.activate(1, 2);
        editor.delete_section(-1);
        assert_eq!(texts(&editor), ["intro"]);
        assert_eq!(editor.source(), "intro");
    }

    #[test]
    fn keeps_the_end_of_the_file_when_the_last_block_goes() {
        let mut editor = document("one\n\ntwo\n");
        editor.activate(1, 0);
        editor.delete_section(-1);
        assert_eq!(editor.source(), "one\n");
    }

    #[test]
    fn leaves_an_empty_block_when_the_only_one_goes() {
        let mut editor = document("only\n");
        editor.delete_section(-1);
        assert_eq!(texts(&editor), [""]);
        assert_eq!(editor.source(), "\n");
    }

    /// A heading needs no blank line around it; the paragraphs either side of one do.
    #[test]
    fn opens_a_seam_a_delete_would_close() {
        let mut editor = document("para\n# Title\ntext");
        editor.activate(1, 0);
        editor.delete_section(-1);
        assert_eq!(texts(&editor), ["para", "text"]);
        assert_eq!(editor.source(), "para\n\ntext");
    }

    #[test]
    fn deletes_every_block_a_selection_touches() {
        let mut editor = document("one\n\ntwo\n\nthree\n\nfour");
        editor.activate(1, 1);
        editor.extend_to(2, 2);
        editor.delete_section(-1);
        assert_eq!(texts(&editor), ["one", "four"]);
    }

    #[test]
    fn takes_one_undo_to_put_a_deleted_section_back() {
        let mut editor = document("one\n\n- a\n  - b\n- c");
        editor.activate(1, 2);
        editor.delete_section(-1);
        assert_eq!(texts(&editor), ["one", "- c"]);
        editor.undo();
        assert_eq!(texts(&editor), ["one", "- a\n  - b\n- c"]);
        assert_eq!((editor.index(), editor.active().cursor()), (1, 2));

        editor.activate(0, 0);
        editor.delete_section(-1);
        editor.undo();
        assert_eq!(texts(&editor), ["one", "- a\n  - b\n- c"]);
    }
}
