//! The arithmetic of a document held as blocks: where a selection runs, what it reads,
//! what is left when it goes, and how the source comes back together.
//!
//! A block is one paragraph of the view. Between every pair of them sits a gap — the
//! source the view does not show, blank lines and all — kept so that saving gives back
//! the file that was opened, byte for byte. There is one more gap than there are blocks:
//! before the first, between each pair, and after the last.

use crate::parse;
use crate::text::byte_offset;
use std::sync::Arc;

/// Where a selection runs, in document order: a block and a byte offset into it at either
/// end. `first == last` for a selection inside one block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub first: usize,
    pub first_at: usize,
    pub last: usize,
    pub last_at: usize,
}

/// Where a selection pinned at (`anchor`, `anchor_at`) and reaching to (`cursor`,
/// `cursor_at`) runs, with the two ends put into document order. Positions arrive counted
/// in characters and leave counted in bytes. `None` where either end names a block that
/// is not there.
pub fn span(
    blocks: &[Arc<String>],
    anchor: usize,
    anchor_at: usize,
    cursor: usize,
    cursor_at: usize,
) -> Option<Span> {
    let (first, first_at, last, last_at) = if anchor <= cursor {
        (anchor, anchor_at, cursor, cursor_at)
    } else {
        (cursor, cursor_at, anchor, anchor_at)
    };
    let mut first_at = byte_offset(blocks.get(first)?, first_at);
    let mut last_at = byte_offset(blocks.get(last)?, last_at);
    // Within one block the cursor may be either side of where it was pinned.
    if first == last && first_at > last_at {
        std::mem::swap(&mut first_at, &mut last_at);
    }
    Some(Span { first, first_at, last, last_at })
}

/// What a selection reads, as it would be written to disk: the gaps between the blocks it
/// runs through are part of it.
pub fn selected_text(blocks: &[Arc<String>], gaps: &[Arc<String>], span: Span) -> String {
    if span.first == span.last {
        return blocks[span.first][span.first_at..span.last_at].to_string();
    }
    let mut selected = blocks[span.first][span.first_at..].to_string();
    for index in span.first + 1..=span.last {
        selected.push_str(&gaps[index]);
        if index == span.last {
            selected.push_str(&blocks[index][..span.last_at]);
        } else {
            selected.push_str(&blocks[index]);
        }
    }
    selected
}

/// The block left behind when a selection goes and `insert` takes its place: what was in
/// front of it joined to what was behind it, across however many blocks it ran through.
/// The cursor lands at the seam, counted in characters.
pub fn spliced(blocks: &[Arc<String>], span: Span, insert: &str) -> (String, usize) {
    let mut kept = blocks[span.first][..span.first_at].to_string();
    kept.push_str(insert);
    let cursor = kept.chars().count();
    kept.push_str(&blocks[span.last][span.last_at..]);
    (kept, cursor)
}

/// Blocks and the separators between them for source that already belongs to one block.
/// Whitespace around the source stays editable by folding it into the end blocks.
pub fn replacement(source: &str) -> (Vec<String>, Vec<String>) {
    let parse::Segments { mut blocks, gaps } = parse::segments(source);
    blocks[0].insert_str(0, &gaps[0]);
    let last = blocks.len() - 1;
    blocks[last].push_str(&gaps[last + 1]);
    let mut separators = gaps[1..last + 1].to_vec();
    open_paragraphs(&mut blocks, &mut separators);
    (blocks, separators)
}

/// What goes between two blocks broken apart: a blank line, which is what a paragraph
/// ends with wherever the writer has not said otherwise.
pub const PARAGRAPH: &str = "\n\n";

/// Every picture the writer left among the words of a paragraph broken out into a
/// paragraph of its own, blocks and the separators between them together. `false` where
/// nothing had to move.
pub fn hoist_images(blocks: &mut Vec<String>, separators: &mut Vec<String>) -> bool {
    let mut moved = false;
    // From the back, so that a block breaking into several leaves the ones still to be
    // looked at where they were.
    for index in (0..blocks.len()).rev() {
        let Some(pieces) = parse::hoisted_images(&blocks[index]) else { continue };
        separators.splice(index..index, vec![PARAGRAPH.to_string(); pieces.len() - 1]);
        blocks.splice(index..index + 1, pieces);
        moved = true;
    }
    moved
}

/// Every blank line a writer left over and above the one that ends a paragraph opened
/// back into the empty paragraph it was written as: the view stacks blocks with one row
/// of air between them, so a run of blank lines left in a gap is a run the writer never
/// sees again. The gap is only cut in more places, never rewritten — the document still
/// says exactly what the file says.
pub fn open_paragraphs(blocks: &mut Vec<String>, separators: &mut Vec<String>) {
    // From the back, so that a separator becoming several leaves the ones still to be
    // looked at where they were.
    for index in (0..separators.len()).rev() {
        let breaks = paragraph_breaks(&separators[index]);
        if breaks.len() < 2 {
            continue;
        }
        blocks.splice(index + 1..index + 1, vec![String::new(); breaks.len() - 1]);
        separators.splice(index..index + 1, breaks);
    }
}

/// The source between two blocks cut into one piece per paragraph break it holds — two
/// newlines each, which is what an empty paragraph is written as. Whatever is left over,
/// an odd newline or the spaces on a blank line, stays on the last piece, so that the
/// pieces put back together are the gap they came from.
fn paragraph_breaks(gap: &str) -> Vec<String> {
    let wanted = gap.bytes().filter(|byte| *byte == b'\n').count() / 2;
    let mut breaks = Vec::with_capacity(wanted);
    let mut newlines = 0;
    let mut start = 0;
    for (at, byte) in gap.bytes().enumerate() {
        if byte != b'\n' {
            continue;
        }
        newlines += 1;
        if newlines % 2 == 0 && breaks.len() + 1 < wanted {
            breaks.push(gap[start..=at].to_string());
            start = at + 1;
        }
    }
    breaks.push(gap[start..].to_string());
    breaks
}

/// The same over a whole document, whose gaps carry the source outside the blocks as well
/// as the source between them.
pub fn hoist_document(segments: &mut parse::Segments) -> bool {
    between(segments, hoist_images)
}

/// The same over a whole document.
pub fn open_paragraphs_document(segments: &mut parse::Segments) {
    between(segments, open_paragraphs);
}

/// A pass over a document's blocks and the separators between them. What is around the
/// outside is left alone: it is what keeps a file that was only read coming back the way
/// it was found.
fn between<T>(
    segments: &mut parse::Segments,
    pass: impl FnOnce(&mut Vec<String>, &mut Vec<String>) -> T,
) -> T {
    let last = segments.gaps.len() - 1;
    let mut separators: Vec<String> = segments.gaps.drain(1..last).collect();
    let outcome = pass(&mut segments.blocks, &mut separators);
    segments.gaps.splice(1..1, separators);
    outcome
}

/// The document as it would be written out: the blocks with their gaps back between them.
pub fn source(blocks: &[Arc<String>], gaps: &[Arc<String>]) -> String {
    let length = blocks.iter().map(|block| block.len()).sum::<usize>()
        + gaps.iter().map(|gap| gap.len()).sum::<usize>();
    let mut text = String::with_capacity(length);
    text.push_str(&gaps[0]);
    for (index, block) in blocks.iter().enumerate() {
        text.push_str(block);
        text.push_str(&gaps[index + 1]);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(source: &str) -> (Vec<Arc<String>>, Vec<Arc<String>>) {
        let parsed = parse::segments(source);
        (
            parsed.blocks.into_iter().map(Arc::new).collect(),
            parsed.gaps.into_iter().map(Arc::new).collect(),
        )
    }

    #[test]
    fn puts_the_document_back_the_way_it_was_found() {
        for text in [
            "# Heading  \n\n\nBody\n",
            "[home]: https://example.com\n\nGo [home].",
            "<section>raw</section>\n",
            "",
        ] {
            let (blocks, gaps) = document(text);
            assert_eq!(source(&blocks, &gaps), text);
        }
    }

    #[test]
    fn puts_a_selection_into_document_order_whichever_way_it_was_drawn() {
        let (blocks, _) = document("alpha\n\nbeta");
        let forwards = span(&blocks, 0, 2, 1, 3).expect("both blocks are there");
        let backwards = span(&blocks, 1, 3, 0, 2).expect("both blocks are there");
        assert_eq!(forwards, backwards);
        assert_eq!(forwards, Span { first: 0, first_at: 2, last: 1, last_at: 3 });
    }

    #[test]
    fn turns_a_selection_drawn_backwards_inside_one_block_the_right_way_round() {
        let (blocks, _) = document("alpha");
        let found = span(&blocks, 0, 4, 0, 1).expect("the block is there");
        assert_eq!(found, Span { first: 0, first_at: 1, last: 0, last_at: 4 });
    }

    #[test]
    fn counts_a_selection_in_characters() {
        // The emoji is one character and four bytes; the word after it starts at 2.
        let (blocks, _) = document("🙂 word");
        let found = span(&blocks, 0, 2, 0, 6).expect("the block is there");
        assert_eq!(found, Span { first: 0, first_at: 5, last: 0, last_at: 9 });
    }

    #[test]
    fn says_nothing_about_a_block_that_is_not_there() {
        let (blocks, _) = document("alpha");
        assert!(span(&blocks, 0, 0, 4, 0).is_none());
    }

    #[test]
    fn reads_a_selection_running_through_several_blocks_with_its_gaps() {
        let (blocks, gaps) = document("alpha\n\nbeta\n\ngamma");
        let found = span(&blocks, 0, 2, 2, 2).expect("the blocks are there");
        assert_eq!(selected_text(&blocks, &gaps, found), "pha\n\nbeta\n\nga");
    }

    #[test]
    fn reads_a_selection_inside_one_block() {
        let (blocks, gaps) = document("alpha\n\nbeta");
        let found = span(&blocks, 1, 0, 1, 3).expect("the block is there");
        assert_eq!(selected_text(&blocks, &gaps, found), "bet");
    }

    #[test]
    fn joins_what_is_left_at_either_end_of_a_selection_that_goes() {
        let (blocks, _) = document("alpha\n\nbeta\n\ngamma");
        let found = span(&blocks, 0, 2, 2, 2).expect("the blocks are there");
        // What was in front of the selection joined to what was behind it: "al" + "mma".
        assert_eq!(spliced(&blocks, found, ""), ("almma".to_string(), 2));
        assert_eq!(spliced(&blocks, found, "X"), ("alXmma".to_string(), 3));
    }

    #[test]
    fn leaves_the_cursor_after_what_was_typed_over_the_selection() {
        let (blocks, _) = document("🙂 alpha");
        let found = span(&blocks, 0, 2, 0, 7).expect("the block is there");
        // One for the emoji, one for the space, then the two typed.
        assert_eq!(spliced(&blocks, found, "hi"), ("🙂 hi".to_string(), 4));
    }

    #[test]
    fn re_reads_a_block_that_was_typed_into_several() {
        let (blocks, separators) = replacement("one\n\ntwo\n\nthree");
        assert_eq!(blocks, ["one", "two", "three"]);
        assert_eq!(separators, ["\n\n", "\n\n"]);
    }

    #[test]
    fn breaks_the_pictures_out_of_the_blocks_they_were_left_in() {
        let mut blocks = vec!["one ![a](1.png) two".to_string(), "plain".to_string()];
        let mut separators = vec!["\n\n".to_string()];
        assert!(hoist_images(&mut blocks, &mut separators));
        assert_eq!(blocks, ["one", "![a](1.png)", "two", "plain"]);
        assert_eq!(separators, ["\n\n", "\n\n", "\n\n"]);
    }

    #[test]
    fn leaves_a_document_with_no_picture_to_move_as_it_was() {
        let mut blocks = vec!["![a](1.png)".to_string(), "plain".to_string()];
        let mut separators = vec!["\n\n\n".to_string()];
        assert!(!hoist_images(&mut blocks, &mut separators));
        assert_eq!(blocks, ["![a](1.png)", "plain"]);
        assert_eq!(separators, ["\n\n\n"]);
    }

    /// The gaps around the outside of the document are none of the hoist's business: they
    /// are what keeps a file that is only read from coming back the way it was found.
    #[test]
    fn keeps_the_gaps_at_the_ends_of_the_document_while_hoisting() {
        let mut segments = parse::segments("\n\nwords ![a](1.png)\n\nplain\n");
        assert!(hoist_document(&mut segments));
        assert_eq!(segments.blocks, ["words", "![a](1.png)", "plain"]);
        assert_eq!(segments.gaps, ["\n\n", "\n\n", "\n\n", "\n"]);
    }

    /// A blank line the writer left over and above the one that ends a paragraph is a
    /// paragraph with nothing in it, which is what they typed to get it and what they
    /// see once it is back.
    #[test]
    fn opens_the_blank_lines_between_blocks_into_empty_paragraphs() {
        let mut blocks = vec!["one".to_string(), "two".to_string()];
        let mut separators = vec!["\n\n\n\n\n\n".to_string()];
        open_paragraphs(&mut blocks, &mut separators);
        assert_eq!(blocks, ["one", "", "", "two"]);
        assert_eq!(separators, ["\n\n", "\n\n", "\n\n"]);
    }

    #[test]
    fn leaves_a_gap_of_one_paragraph_break_as_it_is() {
        for gap in ["\n\n", "\n\n\n", "\n   \n"] {
            let mut blocks = vec!["one".to_string(), "two".to_string()];
            let mut separators = vec![gap.to_string()];
            open_paragraphs(&mut blocks, &mut separators);
            assert_eq!(blocks, ["one", "two"], "{gap:?}");
            assert_eq!(separators, [gap], "{gap:?}");
        }
    }

    /// The gap is cut in more places, never rewritten: what the pieces come to is the
    /// gap, so a document only read still saves back byte for byte.
    #[test]
    fn keeps_every_byte_of_a_gap_it_opens() {
        for gap in ["\n\n\n\n", "\n\n\n\n\n", "\n  \n\t\n\n", "\n\n\n\n\n\n\n"] {
            let mut blocks = vec!["one".to_string(), "two".to_string()];
            let mut separators = vec![gap.to_string()];
            open_paragraphs(&mut blocks, &mut separators);
            assert_eq!(separators.concat(), gap, "{gap:?}");
            assert!(blocks.len() > 2, "{gap:?}");
            assert!(blocks[1..blocks.len() - 1].iter().all(String::is_empty), "{gap:?}");
        }
    }

    #[test]
    fn opens_the_blank_lines_of_a_document_but_not_its_edges() {
        let mut segments = parse::segments("\n\n\nfirst\n\n\n\nsecond\n\n\n");
        open_paragraphs_document(&mut segments);
        assert_eq!(segments.blocks, ["first", "", "second"]);
        assert_eq!(segments.gaps, ["\n\n\n", "\n\n", "\n\n", "\n\n\n"]);
    }

    #[test]
    fn opens_the_blank_lines_of_a_block_that_was_typed_into_several() {
        let (blocks, separators) = replacement("one\n\n\n\ntwo");
        assert_eq!(blocks, ["one", "", "two"]);
        assert_eq!(separators, ["\n\n", "\n\n"]);
    }

    /// Whitespace at the ends of a block belongs to the block: the writer can still
    /// delete it.
    #[test]
    fn keeps_the_whitespace_around_a_block_inside_it() {
        let (blocks, separators) = replacement("  one  ");
        assert_eq!(blocks, ["  one  "]);
        assert!(separators.is_empty());
    }
}
