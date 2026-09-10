//! The block being edited: its text, the cursor in it, and where a selection of its own
//! is pinned. Qt's `TextEdit` owned all of this and nothing in Rust did; here it is one
//! type with no terminal and no parser in it, so every key can be pressed in a test.
//!
//! Offsets are characters. The block's own source is the only text there is — there is no
//! rendered copy to keep in step — so every edit here is an edit of the markdown.

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
}

impl Active {
    pub fn new(text: &str, cursor: usize) -> Self {
        let mut active = Active { text: text.to_string(), cursor: 0, anchor: None, goal: None };
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
    }

    /// Take out the character the cursor is standing against, or the selection where
    /// there is one. `false` where there is nothing to take out on that side, which the
    /// document reads as a merge or a nothing.
    pub fn delete(&mut self, step: Step) -> bool {
        if let Some((start, end)) = self.selection() {
            self.replace(start, end, "");
            self.place(start);
            self.anchor = None;
            return true;
        }
        let (start, end) = if step > 0 {
            (self.cursor, self.neighbour(self.cursor, 1))
        } else {
            (self.neighbour(self.cursor, -1), self.cursor)
        };
        if start == end {
            return false;
        }
        self.replace(start, end, "");
        self.place(start);
        true
    }

    fn replace(&mut self, start: usize, end: usize, with: &str) {
        let range = byte_offset(&self.text, start)..byte_offset(&self.text, end);
        self.text.replace_range(range, with);
    }

    /// Where the block would break in two if the writer pressed Enter here: what is in
    /// front of the cursor and what is behind it, with the newline they typed a moment
    /// ago taken out from between them.
    pub fn split(&self) -> (String, String) {
        let before = self.cursor.saturating_sub(1);
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

    /// Put `marker` either side of the selection, or start an empty pair to type into.
    /// A second press takes it off again.
    pub fn surround(&mut self, marker: &str) {
        let (mut start, mut end) = self.selection().unwrap_or((self.cursor, self.cursor));
        let characters: Vec<char> = self.text.chars().collect();
        // Markdown wants the markers snug against the words: `**bold** `, never `**bold **`.
        while end > start && characters[end - 1].is_whitespace() {
            end -= 1;
        }
        while start < end && characters[start].is_whitespace() {
            start += 1;
        }
        // Stars work as bits: one is italic, two is bold, three is both. Only this
        // marker's own stars come off, so bold nests inside italic and a second press
        // unpicks it. Tildes have no such arithmetic — a pair is a strikeout — but the
        // same count answers.
        let mark = marker.chars().next().expect("a marker has a character");
        let run = marker_run(&characters, mark, start, -1);
        let width = length(marker);
        let marked = run == marker_run(&characters, mark, end, 1)
            && if width == 1 { run % 2 == 1 } else { run >= 2 };

        if marked {
            self.replace(end, end + width, "");
            self.replace(start - width, start, "");
            self.select(start - width, end - width);
        } else {
            self.replace(end, end, marker);
            self.replace(start, start, marker);
            self.select(start + width, end + width);
        }
    }

    /// `[selection](|)`, or `[|]()` with nothing selected. `prefix` is `!` for an image.
    pub fn insert_link(&mut self, prefix: &str) {
        let (start, end) = self.selection().unwrap_or((self.cursor, self.cursor));
        self.replace(end, end, "]()");
        self.replace(start, start, &format!("{prefix}["));
        self.anchor = None;
        // Inside the brackets with nothing selected, inside the parentheses with words
        // already there: either way, the cursor is where the writer still has to type.
        self.place(if start == end {
            start + length(prefix) + 1
        } else {
            end + length(prefix) + 3
        });
    }

    /// `![|](file)`: a picture already written to disk, over whatever was selected, with
    /// the cursor between the brackets where its description goes.
    pub fn insert_picture(&mut self, file: &str) {
        let (start, end) = self.selection().unwrap_or((self.cursor, self.cursor));
        self.replace(start, end, &format!("![]({file})"));
        self.anchor = None;
        self.place(start + 2);
    }

    /// Put a suggestion the checker offered where it objected, leaving the cursor at the
    /// end of it. One edit rather than a delete and an insert, so one undo takes it back.
    pub fn accept(&mut self, at: usize, len: usize, replacement: &str) {
        self.replace(at, at + len, replacement);
        self.anchor = None;
        self.place(at + length(replacement));
    }
}

/// How many `mark`s are packed against `at`, looking back (side -1) or on (side 1).
fn marker_run(characters: &[char], mark: char, at: usize, side: Step) -> usize {
    let mut run = 0;
    loop {
        let index = if side < 0 { at.checked_sub(run + 1) } else { Some(at + run) };
        match index.and_then(|index| characters.get(index)) {
            Some(character) if *character == mark => run += 1,
            _ => return run,
        }
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
