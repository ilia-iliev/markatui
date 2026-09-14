use crate::parse;
use crate::spell;
use crate::text::char_at;
use harper_core::linting::{FlatConfig, LintGroup, LintKind, StructuredConfig, Suggestion};
use harper_core::spell::{FstDictionary, MutableDictionary};
use harper_core::{Dialect, Document, TokenKind};
use pulldown_cmark::{Event, Parser, Tag};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};

/// Something the checker found in a block: where it is, counted in the characters the
/// cursor moves by; what is wrong with it, in a few words of markdown; and what could
/// stand in its place, likeliest first.
///
/// A misspelling and a turn of phrase are the same thing here. They are marked the same
/// way, offered the same way, and accepted the same way; only the message says which of
/// the two it was.
#[derive(Clone, Debug)]
pub struct Lint {
    pub at: usize,
    pub len: usize,
    pub message: String,
    pub replacements: Vec<String>,
    /// The misspelled word itself, where that is what this is: the one the writer would
    /// be taking into their dictionary. Empty for a turn of phrase, which is nothing a
    /// dictionary has an opinion about.
    pub word: String,
    /// Which of the checker's rules objected, by the name the config turns it off under.
    /// Empty for a misspelling: that is the dictionary's business and no rule of ours.
    pub rule: String,
    /// Whether the replacements are still to be worked out. Asking the dictionary what a
    /// misspelled word should have been costs more than checking the block it is in, and
    /// only the one lint the cursor stands in is ever read.
    pending: bool,
}

/// Punctuation that only ever joins one thing to another. A word with one of these hard
/// against it is part of `snake_case`, a path or an address rather than a piece of prose.
/// The full stop that ends a sentence joins nothing to anything, and the word in front of
/// it is checked like any other.
const JOINS: [char; 5] = ['.', '/', ':', '@', '_'];

const CACHE_LIMIT: usize = 256;

/// What the writer's config had to say about the checker's rules: the name of a rule, and
/// whether it is to run. Only the ones spoken for are in here; the rest stay as curated.
pub type Checks = BTreeMap<String, bool>;

/// Turn the writer's rules on and off, then take the spell checker out whatever they
/// said: personal words and lazy suggestions need the spelling pass this file does
/// itself, and running harper's as well would mark the same word twice.
fn configure(config: &mut FlatConfig, checks: &Checks) {
    for (rule, on) in checks {
        config.set_rule_enabled(rule, *on);
    }
    config.set_rule_enabled("SpellCheck", false);
}

/// Whether the checker has a rule of this name, so that a config naming one it does not
/// have is a line the editor complains about rather than a check quietly left on.
///
/// Asked while the writer is waiting for their document, so it is asked of the curated
/// settings rather than of a [`LintGroup`]. The two hold the same names — a group takes
/// its own config from these settings — but a group is every rule built, better than half
/// a second of it, and the question here is only what the names are.
pub fn has_rule(name: &str) -> bool {
    static NAMES: OnceLock<FlatConfig> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            StructuredConfig::curated().to_flat_config().expect("the curated settings are a config")
        })
        .has_rule(name)
}

/// Every rule the writer could turn off, each with the one line harper says about what it
/// looks for. Built here and thrown away: this is what `markatui checks` prints, and it
/// waits on nothing. The dictionary a rule would read does not change its name.
pub fn rules() -> Vec<(String, String)> {
    let group = LintGroup::new_curated(MutableDictionary::new().into(), Dialect::American);
    let mut listed: Vec<(String, String)> = group
        .all_descriptions()
        .into_iter()
        .map(|(name, description)| (name.to_string(), description.to_string()))
        .collect();
    listed.sort();
    listed
}

type CheckKey = String;

struct CheckCache {
    found: HashMap<CheckKey, Vec<Lint>>,
    order: VecDeque<CheckKey>,
    pending: HashSet<CheckKey>,
}

struct Checker {
    group: Arc<Mutex<LintGroup>>,
    cache: Arc<Mutex<CheckCache>>,
    requests: mpsc::Sender<CheckKey>,
}

static CHECKER: OnceLock<Checker> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Build the checker on threads of its own, so that the four hundred milliseconds it
/// takes are spent while the first frame is being drawn. `checks` is what the writer's
/// config had to say about the rules, by the names [`rules`] prints.
pub fn preload(checks: &Checks) {
    spell::preload();
    let checks = checks.clone();
    std::thread::spawn(move || {
        let mut rules = LintGroup::new_curated(FstDictionary::curated(), Dialect::American);
        configure(&mut rules.config, &checks);

        let group = Arc::new(Mutex::new(rules));
        let cache = Arc::new(Mutex::new(CheckCache {
            found: HashMap::new(),
            order: VecDeque::new(),
            pending: HashSet::new(),
        }));
        let (send, receive) = mpsc::channel();
        if CHECKER
            .set(Checker { group: group.clone(), cache: cache.clone(), requests: send })
            .is_err()
        {
            return;
        }
        GENERATION.fetch_add(1, Ordering::Release);

        for key in receive {
            let found = run(&group, &key);
            remember_check(&mut cache.lock().unwrap(), key, found);
            GENERATION.fetch_add(1, Ordering::Release);
        }
    });
}

/// Whether the checker is up yet. Its dictionaries and its rules take the better part
/// of a second between them — longer than the first frame takes to appear. Nothing in the
/// editor waits on it: the tests below, and the editor's own, are what have to.
#[cfg(test)]
pub fn ready() -> bool {
    CHECKER.get().is_some() && spell::ready()
}

/// Changes whenever cached findings may have changed. The event loop reads this small
/// value each tick; checking itself stays off the drawing thread.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// Return findings already worked out for this block. A miss schedules one and returns
/// immediately; the generation change has the event loop ask again when it finishes.
fn request_check(text: &str) -> Vec<Lint> {
    let Some(checker) = CHECKER.get().filter(|_| spell::ready()) else {
        return Vec::new();
    };
    let key = text.to_string();
    let mut cache = checker.cache.lock().unwrap();
    if let Some(found) = cache.found.get(&key) {
        return found.clone();
    }
    if cache.pending.insert(key.clone()) {
        let _ = checker.requests.send(key);
    }
    Vec::new()
}

/// Synchronous checking is kept inside the Rust core for focused tests. UI callers use
/// [`request_check`] and never wait for Harper.
#[cfg(test)]
fn check(text: &str) -> Vec<Lint> {
    let Some(checker) = CHECKER.get().filter(|_| spell::ready()) else {
        return Vec::new();
    };
    run(&checker.group, text)
}

fn remember_check(cache: &mut CheckCache, key: CheckKey, found: Vec<Lint>) {
    cache.pending.remove(&key);
    if cache.found.insert(key.clone(), found).is_none() {
        cache.order.push_back(key);
    }
    while cache.order.len() > CACHE_LIMIT {
        if let Some(oldest) = cache.order.pop_front() {
            cache.found.remove(&oldest);
        }
    }
}

/// Where in a block the checker took exception to something, for the wash the view puts
/// under those words. Only what is already worked out; a miss schedules the work.
pub fn marks(text: &str) -> Vec<Range<usize>> {
    request_check(text).into_iter().map(|lint| lint.at..lint.at + lint.len).collect()
}

/// What the checker makes of the place the cursor is standing in a block, if anything.
/// The narrowest lint wins where several overlap — it is the one that names the words
/// under the cursor — and it is the only one whose replacements are worth working out.
pub fn at(text: &str, cursor: usize) -> Option<Lint> {
    #[cfg(test)]
    let checked = check(text);
    #[cfg(not(test))]
    let checked = request_check(text);
    let mut found = checked
        .into_iter()
        .filter(|lint| cursor >= lint.at && cursor <= lint.at + lint.len)
        .min_by_key(|lint| lint.len)?;
    if found.pending {
        found.replacements = spell::suggestions(&found.word);
        found.pending = false;
    }
    Some(found)
}

/// Take a word into the writer's own dictionary: it is spelled right from here on, in
/// this document and the next.
pub fn learn(word: &str) {
    spell::learn(word);
    start_over();
}

/// Stop running the rule called `name` for the rest of this run. Keeping it off past the
/// end of it is the config's business; this is what the editor does about the tip that is
/// on the screen when the writer says they never want to see it.
pub fn mute(name: &str) {
    let Some(checker) = CHECKER.get() else { return };
    checker.group.lock().unwrap().config.set_rule_enabled(name, false);
    start_over();
}

/// Forget everything made of every block. What the checker would say has changed under
/// it, and the blocks on the screen are asked about again.
fn start_over() {
    let Some(checker) = CHECKER.get() else { return };
    let mut cache = checker.cache.lock().unwrap();
    cache.found.clear();
    cache.order.clear();
    cache.pending.clear();
    GENERATION.fetch_add(1, Ordering::Release);
}

fn run(group: &Mutex<LintGroup>, text: &str) -> Vec<Lint> {
    let document = Document::new_markdown_default_curated(text);
    let characters: Vec<char> = text.chars().collect();

    let mut found: Vec<Lint> = group
        .lock()
        .unwrap()
        // By rule, not in a heap: the name of the rule that objected is what the writer
        // turns that objection off by, and the only place to get it is here.
        .organized_lints(&document)
        .into_iter()
        .flat_map(|(rule, lints)| lints.into_iter().map(move |lint| (rule.clone(), lint)))
        // Spelling is handled below against Harper's built-in dictionary plus the
        // writer's own words; the group's spelling rules would mark the same text twice.
        .filter(|(_, lint)| !matches!(lint.lint_kind, LintKind::Spelling))
        .filter_map(|(rule, lint)| carry(rule, lint, &characters))
        .collect();
    found.extend(misspellings(&document, &characters));
    let left_alone = left_alone(text);
    found.retain(|lint| !left_alone.iter().any(|part| overlaps(lint, part)));
    found.sort_by_key(|lint| (lint.at, lint.len));
    found
}

/// The parts of a block the checker has no business in, in the characters a lint counts
/// in: code, links and tables. None of it is prose — it is a name, an address, a column of
/// figures — and a writer who has to spell it that way cannot take the advice anyway.
fn left_alone(text: &str) -> Vec<Range<usize>> {
    Parser::new_ext(text, parse::options())
        .into_offset_iter()
        .filter(|(event, _)| {
            matches!(
                event,
                Event::Code(_) | Event::Start(Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Table(_))
            )
        })
        .map(|(_, bytes)| char_at(text, bytes.start)..char_at(text, bytes.end))
        .collect()
}

/// Whether a lint has any of itself inside `part`.
fn overlaps(lint: &Lint, part: &Range<usize>) -> bool {
    lint.at < part.end && part.start < lint.at + lint.len
}

/// The words of a block the dictionary does not know. Which of the text is prose, harper
/// has already worked out: an address is a token of its own and not a word, so the
/// question is never asked about it. What is left over, [`left_alone`] takes out.
fn misspellings(document: &Document, characters: &[char]) -> Vec<Lint> {
    document
        .get_tokens()
        .iter()
        .filter(|token| matches!(token.kind, TokenKind::Word(_)))
        .map(|token| token.span.start..token.span.end)
        .filter(|span| checkable(span.clone(), characters))
        .filter_map(|span| {
            let word: String = characters[span.clone()].iter().collect();
            if spell::known(&word) {
                return None;
            }
            Some(Lint {
                at: span.start,
                len: span.end - span.start,
                message: format!("`{word}` is not in the dictionary."),
                replacements: Vec::new(),
                word,
                rule: String::new(),
                pending: true,
            })
        })
        .collect()
}

/// Whether a word harper found is one to ask the dictionary about. A single letter is
/// never worth flagging, and anything with a digit in it is a name for something rather
/// than a word: `h1`, `3rd`, `utf8`. What sits either side of it counts too — see [`JOINS`].
fn checkable(span: Range<usize>, characters: &[char]) -> bool {
    let word = &characters[span.clone()];
    if word.len() < 2 || word.iter().any(|c| c.is_numeric() || JOINS.contains(c)) {
        return false;
    }
    let before = span.start.checked_sub(1).and_then(|i| characters.get(i));
    if before.is_some_and(|c| JOINS.contains(c)) {
        return false;
    }
    let after = characters.get(span.end);
    let beyond = characters.get(span.end + 1);
    !(after.is_some_and(|c| JOINS.contains(c)) && beyond.is_some_and(|c| c.is_alphanumeric()))
}

/// A harper lint in the terms the editor works in: character offsets, and for each
/// suggestion the one piece of text that should stand where the lint is, whichever shape
/// the suggestion took. Taking the words out is a piece of text like any other — an
/// empty one.
fn carry(rule: String, lint: harper_core::linting::Lint, characters: &[char]) -> Option<Lint> {
    let marked = characters.get(lint.span.start..lint.span.end)?;
    let replacements = lint
        .suggestions
        .iter()
        .map(|suggestion| match suggestion {
            Suggestion::ReplaceWith(with) => with.iter().collect(),
            Suggestion::InsertAfter(after) => marked.iter().chain(after.iter()).collect(),
            Suggestion::Remove => String::new(),
        })
        .collect();
    Some(Lint {
        at: lint.span.start,
        len: lint.span.end - lint.span.start,
        message: lint.message,
        replacements,
        word: String::new(),
        rule,
        pending: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The piece of `text` a lint covers, taken in the characters the lint counts in.
    fn covered(text: &str, lint: &Lint) -> String {
        text.chars().skip(lint.at).take(lint.len).collect()
    }

    /// The checker loads on threads of its own; the tests share one and wait for it once.
    fn checker() {
        preload(&Checks::new());
        while !ready() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The lints of a block, as (what they cover, what is offered for it).
    fn found(text: &str) -> Vec<(String, Vec<String>)> {
        checker();
        check(text)
            .into_iter()
            .map(|lint| {
                let covering = covered(text, &lint);
                let offered = if lint.pending {
                    spell::suggestions(&covering)
                } else {
                    lint.replacements.clone()
                };
                (covering, offered)
            })
            .collect()
    }

    /// What the block covers and what it offers where the cursor is standing.
    fn under_cursor(text: &str, cursor: usize) -> Option<(String, Vec<String>)> {
        checker();
        let lint = at(text, cursor)?;
        Some((covered(text, &lint), lint.replacements))
    }

    fn covers(text: &str) -> Vec<String> {
        found(text).into_iter().map(|(covering, _)| covering).collect()
    }

    #[test]
    fn finds_a_typo_and_offers_what_was_meant() {
        let (word, offered) = under_cursor("I recieve mail.", 4).expect("the typo is found");
        assert_eq!(word, "recieve");
        assert!(offered.contains(&"receive".to_string()), "{offered:?}");
    }

    #[test]
    fn finds_a_turn_of_phrase_and_offers_what_was_meant() {
        checker();
        let (phrase, offered) =
            under_cursor("This is very unique writing.", 14).expect("the phrase is found");
        assert_eq!(phrase, "very unique");
        assert!(offered.len() > 1, "{offered:?}");
    }

    /// The point of the exercise: one kind of finding, one shape, one way to accept it.
    /// A block with a typo and a bad turn of phrase gives two lints in reading order,
    /// each covering its own words and each with something to put there.
    #[test]
    fn marks_a_typo_and_a_phrase_the_same_way() {
        let lints = found("This is very unique and I recieve it.");
        let covering: Vec<String> = lints.iter().map(|(word, _)| word.clone()).collect();
        assert_eq!(covering, ["very unique", "recieve"]);
        for (word, offered) in lints {
            assert!(!offered.is_empty(), "nothing offered for {word}");
        }
    }

    #[test]
    fn leaves_alone_what_was_not_written_as_prose() {
        assert_eq!(covers("Call `recieve_this` now."), Vec::<String>::new());
        assert_eq!(covers("Read\n\n```\nrecieve\n```\n"), Vec::<String>::new());
        assert_eq!(covers("See [the exampel](http://a.test/pge)."), Vec::<String>::new());
        assert_eq!(covers("| Naem |\n| --- |\n| tpyo |\n"), Vec::<String>::new());
        assert_eq!(covers("Mail me@exampel.com or see exampel.com now."), Vec::<String>::new());
        assert_eq!(covers("The snake_case_naem and the h1 and utf8."), Vec::<String>::new());
        assert_eq!(covers("Read ~/notes/thnig now."), Vec::<String>::new());
    }

    /// Only the link itself is left alone; the sentence it stands in is prose like any other.
    #[test]
    fn keeps_the_prose_a_link_stands_in() {
        assert_eq!(covers("A tpyo beside [a link](http://a.test/pge)."), ["tpyo"]);
    }

    #[test]
    fn keeps_the_full_stop_that_ends_a_sentence() {
        assert_eq!(covers("A tpyo. Another sentence."), ["tpyo"]);
    }

    #[test]
    fn counts_positions_in_characters() {
        checker();
        // The emoji is one character, so the word after it starts at 2.
        let lints = check("🙂 recieve it.");
        assert_eq!(lints.len(), 1);
        assert_eq!((lints[0].at, lints[0].len), (2, 7));
    }

    /// Where a typo sits inside something the checker objects to as a whole, standing in
    /// the typo offers the typo: the narrower lint is the one that names those words.
    #[test]
    fn offers_the_narrowest_thing_the_cursor_is_standing_in() {
        checker();
        let text = "This is very unique writing.";
        let (whole, _) = under_cursor(text, 8).expect("the phrase is found");
        assert_eq!(whole, "very unique");
    }

    /// The name of the rule that objected comes back with the objection: it is what a
    /// writer who never wants to see that tip again turns off.
    #[test]
    fn says_which_rule_objected() {
        checker();
        let found = at("This is very unique writing.", 14).expect("the phrase is found");
        assert!(!found.rule.is_empty());
        assert!(has_rule(&found.rule), "{}", found.rule);
        // A misspelling is the dictionary's business and has no rule behind it.
        let typo = at("I recieve mail.", 4).expect("the typo is found");
        assert!(typo.rule.is_empty());
    }

    /// A rule the writer turned off says nothing, and the block it would have objected to
    /// comes back clean. Its own group, so that the shared checker is left as it was.
    #[test]
    fn keeps_quiet_about_a_check_the_writer_turned_off() {
        let heading = "# This is a title";
        let group = |checks: Checks| {
            let mut rules = LintGroup::new_curated(FstDictionary::curated(), Dialect::American);
            configure(&mut rules.config, &checks);
            Mutex::new(rules)
        };
        let objected = run(&group(Checks::new()), heading);
        assert_eq!(
            objected.iter().map(|lint| lint.rule.as_str()).collect::<Vec<_>>(),
            ["UseTitleCase"]
        );

        let turned_off = Checks::from([("UseTitleCase".to_string(), false)]);
        assert!(run(&group(turned_off), heading).is_empty());
    }

    /// What the writer said about a rule is what the checker runs it by, and the spell
    /// checker stays out whatever they said about it: this file does the spelling.
    #[test]
    fn runs_the_rules_the_writer_asked_for() {
        let mut config = FlatConfig::new_curated();
        let asked =
            Checks::from([("UseTitleCase".to_string(), false), ("SpellCheck".to_string(), true)]);
        configure(&mut config, &asked);
        assert!(!config.is_rule_enabled("UseTitleCase"));
        assert!(!config.is_rule_enabled("SpellCheck"));
    }

    /// The name in the writer's config is a name harper knows; one it does not know is
    /// what the config complains about rather than turning nothing off.
    ///
    /// Every rule the listing prints is one the config will take, and takes the same way
    /// a built group would. The two are asked of different things — [`has_rule`] reads
    /// the curated settings, because building a group in front of a waiting writer costs
    /// them better than half a second — and nothing but this says they agree.
    #[test]
    fn knows_the_rules_it_lets_a_writer_name() {
        assert!(has_rule("UseTitleCase"));
        assert!(!has_rule("UseTitleCse"));
        let listed = rules();
        assert!(listed.iter().any(|(name, _)| name == "UseTitleCase"), "{}", listed.len());
        assert!(listed.iter().all(|(_, description)| !description.is_empty()));

        let built = FlatConfig::new_curated();
        for (name, _) in &listed {
            assert!(has_rule(name), "the config would refuse {name}");
            assert_eq!(built.has_rule(name), has_rule(name), "{name}");
        }
    }

    #[test]
    fn says_nothing_where_there_is_nothing_wrong() {
        checker();
        assert!(at("This sentence is fine.", 3).is_none());
        assert!(covers("This sentence is fine.").is_empty());
    }
}
