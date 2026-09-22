use harper_core::spell::{Dictionary as _, FstDictionary, suggest_correct_spelling_str};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

struct Speller {
    dictionary: Arc<FstDictionary>,
    personal: RwLock<HashSet<String>>,
}

fn personal_words() -> HashSet<String> {
    let Some(path) = crate::storage::dictionary() else {
        return HashSet::new();
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return HashSet::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|word| !word.is_empty() && !word.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Harper's American-English dictionary is compiled into the program. Keep one handle to
/// it beside the writer's own words, and initialize both away from the UI thread.
static SPELLER: OnceLock<Speller> = OnceLock::new();

pub fn preload() {
    std::thread::spawn(|| {
        speller();
    });
}

/// Whether the built-in dictionary is ready. Asking it a question before then would make
/// the UI wait while Harper expands its compiled word list and builds its search index.
pub fn ready() -> bool {
    SPELLER.get().is_some()
}

fn speller() -> &'static Speller {
    SPELLER.get_or_init(|| Speller {
        dictionary: FstDictionary::curated(),
        personal: RwLock::new(personal_words()),
    })
}

/// Whether the built-in dictionary or the writer's own words know `word`.
pub fn known(word: &str) -> bool {
    let speller = speller();
    speller.personal.read().unwrap().contains(word) || speller.dictionary.contains_word_str(word)
}

/// What the built-in dictionary would put in place of a word it does not know, likeliest
/// first. Cut short: these are cycled through one at a time, and nobody reaches the ninth.
pub fn suggestions(word: &str) -> Vec<String> {
    suggest_correct_spelling_str(word, 8, 3, speller().dictionary.as_ref())
}

/// Take `word` into the writer's own dictionary now and on subsequent runs.
pub fn learn(word: &str) {
    speller().personal.write().unwrap().insert(word.to_string());
    if let Some(path) = crate::storage::dictionary() {
        remember(&path, word);
    }
}

/// Write `word` at the end of the writer's dictionary file, making the file if this is
/// the first word they have taken into it.
fn remember(path: &Path, word: &str) {
    let mut text = fs::read_to_string(path).unwrap_or_default();
    if !(text.is_empty() || text.ends_with('\n')) {
        text.push('\n');
    }
    text.push_str(word);
    text.push('\n');

    crate::storage::replace(path, text.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_a_word_and_does_not_know_a_typo() {
        assert!(known("receive"));
        assert!(!known("recieve"));
    }

    #[test]
    fn suggests_the_word_that_was_meant() {
        assert!(suggestions("recieve").contains(&"receive".to_string()));
    }
}
