//! The block being edited: its text, the cursor in it, and where a selection of its own
//! is pinned. Qt's `TextEdit` owned all of this and nothing in Rust did; here it is one
//! type with no terminal and no parser in it, so every key can be pressed in a test.
//!
//! Offsets are characters. The block's own source is the only text there is — there is no
//! rendered copy to keep in step — so every edit here is an edit of the markdown.

mod markup;

use crate::marks;
use crate::text::{byte_offset, length};

/// Which way a movement goes. Left and up are -1, right and down 1.
pub type Step = i32;

pub struct Active {
    text: String,
    /// Where the cursor stands, in characters.
    cursor: usize,
    /// The other end of the block's own selection, or nowhere for a bare cursor.
    anchor: Option<usize>,
    /// The column a run of up-and-down movement is aiming for, so that passing through a
    /// short line does not drag the cursor in to its end for good.
    goal: Option<usize>,
    /// Whether deletion took the last content out of this block. An empty block that was
    /// opened by Enter is intentional; one emptied by Backspace or Delete can go.
    emptied: bool,
}

impl Active {
    pub fn new(text: &str, cursor: usize) -> Self {
        let mut active =
            Active { text: text.to_string(), cursor: 0, anchor: None, goal: None, emptied: false };
        active.place(cursor);
        active
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    pub fn length(&self) -> usize {
        length(&self.text)
    }

    pub fn emptied(&self) -> bool {
        self.emptied
    }

    /// Where the block's own selection runs, in characters, if one does. Empty where the
    /// anchor has caught up with the cursor, which is no selection at all.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        let (start, end) = (anchor.min(self.cursor), anchor.max(self.cursor));
        (start != end).then_some((start, end))
    }

    pub fn selected_text(&self) -> String {
        match self.selection() {
            Some((start, end)) => self.slice(start, end).to_string(),
            None => String::new(),
        }
    }

    fn slice(&self, start: usize, end: usize) -> &str {
        &self.text[byte_offset(&self.text, start)..byte_offset(&self.text, end)]
    }

    /// Put the cursor at `at`, or at the end of the block for a position past it. The
    /// caller may hand in a position from a block that has since been rewritten.
    pub fn place(&mut self, at: usize) {
        self.cursor = at.min(self.length());
        self.goal = None;
    }

    pub fn drop_selection(&mut self) {
        self.anchor = None;
    }

    /// Pin a selection where the cursor stands, unless one is already running.
    pub fn start_selection(&mut self) {
        self.anchor.get_or_insert(self.cursor);
    }

    /// Pin a selection at `at` without moving the cursor: a selection that left this
    /// block and came back is the block's own again, pinned where it always was.
    pub fn pin(&mut self, at: usize) {
        self.anchor = Some(at.min(self.length()));
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.place(self.length());
    }

    /// The word `at` stands in, as the two ends of it. A position out among the spaces
    /// takes the word that ended there; where none did — the head of a line, a run of
    /// punctuation — the two ends are the same, which is no selection at all.
    pub fn word(&self, at: usize) -> (usize, usize) {
        let characters: Vec<char> = self.text.chars().collect();
        let at = at.min(characters.len());
        let start = characters[..at]
            .iter()
            .rposition(|c| !c.is_alphanumeric())
            .map_or(0, |index| index + 1);
        let end = characters[at..]
            .iter()
            .position(|c| !c.is_alphanumeric())
            .map_or(characters.len(), |offset| at + offset);
        (start, end)
    }

    pub fn select(&mut self, from: usize, to: usize) {
        self.anchor = Some(from.min(self.length()));
        self.place(to);
    }

    // ---- moving ----------------------------------------------------------------

    /// Step one character.
    ///
    /// Nothing here has to walk over a marker that is not on screen, and nothing below
    /// has to reveal one before deleting it, because a marker beside the cursor is never
    /// hidden: the view shows a span's markers as soon as the cursor reaches the span,
    /// either end included. `style::never_hides_a_marker_beside_the_cursor` is that
    /// invariant written down, and the stepping and backspace rules rest on it.
    pub fn step(&mut self, step: Step) {
        self.cursor = self.neighbour(self.cursor, step);
        self.goal = None;
    }

    /// Where one step from `at` lands, stopping at the ends of the block.
    fn neighbour(&self, at: usize, step: Step) -> usize {
        if step > 0 { (at + 1).min(self.length()) } else { at.saturating_sub(1) }
    }

    /// The end of the block a movement in `step` runs into.
    fn edge(&self, step: Step) -> usize {
        if step > 0 { self.length() } else { 0 }
    }

    /// Step a word at a time: over the run of spaces, then over the word beyond it.
    pub fn step_word(&mut self, step: Step) {
        let characters: Vec<char> = self.text.chars().collect();
        let mut at = self.cursor;
        let peek = |at: usize| -> Option<char> {
            let index = if step > 0 { at } else { at.checked_sub(1)? };
            characters.get(index).copied()
        };
        while peek(at).is_some_and(|c| !c.is_alphanumeric()) {
            at = self.neighbour(at, step);
        }
        while peek(at).is_some_and(char::is_alphanumeric) {
            at = self.neighbour(at, step);
        }
        self.cursor = at;
        self.goal = None;
    }

    /// Move to the start or end of the line, barring the whitespace at that edge: Home
    /// lands on the first character of what is written rather than in the indentation in
    /// front of it, End just after the last rather than out in the trailing spaces. A
    /// line that is nothing but whitespace has no such place, and keeps its plain edges.
    pub fn to_line_edge(&mut self, step: Step) {
        let (start, end) = self.line_bounds(self.cursor);
        let characters: Vec<char> = self.text.chars().collect();
        let line = &characters[start..end];
        self.cursor = if step > 0 {
            line.iter().rposition(|c| !c.is_whitespace()).map_or(end, |at| start + at + 1)
        } else {
            line.iter().position(|c| !c.is_whitespace()).map_or(start, |at| start + at)
        };
        self.goal = None;
    }

    /// Put the cursor at the end of the table row it is on, past the row of dashes where
    /// that is what follows: the dashes belong to the heading above them, and a row opened
    /// between the two would leave the table without the line that says how its columns
    /// read.
    pub fn to_row_end(&mut self) {
        self.to_line_edge(1);
        let (_, end) = self.line_bounds(self.cursor);
        let (start, after) = self.line_bounds(end + 1);
        if marks::divider(self.slice(start, after)) {
            self.place(after);
        }
    }

    pub fn to_block_edge(&mut self, step: Step) {
        self.cursor = self.edge(step);
        self.goal = None;
    }

    /// Move to the line `step` away, keeping to the column the run of movement is aiming
    /// for. `false` where there is no such line, which is the caller's cue to leave the
    /// block — the rule the Qt editor had at the top and bottom of a block.
    pub fn step_line(&mut self, step: Step) -> bool {
        let (start, end) = self.line_bounds(self.cursor);
        let goal = *self.goal.get_or_insert(self.cursor - start);
        let landing = if step > 0 {
            if end >= self.length() {
                return false;
            }
            end + 1
        } else {
            if start == 0 {
                return false;
            }
            self.line_bounds(start - 1).0
        };
        let (_, landed_end) = self.line_bounds(landing);
        self.cursor = (landing + goal).min(landed_end);
        // Kept across the move, which is the whole point of it.
        self.goal = Some(goal);
        true
    }

    /// Which line of the block the cursor is on, and how far into it, both counted in
    /// characters.
    pub fn position(&self) -> (usize, usize) {
        let (start, _) = self.line_bounds(self.cursor);
        (self.slice(0, start).matches('\n').count(), self.cursor - start)
    }

    /// Where `column` characters into line `line` falls, or the end of that line where it
    /// is shorter than that. A line past the foot of the block is the end of the block.
    pub fn offset(&self, line: usize, column: usize) -> usize {
        let mut lines = self.text.split('\n');
        let before: usize = lines.by_ref().take(line).map(|line| length(line) + 1).sum();
        lines.next().map_or(self.length(), |text| before + column.min(length(text)))
    }

    /// Where the source line holding `at` begins and ends, in characters.
    fn line_bounds(&self, at: usize) -> (usize, usize) {
        let characters: Vec<char> = self.text.chars().collect();
        let start = characters[..at.min(characters.len())]
            .iter()
            .rposition(|c| *c == '\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let end = characters[at.min(characters.len())..]
            .iter()
            .position(|c| *c == '\n')
            .map(|offset| at + offset)
            .unwrap_or(characters.len());
        (start, end)
    }

    // ---- editing ---------------------------------------------------------------

    /// Put `insert` where the cursor is, taking out the selection first if there is one.
    pub fn insert(&mut self, insert: &str) {
        let (start, end) = self.selection().unwrap_or((self.cursor, self.cursor));
        self.replace(start, end, insert);
        self.place(start + length(insert));
        self.anchor = None;
        self.emptied = false;
    }

    /// Take out the character the cursor is standing against, or the selection where
    /// there is one. `false` where there is nothing to take out on that side, which the
    /// document reads as a merge or a nothing.
    pub fn delete(&mut self, step: Step) -> bool {
        let selection = self.selection();
        let (start, end) = selection.unwrap_or_else(|| {
            if step > 0 {
                (self.cursor, self.neighbour(self.cursor, 1))
            } else {
                (self.neighbour(self.cursor, -1), self.cursor)
            }
        });
        if start == end {
            return false;
        }

        let had_content = !self.text.trim().is_empty();
        self.replace(start, end, "");
        self.place(start);
        if selection.is_some() {
            self.anchor = None;
        }
        self.emptied |= had_content && self.text.trim().is_empty();
        true
    }

    fn replace(&mut self, start: usize, end: usize, with: &str) {
        let range = byte_offset(&self.text, start)..byte_offset(&self.text, end);
        self.text.replace_range(range, with);
    }

    /// Where the block would break in two if the writer pressed Enter here: what is in
    /// front of the cursor and what is behind it, with the newline they typed a moment
    /// ago taken out from between them. There is one to take out only where a first
    /// Enter left it: a heading breaks on the press that ends it.
    pub fn split(&self) -> (String, String) {
        let before = match self.character_before() {
            Some('\n') => self.cursor - 1,
            _ => self.cursor,
        };
        (self.slice(0, before).to_string(), self.slice(self.cursor, self.length()).to_string())
    }

    /// Whether an Enter here ends the block rather than adding a line to it — which is
    /// what a second Enter means, the first having left a newline behind the cursor.
    pub fn ends_block(&self) -> bool {
        self.cursor > 0 && self.character_before() == Some('\n')
    }

    fn character_before(&self) -> Option<char> {
        self.text[..byte_offset(&self.text, self.cursor)].chars().next_back()
    }

    /// Put a suggestion the checker offered where it objected, leaving the cursor at the
    /// end of it. One edit rather than a delete and an insert, so one undo takes it back.
    pub fn accept(&mut self, at: usize, len: usize, replacement: &str) {
        self.replace(at, at + len, replacement);
        self.anchor = None;
        self.place(at + length(replacement));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, cursor: usize) -> Active {
        Active::new(text, cursor)
    }

    /// The block with the cursor drawn in it, so a test reads as what is on screen.
    fn shown(active: &Active) -> String {
        let mut text = active.text.clone();
        text.insert(byte_offset(&active.text, active.cursor), '|');
        text
    }

    #[test]
    fn types_where_the_cursor_is() {
        let mut active = at("ab", 1);
        active.insert("X");
        assert_eq!(shown(&active), "aX|b");
    }

    #[test]
    fn types_over_a_selection() {
        let mut active = at("alpha", 0);
        active.select(1, 4);
        active.insert("X");
        assert_eq!(shown(&active), "aX|a");
    }

    #[test]
    fn deletes_either_side_of_the_cursor() {
        let mut active = at("abc", 2);
        assert!(active.delete(-1));
        assert_eq!(shown(&active), "a|c");
        assert!(active.delete(1));
        assert_eq!(shown(&active), "a|");
    }

    #[test]
    fn has_nothing_to_delete_at_the_ends_of_the_block() {
        assert!(!at("abc", 0).delete(-1));
        assert!(!at("abc", 3).delete(1));
    }

    #[test]
    fn counts_in_characters_not_bytes() {
        let mut active = at("🙂b", 1);
        assert!(active.delete(-1));
        assert_eq!(shown(&active), "|b");
    }

    /// Backspace against a marker only ever takes out something on screen, because the
    /// cursor standing beside a span is what shows that span's markers in the first place.
    #[test]
    fn deletes_a_marker_only_once_it_is_on_screen() {
        let mut active = at("**b** c", 5);
        assert!(active.delete(-1));
        assert_eq!(shown(&active), "**b*| c");
    }

    #[test]
    fn steps_a_word_at_a_time() {
        let mut active = at("one two three", 0);
        active.step_word(1);
        assert_eq!(shown(&active), "one| two three");
        active.step_word(1);
        assert_eq!(shown(&active), "one two| three");
        active.step_word(-1);
        assert_eq!(shown(&active), "one |two three");
    }

    #[test]
    fn walks_the_lines_of_a_block_keeping_its_column() {
        let mut active = at("long line\nx\nanother line", 6);
        assert!(active.step_line(1));
        // The short line has no sixth column, so the cursor rests at its end.
        assert_eq!(shown(&active), "long line\nx|\nanother line");
        assert!(active.step_line(1));
        // And the column it was aiming for is still what it lands on.
        assert_eq!(shown(&active), "long line\nx\nanothe|r line");
    }

    #[test]
    fn says_when_there_is_no_line_left_to_walk_to() {
        assert!(!at("one\ntwo", 1).step_line(-1));
        assert!(!at("one\ntwo", 5).step_line(1));
    }

    #[test]
    fn goes_to_the_ends_of_the_line_it_is_on() {
        let mut active = at("one\ntwo", 5);
        active.to_line_edge(-1);
        assert_eq!(active.cursor(), 4);
        active.to_line_edge(1);
        assert_eq!(active.cursor(), 7);
    }

    #[test]
    fn stops_at_the_writing_rather_than_the_whitespace_round_it() {
        //             0123456789
        let mut active = at(
            "  two  
x",
            7,
        );
        active.to_line_edge(-1);
        assert_eq!(active.cursor(), 2);
        active.to_line_edge(1);
        assert_eq!(active.cursor(), 5);
        // A line with nothing but whitespace on it keeps its plain edges.
        let mut blank = at("   ", 1);
        blank.to_line_edge(1);
        assert_eq!(blank.cursor(), 3);
        blank.to_line_edge(-1);
        assert_eq!(blank.cursor(), 0);
    }

    #[test]
    fn wraps_a_selection_in_a_marker_and_unwraps_it_again() {
        let mut active = at("a bold b", 2);
        active.select(2, 6);
        active.surround("**");
        assert_eq!(active.text(), "a **bold** b");
        assert_eq!(active.selected_text(), "bold");
        active.surround("**");
        assert_eq!(active.text(), "a bold b");
    }

    #[test]
    fn keeps_the_markers_snug_against_the_words() {
        let mut active = at("a bold b", 1);
        active.select(1, 7);
        active.surround("*");
        assert_eq!(active.text(), "a *bold* b");
    }

    #[test]
    fn starts_an_empty_pair_to_type_into() {
        let mut active = at("ab", 1);
        active.surround("**");
        assert_eq!(shown(&active), "a**|**b");
    }

    #[test]
    fn writes_a_link_around_the_selection_and_waits_in_the_brackets() {
        let mut active = at("go home now", 3);
        active.select(3, 7);
        active.insert_link("");
        assert_eq!(shown(&active), "go [home](|) now");

        let mut empty = at("go ", 3);
        empty.insert_link("!");
        assert_eq!(shown(&empty), "go ![|]()");
    }

    #[test]
    fn writes_a_pasted_picture_in_with_room_for_its_description() {
        let mut active = at("see ", 4);
        active.insert_picture("post-1.png");
        assert_eq!(shown(&active), "see ![|](post-1.png)");

        // A paste goes over the selection, the same as a paste of words does.
        let mut over = at("see this", 4);
        over.select(4, 8);
        over.insert_picture("post-1.png");
        assert_eq!(shown(&over), "see ![|](post-1.png)");
    }

    #[test]
    fn breaks_the_block_at_the_newline_the_writer_just_typed() {
        let active = at("one\n", 4);
        assert!(active.ends_block());
        assert_eq!(active.split(), ("one".to_string(), String::new()));
        assert!(!at("one", 3).ends_block());
    }

    #[test]
    fn puts_a_suggestion_where_the_checker_objected() {
        let mut active = at("I recieve mail.", 15);
        active.accept(2, 7, "receive");
        assert_eq!(shown(&active), "I receive| mail.");
    }
}
