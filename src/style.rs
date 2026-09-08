//! What each character of a block is part of, worked out from the same markdown parser
//! that splits the document into blocks. Two questions are asked here and nowhere else:
//! what a character is part of inline — [`mask`] — and what at the head of a line is
//! structure rather than words — [`prefix`]. What either of them is drawn as is the
//! layout's business.

use crate::parse::{self, Kind};
use crate::text::{byte_offset, length};
use pulldown_cmark::{Event, Parser, Tag};
use std::ops::Range;

/// What a character is part of. One stretch of prose can be several of these at once — a
/// word inside a link inside a bold run — so they are bits.
pub const BOLD: u16 = 1;
pub const ITALIC: u16 = 2;
pub const CODE: u16 = 4;
pub const STRIKE: u16 = 8;
pub const LINK: u16 = 16;
/// A syntax marker with the cursor beside it: left legible, and muted.
pub const MARKER: u16 = 32;
/// A syntax marker away from the cursor: not drawn at all.
pub const HIDDEN: u16 = 64;
/// A heading's words, whatever else they are.
pub const HEADING: u16 = 128;
/// Not prose — code, a link, an address. The checker's marks keep off it.
pub const UNCHECKED: u16 = 16384;
/// Something the checker took exception to. Added by the editor, not by the parser.
pub const LINT: u16 = 32768;

/// Where a block's cursor is, in characters, or nowhere for a block that does not hold
/// it. Every entry point that cares about the cursor takes one of these.
pub type Cursor = Option<usize>;

/// What delimits a span, and so what of it is a marker rather than the thing itself.
#[derive(Clone, Copy)]
enum Delimiters {
    /// Nothing. A paragraph is made of its contents and no more.
    None,
    /// A marker either side: emphasis, a link. These go away unless the cursor is inside
    /// the span they belong to.
    Around,
}

struct Frame {
    range: Range<usize>,
    bits: u16,
    delimiters: Delimiters,
    /// Where this span's contents begin and end, as its children report themselves. What
    /// is left over at either edge is the marker.
    content: Option<Range<usize>>,
}

/// One set of style bits per character of `text`. `cursor` is where the cursor stands in
/// those same characters. Only inline markup is answered for here; the structure at the
/// head of a line is [`prefix`]'s.
pub fn mask(text: &str, cursor: Cursor) -> Vec<u16> {
    // Marked per byte, which is what the parser counts in, and counted out per character
    // at the end. Every range a parser hands back falls on a character boundary.
    let mut bytes = vec![0u16; text.len()];
    let cursor = cursor.map(|at| byte_offset(text, at));
    mark_prose(text, cursor, &mut bytes);
    text.char_indices().map(|(offset, _)| bytes[offset]).collect()
}

fn mark(bytes: &mut [u16], range: Range<usize>, bits: u16) {
    let end = range.end.min(bytes.len());
    for byte in &mut bytes[range.start.min(end)..end] {
        *byte |= bits;
    }
}

/// How a marker belonging to `span` is drawn: legible while the cursor is inside the
/// span, gone the moment it leaves.
fn marker_bits(cursor: Option<usize>, span: &Range<usize>) -> u16 {
    match cursor {
        Some(at) if span.contains(&at) || at == span.end => MARKER,
        _ => HIDDEN,
    }
}

fn mark_prose(text: &str, cursor: Option<usize>, bytes: &mut [u16]) {
    let mut stack: Vec<Frame> = Vec::new();

    for (event, range) in Parser::new_ext(text, parse::options()).into_offset_iter() {
        // A span closing is that span reported a second time, and is nobody's child.
        if matches!(event, Event::End(_)) {
            let Some(frame) = stack.pop() else { continue };
            mark(bytes, frame.range.clone(), inherited(&stack) | frame.bits);
            mark_delimiters(&frame, cursor, bytes);
            continue;
        }
        note_child(&mut stack, &range);

        match event {
            Event::Start(tag) => {
                let (bits, delimiters) = shape(&tag);
                stack.push(Frame { range, bits, delimiters, content: None });
            }
            // Inline code comes whole, contents and backticks together, so its markers
            // are the runs of backticks at either end.
            Event::Code(_) => {
                mark(bytes, range.clone(), inherited(&stack) | CODE | UNCHECKED);
                let source = text[range.clone()].as_bytes();
                let open = source.iter().take_while(|byte| **byte == b'`').count();
                let close = source.iter().rev().take_while(|byte| **byte == b'`').count();
                if open + close < source.len() {
                    let bits = marker_bits(cursor, &range);
                    mark(bytes, range.start..range.start + open, bits);
                    mark(bytes, range.end - close..range.end, bits);
                }
            }
            _ => mark(bytes, range, inherited(&stack)),
        }
    }
}

/// Take `range` into the contents of the span it sits inside, so that what is left of
/// that span at either edge is its markers.
fn note_child(stack: &mut [Frame], range: &Range<usize>) {
    let Some(frame) = stack.last_mut() else { return };
    match &mut frame.content {
        Some(content) => {
            content.start = content.start.min(range.start);
            content.end = content.end.max(range.end);
        }
        content => *content = Some(range.clone()),
    }
}

fn mark_delimiters(frame: &Frame, cursor: Option<usize>, bytes: &mut [u16]) {
    let Some(content) = &frame.content else { return };
    match frame.delimiters {
        Delimiters::None => {}
        Delimiters::Around => {
            let bits = marker_bits(cursor, &frame.range);
            mark(bytes, frame.range.start..content.start, bits);
            mark(bytes, content.end..frame.range.end, bits);
        }
    }
}

fn inherited(stack: &[Frame]) -> u16 {
    stack.iter().fold(0, |bits, frame| bits | frame.bits)
}

fn shape(tag: &Tag) -> (u16, Delimiters) {
    match tag {
        Tag::Strong => (BOLD, Delimiters::Around),
        Tag::Emphasis => (ITALIC, Delimiters::Around),
        Tag::Strikethrough => (STRIKE, Delimiters::Around),
        // A link is an address as much as it is words: nothing in it is spelled wrong.
        Tag::Link { .. } | Tag::Image { .. } => (LINK | UNCHECKED, Delimiters::Around),
        // A heading's words are bold and coloured wherever they sit in the line; the
        // hashes in front of them are the line's structure, and [`prefix`] has them.
        Tag::Heading { .. } => (HEADING, Delimiters::None),
        _ => (0, Delimiters::None),
    }
}

/// What stands at the head of a line before its words begin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Marker {
    None,
    Heading(usize),
    Bullet,
    /// A numbered item, as the writer wrote the number: `1.`, `27)`.
    Numbered(String),
    /// A fence line, which is all marker and has no words after it.
    Fence,
    /// A rule or a setext underline: likewise all marker.
    Whole,
}

/// The structure at the head of one source line: how much of it is not words, how deep in
/// quotes it sits, how far its own list indents it, and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefix {
    /// Characters at the head of the line that are structure rather than words.
    pub len: usize,
    /// How many block quotes the line is inside.
    pub quote: usize,
    /// The list nesting the line's words hang at, in source characters of indent.
    pub indent: usize,
    pub marker: Marker,
}

impl Prefix {
    fn empty() -> Self {
        Prefix { len: 0, quote: 0, indent: 0, marker: Marker::None }
    }
}

/// What of `line` is structure, given the kind of block it belongs to and whether it is
/// the first or last line of it — which is what tells a fence from three backticks a
/// writer happened to type.
pub fn prefix(line: &str, kind: Kind, edge: bool) -> Prefix {
    match kind {
        Kind::Code => fence(line, edge),
        Kind::Rule => Prefix { len: length(line), quote: 0, indent: 0, marker: Marker::Whole },
        Kind::Heading | Kind::List | Kind::Quote | Kind::Paragraph => within(line, kind),
        Kind::Table | Kind::Image => Prefix::empty(),
    }
}

fn fence(line: &str, edge: bool) -> Prefix {
    let opens = line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~");
    if edge && opens {
        Prefix { len: length(line), quote: 0, indent: 0, marker: Marker::Fence }
    } else {
        Prefix::empty()
    }
}

/// A line inside a block that can carry quotes, headings and list items — which, once the
/// quote markers are off, is any of them.
fn within(line: &str, kind: Kind) -> Prefix {
    let (quote, after) = quotes(line);
    let taken = length(line) - length(after);
    let indent = after.len() - after.trim_start().len();
    let body = after.trim_start();

    let marker = if kind == Kind::Heading && setext(body) {
        return Prefix { len: length(line), quote, indent, marker: Marker::Whole };
    } else if let Some(level) = hashes(body) {
        Marker::Heading(level)
    } else if body.starts_with("- ") || body.starts_with("* ") || body.starts_with("+ ") {
        Marker::Bullet
    } else if let Some(number) = numbered(body) {
        Marker::Numbered(number)
    } else {
        return Prefix { len: taken, quote, indent, marker: Marker::None };
    };

    let marker_len = match &marker {
        Marker::Heading(level) => level + 1,
        Marker::Bullet => 2,
        Marker::Numbered(number) => length(number) + 1,
        _ => 0,
    };
    Prefix { len: taken + indent + marker_len, quote, indent, marker }
}

/// How deep the line's quote markers run, and what is left of the line beneath them.
fn quotes(line: &str) -> (usize, &str) {
    let mut depth = 0;
    let mut rest = line;
    loop {
        let trimmed = rest.trim_start_matches(' ');
        let Some(inner) = trimmed.strip_prefix('>') else { return (depth, rest) };
        depth += 1;
        // A quote marker takes the one space after it; anything more is indent.
        rest = inner.strip_prefix(' ').unwrap_or(inner);
    }
}

/// The level of an ATX heading, where the line is one. The space after the hashes is
/// what makes it a heading rather than a word that starts with one.
fn hashes(body: &str) -> Option<usize> {
    let level = body.chars().take_while(|character| *character == '#').count();
    let rest = &body[level..];
    (1..=6).contains(&level).then_some(level).filter(|_| rest.starts_with(' '))
}

/// The number of an ordered list item, as it was written.
fn numbered(body: &str) -> Option<String> {
    let digits: String = body.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() > 9 {
        return None;
    }
    let rest = &body[digits.len()..];
    let mark = rest.chars().next().filter(|c| *c == '.' || *c == ')')?;
    rest[1..].starts_with(' ').then(|| format!("{digits}{mark}"))
}

/// Whether the line is a setext underline: the `=====` or `-----` that makes the line
/// above it a heading.
fn setext(body: &str) -> bool {
    !body.is_empty()
        && (body.chars().all(|c| c == '=') || body.chars().all(|c| c == '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mask as one letter per character: what the view would draw there.
    fn picture(text: &str, cursor: i32) -> String {
        drawing(&mask(text, usize::try_from(cursor).ok()))
    }

    fn drawing(bits: &[u16]) -> String {
        bits.iter()
            .map(|bits| match bits {
                _ if bits & HIDDEN != 0 => 'h',
                _ if bits & MARKER != 0 => 'm',
                _ if bits & CODE != 0 => 'c',
                _ if bits & LINK != 0 => 'l',
                _ if bits & (BOLD | ITALIC) == BOLD | ITALIC => '3',
                _ if bits & BOLD != 0 => 'b',
                _ if bits & ITALIC != 0 => 'i',
                _ if bits & STRIKE != 0 => 's',
                _ if bits & HEADING != 0 => 'H',
                _ => '.',
            })
            .collect()
    }

    #[test]
    fn shrinks_the_markers_away_and_brings_them_back_under_the_cursor() {
        assert_eq!(picture("**bold**", -1), "hhbbbbhh");
        assert_eq!(picture("**bold**", 3), "mmbbbbmm");
        assert_eq!(picture("a ~~gone~~ b", -1), "..hhsssshh..");
    }

    #[test]
    fn nests_emphasis_the_way_markdown_does() {
        assert_eq!(picture("**a *b* c**", -1), "hhbbh3hbbhh");
    }

    #[test]
    fn leaves_an_underscore_inside_a_word_alone() {
        assert_eq!(picture("snake_case_name", -1), ".".repeat(15));
    }

    #[test]
    fn marks_code_and_keeps_the_checker_off_it() {
        assert_eq!(picture("`x y`", -1), "hccch");
        assert_eq!(picture("`x y`", 2), "mcccm");
        assert!(mask("`x`", None).iter().all(|bits| bits & UNCHECKED != 0));
    }

    #[test]
    fn marks_a_link_whole_and_keeps_the_checker_off_it() {
        assert_eq!(picture("[a](b)", -1), "hlhhhh");
        assert_eq!(picture("[a](b)", 1), "mlmmmm");
        assert!(mask("[a](b)", None).iter().all(|bits| bits & UNCHECKED != 0));
    }

    #[test]
    fn counts_positions_in_characters() {
        // The emoji is one character, so the mask has one entry for it.
        assert_eq!(picture("🙂 **b**", -1), "..hhbhh");
        assert_eq!(picture("🙂 **b**", 3), "..mmbmm");
    }

    #[test]
    /// A heading's line is bold and coloured whole. Which of it is hashes rather than
    /// words is [`prefix`]'s answer, not the mask's.
    fn marks_a_heading_line_whole() {
        assert_eq!(picture("## Title", -1), "HHHHHHHH");
    }

    /// What the stepping and backspace rules rest on: wherever the cursor stands, the
    /// characters either side of it are on screen. A span shows its markers as soon as
    /// the cursor reaches it, either end included, so there is never an invisible
    /// character next to the cursor to be stepped over or deleted unseen.
    #[test]
    fn never_hides_a_marker_beside_the_cursor() {
        for text in [
            "**bold** and *thin*",
            "a `code` b",
            "go [home](https://example.com) now",
            "**a *b* c** d",
            "~~gone~~",
        ] {
            for cursor in 0..=text.chars().count() {
                let bits = mask(text, Some(cursor));
                for beside in [cursor.checked_sub(1), Some(cursor).filter(|at| *at < bits.len())] {
                    let Some(beside) = beside else { continue };
                    assert_eq!(
                        bits[beside] & HIDDEN,
                        0,
                        "{text:?} hides character {beside} with the cursor at {cursor}"
                    );
                }
            }
        }
    }

    #[test]
    fn takes_the_hashes_off_a_heading() {
        assert_eq!(
            prefix("## Title", Kind::Heading, true),
            Prefix { len: 3, quote: 0, indent: 0, marker: Marker::Heading(2) }
        );
        // Six is the deepest heading there is; a seventh hash is just a word.
        assert_eq!(prefix("####### x", Kind::Heading, true).marker, Marker::None);
        // And the space is what makes it one at all.
        assert_eq!(prefix("#hashtag", Kind::Paragraph, true).marker, Marker::None);
    }

    #[test]
    fn takes_the_bullet_off_a_list_item_and_keeps_its_indent() {
        assert_eq!(
            prefix("  - nested", Kind::List, false),
            Prefix { len: 4, quote: 0, indent: 2, marker: Marker::Bullet }
        );
        assert_eq!(
            prefix("12. item", Kind::List, false),
            Prefix { len: 4, quote: 0, indent: 0, marker: Marker::Numbered("12.".into()) }
        );
    }

    #[test]
    fn counts_the_quote_markers_and_looks_under_them() {
        assert_eq!(
            prefix("> > quoted", Kind::Quote, false),
            Prefix { len: 4, quote: 2, indent: 0, marker: Marker::None }
        );
        assert_eq!(prefix("> - item", Kind::Quote, false).marker, Marker::Bullet);
    }

    #[test]
    fn takes_a_fence_only_at_the_ends_of_its_block() {
        assert_eq!(prefix("```rust", Kind::Code, true).marker, Marker::Fence);
        assert_eq!(prefix("```", Kind::Code, false).marker, Marker::None);
        assert_eq!(prefix("    x = 1", Kind::Code, false), Prefix::empty());
    }

    #[test]
    fn takes_a_rule_and_a_setext_underline_whole() {
        assert_eq!(prefix("---", Kind::Rule, true).marker, Marker::Whole);
        assert_eq!(prefix("=====", Kind::Heading, false).marker, Marker::Whole);
    }
}
