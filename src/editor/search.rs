//! Where a word turns up in a document held as blocks, and which of those places is
//! being looked at.

use crate::text::char_at;
use std::sync::Arc;

/// One occurrence: the block it stands in, and where it starts and ends inside that
/// block, counted in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurrence {
    pub block: usize,
    pub at: usize,
    pub end: usize,
}

/// What the search is looking at: every occurrence of the word it was given, in document
/// order, and which of them is under the cursor.
#[derive(Default)]
pub struct Search {
    found: Vec<Occurrence>,
    choice: usize,
}

impl Search {
    /// Look for `needle` from scratch. The first occurrence is the one put on show, and
    /// there is none to show for a word that is not in the document.
    pub fn look_for(&mut self, blocks: &[Arc<String>], needle: &str) -> Option<Occurrence> {
        self.found = occurrences(blocks, needle);
        self.choice = 0;
        self.showing()
    }

    /// Walk `direction` through the occurrences, coming back round the document at
    /// either end. `None` where there is nowhere to go, which is a word that turns up
    /// once or not at all.
    pub fn walk(&mut self, direction: i32) -> Option<Occurrence> {
        let count = self.found.len();
        if count < 2 {
            return None;
        }
        self.choice = (self.choice as i32 + direction).rem_euclid(count as i32) as usize;
        self.showing()
    }

    fn showing(&self) -> Option<Occurrence> {
        self.found.get(self.choice).copied()
    }

    pub fn count(&self) -> usize {
        self.found.len()
    }

    /// Which occurrence is on show, or nowhere when the word is not in the document.
    pub fn choice(&self) -> Option<usize> {
        (!self.found.is_empty()).then_some(self.choice)
    }

    pub fn forget(&mut self) {
        self.found.clear();
        self.choice = 0;
    }
}

/// Every occurrence of `needle`, in document order, ignoring case. Occurrences do not
/// overlap: `aa` turns up twice in `aaaa`, not three times.
pub fn occurrences(blocks: &[Arc<String>], needle: &str) -> Vec<Occurrence> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        let mut skip_to = 0;
        for (at, _) in block.char_indices() {
            if at < skip_to {
                continue;
            }
            let Some(length) = length_at(block, at, needle) else {
                continue;
            };
            found.push(Occurrence {
                block: index,
                at: char_at(block, at),
                end: char_at(block, at + length),
            });
            skip_to = at + length;
        }
    }
    found
}

/// How many bytes of `haystack` from `at` are `needle`, ignoring case, or `None` where it
/// does not stand there. The two are walked a character at a time rather than lowercased
/// whole: folding a string can change its length, and these offsets have to point back
/// into the text as it was written.
fn length_at(haystack: &str, at: usize, needle: &str) -> Option<usize> {
    let mut length = 0;
    let mut found = haystack[at..].chars();
    for wanted in needle.chars() {
        let character = found.next()?;
        if !same(character, wanted) {
            return None;
        }
        length += character.len_utf8();
    }
    Some(length)
}

fn same(one: char, other: char) -> bool {
    one == other || one.to_lowercase().eq(other.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn document(source: &str) -> Vec<Arc<String>> {
        parse::segments(source).blocks.into_iter().map(Arc::new).collect()
    }

    /// The document the walking tests use: one occurrence of `marker` in each of three
    /// blocks, and a word that turns up once.
    fn marked() -> Vec<Arc<String>> {
        document("One marker here.\n\nTwo marker there.\n\nA solitary marker word.")
    }

    #[test]
    fn finds_a_word_in_every_block_it_stands_in() {
        assert_eq!(
            occurrences(&marked(), "marker"),
            [
                Occurrence { block: 0, at: 4, end: 10 },
                Occurrence { block: 1, at: 4, end: 10 },
                Occurrence { block: 2, at: 11, end: 17 },
            ]
        );
    }

    #[test]
    fn ignores_the_case_it_was_typed_in() {
        let blocks = document("Cat CAT cat");
        let found = occurrences(&blocks, "cAt");
        assert_eq!(found.len(), 3);
        assert_eq!(found[1], Occurrence { block: 0, at: 4, end: 7 });
    }

    #[test]
    fn counts_in_characters() {
        // The emoji is one character and four bytes.
        let blocks = document("🙂 word");
        assert_eq!(occurrences(&blocks, "word"), [Occurrence { block: 0, at: 2, end: 6 }]);
    }

    #[test]
    fn does_not_let_one_occurrence_run_into_the_next() {
        let blocks = document("aaaa");
        assert_eq!(
            occurrences(&blocks, "aa"),
            [Occurrence { block: 0, at: 0, end: 2 }, Occurrence { block: 0, at: 2, end: 4 }]
        );
    }

    #[test]
    fn shows_the_first_occurrence_as_the_word_is_typed() {
        let blocks = marked();
        let mut search = Search::default();
        // Each letter is a search of its own: the writer is still typing.
        assert_eq!(search.look_for(&blocks, "mark"), Some(Occurrence { block: 0, at: 4, end: 8 }));
        assert_eq!(
            search.look_for(&blocks, "marker"),
            Some(Occurrence { block: 0, at: 4, end: 10 })
        );
        assert_eq!(search.choice(), Some(0));
        assert_eq!(search.count(), 3);
    }

    #[test]
    fn walks_the_occurrences_both_ways() {
        let blocks = marked();
        let mut search = Search::default();
        search.look_for(&blocks, "marker");
        assert_eq!(search.walk(1), Some(Occurrence { block: 1, at: 4, end: 10 }));
        assert_eq!(search.walk(1), Some(Occurrence { block: 2, at: 11, end: 17 }));
        assert_eq!(search.walk(-1), Some(Occurrence { block: 1, at: 4, end: 10 }));
        assert_eq!(search.choice(), Some(1));
    }

    #[test]
    fn comes_back_round_the_document_at_either_end() {
        let blocks = marked();
        let mut search = Search::default();
        search.look_for(&blocks, "marker");
        // Backwards off the front of the document lands on the last occurrence.
        assert_eq!(search.walk(-1), Some(Occurrence { block: 2, at: 11, end: 17 }));
        assert_eq!(search.walk(1), Some(Occurrence { block: 0, at: 4, end: 10 }));
    }

    #[test]
    fn has_nowhere_to_walk_for_a_word_that_turns_up_once() {
        let blocks = marked();
        let mut search = Search::default();
        let alone = search.look_for(&blocks, "solitary");
        assert_eq!(search.count(), 1);
        assert_eq!(search.walk(1), None);
        assert_eq!(search.walk(-1), None);
        // And nothing moved: the one occurrence is still the one on show.
        assert_eq!(search.showing(), alone);
    }

    #[test]
    fn finds_nothing_for_nothing() {
        let blocks = marked();
        let mut search = Search::default();
        for needle in ["", "nowhere"] {
            assert_eq!(search.look_for(&blocks, needle), None);
            assert_eq!(search.count(), 0);
            assert_eq!(search.choice(), None);
            assert_eq!(search.walk(1), None);
        }
    }

    #[test]
    fn forgets_what_it_was_looking_at_when_it_closes() {
        let blocks = marked();
        let mut search = Search::default();
        search.look_for(&blocks, "marker");
        search.walk(1);
        search.forget();
        assert_eq!(search.count(), 0);
        assert_eq!(search.choice(), None);
        assert_eq!(search.showing(), None);
    }
}
