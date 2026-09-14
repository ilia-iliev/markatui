//! The markdown markers a key puts round the words, or at the head of the lines, of the
//! block being edited.
//!
//! There is no rendered copy to keep in step, so every one of these is an edit of the
//! source itself: a marker goes in as the characters it is written with, and comes off by
//! the same characters being taken out again. Which is also why each one asks whether the
//! marker is already there — the key that puts it on is the key that takes it off.

use crate::active::{Active, Step};
use crate::marks::{self, Mark};
use crate::text::length;

impl Active {
    /// Put `marker` either side of the selection, or start an empty pair to type into.
    /// A second press takes it off again.
    pub fn surround(&mut self, marker: &str) {
        let (start, end) = self.trimmed_selection();
        let characters: Vec<char> = self.text.chars().collect();
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

    /// Where the selection runs with the whitespace at its edges left out, or the bare
    /// cursor where there is none. Markdown wants its markers snug against the words:
    /// `**bold** `, never `**bold **`.
    fn trimmed_selection(&self) -> (usize, usize) {
        let (mut start, mut end) = self.selection().unwrap_or((self.cursor, self.cursor));
        let characters: Vec<char> = self.text.chars().collect();
        while end > start && characters[end - 1].is_whitespace() {
            end -= 1;
        }
        while start < end && characters[start].is_whitespace() {
            start += 1;
        }
        (start, end)
    }

    /// Put an HTML tag pair either side of the selection — the only way markdown has of
    /// saying underline. A second press takes it off again.
    pub fn wrap(&mut self, tag: &str) {
        let (start, end) = self.trimmed_selection();
        let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
        let (open_len, close_len) = (length(&open), length(&close));
        let wrapped = start >= open_len
            && self.slice(start - open_len, start) == open
            && end + close_len <= self.length()
            && self.slice(end, end + close_len) == close;

        if wrapped {
            self.replace(end, end + close_len, "");
            self.replace(start - open_len, start, "");
            self.select(start - open_len, end - open_len);
        } else {
            self.replace(end, end, &close);
            self.replace(start, start, &open);
            self.select(start + open_len, end + open_len);
        }
    }

    /// Put the whole block in a fenced code block, or take it out of one. The cursor
    /// lands at the end of the opening fence, which is where the language goes.
    pub fn fence(&mut self) {
        const FENCE: &str = "```";
        let lines: Vec<&str> = self.text.split('\n').collect();
        let fenced = lines.len() > 1
            && lines[0].trim_start().starts_with(FENCE)
            && lines[lines.len() - 1].trim().starts_with(FENCE);

        self.anchor = None;
        self.text = match fenced {
            true => lines[1..lines.len() - 1].join("\n"),
            false => format!("{FENCE}\n{}\n{FENCE}", self.text),
        };
        self.place(if fenced { 0 } else { FENCE.len() });
    }

    /// Put `mark` at the head of every line the cursor or the selection touches, or take
    /// it off them where they all carry it already. A selection stays over the lines it
    /// was on, so the same key pressed twice puts the block back.
    pub fn mark_lines(&mut self, mark: Mark) {
        self.rewrite_lines(|lines| marks::toggle(lines, mark));
    }

    /// Every line the cursor or the selection touches, one level of indent further in or
    /// back out. `false` where there was no indent left to give back.
    pub fn indent_lines(&mut self, step: Step) -> bool {
        self.rewrite_lines(|lines| marks::indented(lines, step))
    }

    /// Every line the cursor or the selection touches, rewritten by `change`. What is
    /// rewritten is the head of the line, so the cursor keeps its place among the words
    /// and a selection stays over the lines it was on. `false` where nothing changed.
    fn rewrite_lines(&mut self, change: impl Fn(&[&str]) -> Vec<String>) -> bool {
        let selected = self.selection().is_some();
        let (start, end) = self.touched_lines();
        let text = self.slice(start, end).to_string();
        let lines: Vec<&str> = text.split('\n').collect();
        let marked = change(&lines);
        if marked == lines {
            return false;
        }

        // The cursor keeps its place among the words: everything that changes is at the
        // head of the line, so the line's own growth is what the cursor moves by.
        let (line_start, _) = self.line_bounds(self.cursor);
        let index = self.slice(start, line_start).matches('\n').count();
        let grown = length(&marked[index]) as isize - length(lines[index]) as isize;
        let within = (self.cursor - line_start) as isize + grown;
        let before: usize = marked[..index].iter().map(|line| length(line) + 1).sum();
        let cursor = start + before + within.clamp(0, length(&marked[index]) as isize) as usize;

        let marked = marked.join("\n");
        let count = length(&marked);
        self.replace(start, end, &marked);
        match selected {
            true => self.select(start, start + count),
            false => {
                self.anchor = None;
                self.place(cursor);
            }
        }
        true
    }

    /// The whole of every line the cursor or the selection touches. Marking a line is a
    /// change to the line, not to the part of it that happens to be selected.
    fn touched_lines(&self) -> (usize, usize) {
        let (from, to) = self.selection().unwrap_or((self.cursor, self.cursor));
        (self.line_bounds(from).0, self.line_bounds(to).1)
    }

    /// The list item the cursor is standing in, if it is standing in one.
    pub fn item(&self) -> Option<marks::Item> {
        marks::item(self.line())
    }

    /// The whole of the line the cursor is standing on.
    pub fn line(&self) -> &str {
        let (start, end) = self.line_bounds(self.cursor);
        self.slice(start, end)
    }

    /// Move to the next cell of the table, or the one before it. `false` where there is
    /// none that way, which leaves the cursor where the writer put it.
    pub fn step_cell(&mut self, step: Step) -> bool {
        let Some(at) = marks::cell_step(&self.text, self.cursor, step) else { return false };
        self.anchor = None;
        self.place(at);
        true
    }

    /// End the list: the marker of the empty item the writer pressed Enter on comes off,
    /// leaving the line for whatever they write instead.
    pub fn end_item(&mut self, item: &marks::Item) {
        let (start, _) = self.line_bounds(self.cursor);
        self.replace(start, start + item.len, "");
        self.anchor = None;
        self.place(start);
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
