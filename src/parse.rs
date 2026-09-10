use pulldown_cmark::{Event, Options, Parser};
use std::ops::Range;

/// Top-level Markdown blocks and the source between them. `gaps` has one more item than
/// `blocks`: before the first block, between each pair, and after the last. Joining them
/// recreates the input byte for byte.
#[derive(Debug, PartialEq, Eq)]
pub struct Segments {
    pub blocks: Vec<String>,
    pub gaps: Vec<String>,
}

pub fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_FOOTNOTES
}

/// Split Markdown while retaining every byte outside its semantic blocks. Pulldown-cmark
/// deliberately emits no event for some source, notably reference definitions. Any such
/// non-whitespace gap becomes a block of its own rather than disappearing on save.
pub fn segments(source: &str) -> Segments {
    let mut ranges = semantic_ranges(source);
    for range in &mut ranges {
        range.end = range.start + source[range.clone()].trim_end().len();
    }
    ranges.retain(|range| !range.is_empty());
    include_unparsed(source, &mut ranges);
    ranges.sort_by_key(|range| range.start);

    if ranges.is_empty() {
        return Segments {
            blocks: vec![source.to_string()],
            gaps: vec![String::new(), String::new()],
        };
    }

    let blocks = ranges.iter().map(|range| source[range.clone()].to_string()).collect();
    let mut gaps = Vec::with_capacity(ranges.len() + 1);
    gaps.push(source[..ranges[0].start].to_string());
    for pair in ranges.windows(2) {
        gaps.push(source[pair[0].end..pair[1].start].to_string());
    }
    gaps.push(source[ranges.last().expect("there is a range").end..].to_string());
    Segments { blocks, gaps }
}

fn semantic_ranges(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;

    for (event, range) in Parser::new_ext(source, options()).into_offset_iter() {
        match event {
            Event::Start(_) => {
                if depth == 0 {
                    start = range.start;
                }
                depth += 1;
            }
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    ranges.push(start..range.end);
                }
            }
            _ if depth == 0 => ranges.push(range),
            _ => {}
        }
    }
    ranges
}

fn include_unparsed(source: &str, ranges: &mut Vec<Range<usize>>) {
    ranges.sort_by_key(|range| range.start);
    let mut uncovered = Vec::new();
    let mut end = 0;
    for range in ranges.iter() {
        add_non_whitespace(source, end..range.start, &mut uncovered);
        end = end.max(range.end);
    }
    add_non_whitespace(source, end..source.len(), &mut uncovered);
    ranges.extend(uncovered);
}

fn add_non_whitespace(source: &str, range: Range<usize>, ranges: &mut Vec<Range<usize>>) {
    let part = &source[range.clone()];
    let Some(first) = part.find(|character: char| !character.is_whitespace()) else {
        return;
    };
    let last = part
        .rfind(|character: char| !character.is_whitespace())
        .expect("a first non-whitespace character has a last");
    let last = last + part[last..].chars().next().expect("last points at a character").len_utf8();
    ranges.push(range.start + first..range.start + last);
}

/// What kind of block this is, which is what decides how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Paragraph,
    Heading,
    Code,
    Quote,
    Table,
    List,
    Rule,
    Image,
}

pub fn kind(block: &str) -> Kind {
    use pulldown_cmark::Tag;

    if lone_image(block).is_some() {
        return Kind::Image;
    }
    // The first event names the block: everything after it is inside that block.
    match Parser::new_ext(block, options()).next() {
        Some(Event::Start(Tag::Heading { .. })) => Kind::Heading,
        Some(Event::Start(Tag::CodeBlock(_))) => Kind::Code,
        Some(Event::Start(Tag::BlockQuote(_))) => Kind::Quote,
        Some(Event::Start(Tag::Table(_))) => Kind::Table,
        Some(Event::Start(Tag::List(_))) => Kind::List,
        Some(Event::Rule) => Kind::Rule,
        _ => Kind::Paragraph,
    }
}

/// The image path of a block that is a lone `![alt](path)` paragraph, if any.
pub fn lone_image(block: &str) -> Option<String> {
    use pulldown_cmark::{Tag, TagEnd};

    let mut path = None;
    let mut in_image = false;

    for event in Parser::new_ext(block, options()) {
        match event {
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
            Event::Start(Tag::Image { dest_url, .. }) => {
                if path.is_some() {
                    return None;
                }
                path = Some(dest_url.to_string());
                in_image = true;
            }
            Event::End(TagEnd::Image) => in_image = false,
            // Alt text lives inside the image; anything outside it disqualifies the block.
            _ if in_image => {}
            Event::Text(text) if text.trim().is_empty() => {}
            _ => return None,
        }
    }

    path
}
/// A paragraph broken up so that every picture the writer left among the words becomes a
/// paragraph of its own: markdown holds a picture inline, and a terminal has nowhere to
/// draw one but rows of its own. `None` where there is nothing to move.
pub fn hoisted_images(block: &str) -> Option<Vec<String>> {
    let mut pieces = Vec::new();
    let mut end = 0;
    for range in loose_images(block) {
        add_piece(&block[end..range.start], &mut pieces);
        add_piece(&block[range.clone()], &mut pieces);
        end = range.end;
    }
    add_piece(&block[end..], &mut pieces);
    (pieces.len() > 1).then_some(pieces)
}

fn add_piece(part: &str, pieces: &mut Vec<String>) {
    let part = part.trim();
    if !part.is_empty() {
        pieces.push(part.to_string());
    }
}

/// Where the pictures a paragraph holds itself sit in its source. Only those move: one in
/// a heading or a list item would take the block apart, and one inside a link or an
/// emphasis would leave the markers around it holding nothing.
fn loose_images(block: &str) -> Vec<Range<usize>> {
    use pulldown_cmark::Tag;

    let mut ranges = Vec::new();
    let mut depth = 0usize;
    for (event, range) in Parser::new_ext(block, options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Paragraph) if depth == 0 => depth = 1,
            Event::Start(_) if depth == 0 => return Vec::new(),
            Event::Start(Tag::Image { .. }) if depth == 1 => {
                ranges.push(range);
                depth += 1;
            }
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            _ => {}
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The blocks of `source`, which is what the model is built out of. An empty
    /// document is one empty block, so that there is always somewhere to type.
    fn segment(source: &str) -> Vec<String> {
        if source.trim().is_empty() {
            return vec![String::new()];
        }
        segments(source).blocks
    }

    /// The blocks put back together with one blank line between them: what the document
    /// would look like had it been written out afresh rather than kept as it was found.
    fn roundtrip(source: &str) -> String {
        segment(source).join("\n\n")
    }

    /// The document put back together byte for byte, which is what saving does.
    fn exact_roundtrip(source: &str) -> String {
        let Segments { blocks, gaps } = segments(source);
        let mut text = gaps[0].clone();
        for (index, block) in blocks.iter().enumerate() {
            text.push_str(block);
            text.push_str(&gaps[index + 1]);
        }
        text
    }

    #[test]
    fn splits_headings_and_paragraphs() {
        let blocks = segment("# Title\n\nA paragraph.\n\n## Sub\n\nAnother one.\n");
        assert_eq!(blocks, ["# Title", "A paragraph.", "## Sub", "Another one."]);
    }

    #[test]
    fn keeps_a_list_whole() {
        let source = "Intro\n\n- one\n- two\n  - nested\n- three\n\nOutro";
        let blocks = segment(source);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1], "- one\n- two\n  - nested\n- three");
        assert_eq!(roundtrip(source), source);
    }

    #[test]
    fn keeps_a_fenced_code_block_whole() {
        let source = "```rust\nfn main() {\n\n    println!(\"hi\");\n}\n```";
        assert_eq!(segment(source), [source]);
        assert_eq!(roundtrip(source), source);
    }

    #[test]
    fn keeps_a_blockquote_whole() {
        let source = "> quoted\n> lines\n\nafter";
        assert_eq!(segment(source), ["> quoted\n> lines", "after"]);
    }

    #[test]
    fn preserves_setext_headings() {
        let source = "Title\n=====\n\nbody";
        assert_eq!(segment(source), ["Title\n=====", "body"]);
    }

    #[test]
    fn handles_rules_and_tables() {
        let source = "a\n\n---\n\n| x | y |\n| - | - |\n| 1 | 2 |\n\nb";
        let blocks = segment(source);
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[1], "---");
        assert_eq!(roundtrip(source), source);
    }

    #[test]
    fn semantic_join_normalizes_blank_lines() {
        assert_eq!(roundtrip("a\n\n\n\nb"), "a\n\nb");
    }

    #[test]
    fn exact_segments_preserve_all_whitespace() {
        for source in ["a\n\n\n\nb", "  a  \n\n b\n", "", "   \n\n "] {
            assert_eq!(exact_roundtrip(source), source);
        }
    }

    #[test]
    fn exact_segments_keep_html_and_reference_definitions() {
        for source in
            ["<section>\nraw html\n</section>\n", "[home]: https://example.com\n\nGo [home].\n"]
        {
            assert_eq!(exact_roundtrip(source), source);
            assert!(segments(source).blocks.iter().any(|block| !block.is_empty()));
        }
    }

    #[test]
    fn empty_source_is_one_empty_block() {
        assert_eq!(segment(""), [""]);
        assert_eq!(segment("   \n\n "), [""]);
    }

    #[test]
    fn names_block_kinds() {
        assert_eq!(kind("# Title"), Kind::Heading);
        assert_eq!(kind("Title\n====="), Kind::Heading);
        assert_eq!(kind("Just words."), Kind::Paragraph);
        assert_eq!(kind("- one\n- two"), Kind::List);
        assert_eq!(kind("1. one"), Kind::List);
        assert_eq!(kind("```rust\nfn main() {}\n```"), Kind::Code);
        assert_eq!(kind("> quoted"), Kind::Quote);
        assert_eq!(kind("| a | b |\n| - | - |\n| 1 | 2 |"), Kind::Table);
        assert_eq!(kind("---"), Kind::Rule);
        assert_eq!(kind("![alt](pic.png)"), Kind::Image);
        assert_eq!(kind(""), Kind::Paragraph);
    }

    #[test]
    fn breaks_a_paragraph_up_round_the_pictures_left_in_it() {
        assert_eq!(
            hoisted_images("![a](1.png) after"),
            Some(vec!["![a](1.png)".into(), "after".into()])
        );
        assert_eq!(
            hoisted_images("before ![a](1.png)"),
            Some(vec!["before".into(), "![a](1.png)".into()])
        );
        assert_eq!(
            hoisted_images("before ![a](1.png) after"),
            Some(vec!["before".into(), "![a](1.png)".into(), "after".into()])
        );
        assert_eq!(
            hoisted_images("![a](1.png) ![b](2.png)"),
            Some(vec!["![a](1.png)".into(), "![b](2.png)".into()])
        );
        // A picture on a line of its own inside a paragraph is inline all the same.
        assert_eq!(
            hoisted_images("words\n![a](1.png)"),
            Some(vec!["words".into(), "![a](1.png)".into()])
        );
        // What is around a picture keeps its markers.
        assert_eq!(
            hoisted_images("**bold** ![a](1.png)"),
            Some(vec!["**bold**".into(), "![a](1.png)".into()])
        );
    }

    /// Only a paragraph's own pictures move. One in a heading or a list would take the
    /// block apart, and one inside a link or an emphasis would leave the markers around
    /// it holding nothing.
    #[test]
    fn leaves_alone_a_block_with_no_picture_to_move() {
        for block in [
            "![alt](pic.png)",
            "just words",
            "",
            "# heading ![a](1.png)",
            "- item ![a](1.png)",
            "> quoted ![a](1.png)",
            "see [![a](1.png)](https://example.com)",
            "*words ![a](1.png)*",
            "`![a](1.png)`",
        ] {
            assert_eq!(hoisted_images(block), None, "{block:?}");
        }
    }

    #[test]
    fn detects_lone_images() {
        assert_eq!(lone_image("![alt](pic.png)"), Some("pic.png".into()));
        assert_eq!(lone_image("text ![alt](pic.png) more"), None);
        assert_eq!(lone_image("# heading"), None);
        assert_eq!(lone_image("![a](1.png) ![b](2.png)"), None);
    }
}
