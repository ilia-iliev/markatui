//! Counting text two ways. Rust indexes a string in bytes; everything above these
//! functions counts in Unicode scalar values, because that is the unit a writer moves the
//! cursor by. Every offset that crosses between the two comes through here.

/// Where the character position `at` falls in `text`, which Rust counts in bytes.
/// A position past the end lands at the end rather than running off it.
pub fn byte_offset(text: &str, at: usize) -> usize {
    text.char_indices()
        .nth(at)
        .map(|(offset, _)| offset)
        .unwrap_or(text.len())
}

/// Where a byte offset into `text` stands, counted in characters.
pub fn char_at(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

/// How many characters `text` is.
pub fn length(text: &str) -> usize {
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_an_emoji_as_one() {
        assert_eq!(char_at("🙂 a", 5), 2);
        assert_eq!(byte_offset("🙂 a", 2), 5);
    }

    #[test]
    fn stops_at_the_end_rather_than_running_past_it() {
        assert_eq!(byte_offset("ab", 9), 2);
        assert_eq!(char_at("ab", 9), 2);
    }
}
