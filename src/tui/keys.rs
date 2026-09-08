//! The keymap: a keystroke turned into what the writer meant by it, and nothing else.
//! Nothing here touches the document, so the whole map can be pressed in a test.
//!
//! The keys are the Qt front end's, key for key, and the commands among them are the
//! config's to move: [`COMMANDS`] keeps each name, default key, section, and action in
//! one place. Moving in the document — the arrows, Home and End, a page at a
//! time — is not bindable, because there is nothing to argue about in it. Nor are Esc and
//! Ctrl+D, which always leave: a writer who cannot find the way out of an editor is stuck
//! in it, and no config should be able to arrange that.
//!
//! Where the kitty protocol is not answered, Ctrl+I arrives as Tab and Ctrl+Enter as
//! Enter, and italics and accepting a suggestion are lost with them; the probe says which
//! terminal is which.

use crate::active::Step;
use crate::editor::Motion;
use crate::tui::config;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Type(String),
    /// A movement the editor can make on its own.
    Move(Motion, Extend),
    /// Up or down, which crosses wrapped rows and so needs the layout to resolve.
    Row(Step, Extend),
    Page(Step),
    SelectAll,
    Delete(Step),
    Enter,
    Undo,
    Copy,
    Paste,
    Surround(&'static str),
    /// `[]()`, or `![]()` for an image.
    Link(&'static str),
    Save,
    Quit,
    OpenSearch,
    ToggleGrammar,
    ToggleReading,
    CloseSearch,
    CycleSearch(Step),
    AcceptLint,
    CycleLint(Step),
    Learn,
    /// The quit prompt's three answers.
    SaveAndQuit,
    DiscardAndQuit,
    Cancel,
}

/// Whether the keystroke carries a selection along with it.
pub type Extend = bool;

/// Every command a key can be put on. Keeping its display section here means the command
/// list, config file, and `keymap show` cannot drift apart.
pub struct Command {
    pub section: &'static str,
    pub name: &'static str,
    default: &'static str,
    action: Action,
}

pub const COMMANDS: [Command; 18] = [
    Command { section: "Document", name: "save", default: "ctrl+s", action: Action::Save },
    Command { section: "Document", name: "quit", default: "ctrl+q", action: Action::Quit },
    Command { section: "Edit", name: "undo", default: "ctrl+z", action: Action::Undo },
    Command { section: "Edit", name: "select_all", default: "ctrl+a", action: Action::SelectAll },
    Command { section: "Edit", name: "copy_selection", default: "ctrl+c", action: Action::Copy },
    Command { section: "Edit", name: "paste", default: "ctrl+v", action: Action::Paste },
    Command { section: "Find", name: "find", default: "ctrl+f", action: Action::OpenSearch },
    Command { section: "View", name: "toggle_grammar", default: "ctrl+g", action: Action::ToggleGrammar },
    Command { section: "View", name: "toggle_reading", default: "ctrl+r", action: Action::ToggleReading },
    Command { section: "Formatting", name: "bold_selection", default: "ctrl+b", action: Action::Surround("**") },
    Command { section: "Formatting", name: "italic_selection", default: "ctrl+i", action: Action::Surround("*") },
    Command { section: "Formatting", name: "strike_selection", default: "ctrl+u", action: Action::Surround("~~") },
    Command { section: "Formatting", name: "link_selection", default: "ctrl+shift+l", action: Action::Link("") },
    Command { section: "Formatting", name: "insert_image", default: "ctrl+shift+i", action: Action::Link("!") },
    // Control walks the suggestions the checker offered rather than the text; where it
    // has offered none, the caller lets it move the cursor as it always did.
    Command { section: "Suggestions", name: "previous_suggestion", default: "ctrl+up", action: Action::CycleLint(-1) },
    Command { section: "Suggestions", name: "next_suggestion", default: "ctrl+down", action: Action::CycleLint(1) },
    Command { section: "Suggestions", name: "accept_suggestion", default: "ctrl+enter", action: Action::AcceptLint },
    // The word is spelled the way the writer meant it, and the dictionary is the one
    // that is wrong. It keeps the word from here on.
    Command { section: "Suggestions", name: "learn_word", default: "ctrl+shift+enter", action: Action::Learn },
];

/// A keystroke as the config writes it: `ctrl+shift+l`, `f5`, `esc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    code: KeyCode,
    modifiers: KeyModifiers,
}

/// The modifiers a binding can name. The kitty protocol reports others — super, hyper,
/// meta — and a binding that names none of them should not be thrown by one arriving.
const NAMED: KeyModifiers = KeyModifiers::CONTROL
    .union(KeyModifiers::SHIFT)
    .union(KeyModifiers::ALT);

/// The keys that have a name of their own, and the names the config writes them by. Read
/// both ways — [`Binding::parse`] looks a name up here and [`Display`] looks a key
/// up — so that a map written out is a map that reads back. A letter needs no entry: it
/// is written as itself.
const NAMED_KEYS: [(&str, KeyCode); 15] = [
    ("enter", KeyCode::Enter),
    ("tab", KeyCode::Tab),
    ("esc", KeyCode::Esc),
    ("space", KeyCode::Char(' ')),
    ("backspace", KeyCode::Backspace),
    ("delete", KeyCode::Delete),
    ("insert", KeyCode::Insert),
    ("up", KeyCode::Up),
    ("down", KeyCode::Down),
    ("left", KeyCode::Left),
    ("right", KeyCode::Right),
    ("home", KeyCode::Home),
    ("end", KeyCode::End),
    ("pageup", KeyCode::PageUp),
    ("pagedown", KeyCode::PageDown),
];

impl fmt::Display for Binding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (modifier, name) in [
            (KeyModifiers::CONTROL, "ctrl+"),
            (KeyModifiers::SHIFT, "shift+"),
            (KeyModifiers::ALT, "alt+"),
        ] {
            if self.modifiers.contains(modifier) {
                write!(formatter, "{name}")?;
            }
        }
        if let Some((name, _)) = NAMED_KEYS.iter().find(|(_, code)| *code == self.code) {
            return write!(formatter, "{name}");
        }
        match self.code {
            KeyCode::Char(character) => write!(formatter, "{character}"),
            KeyCode::F(number) => write!(formatter, "f{number}"),
            _ => Err(fmt::Error),
        }
    }
}

impl Binding {
    pub fn parse(text: &str) -> Option<Binding> {
        let text = text.trim();
        let (front, name) = match text.strip_suffix("++") {
            // The key itself is a plus, as in `ctrl++`.
            Some(front) => (front, "+"),
            None if text == "+" => ("", "+"),
            // A trailing plus with nothing after it is a binding with no key in it.
            None if text.ends_with('+') => return None,
            None => text.rsplit_once('+').unwrap_or(("", text)),
        };
        let mut modifiers = KeyModifiers::NONE;
        for word in front.split('+').filter(|word| !word.is_empty()) {
            modifiers |= match word {
                "ctrl" => KeyModifiers::CONTROL,
                "shift" => KeyModifiers::SHIFT,
                "alt" => KeyModifiers::ALT,
                _ => return None,
            };
        }
        Some(Binding { code: code(name)?, modifiers })
    }

    /// Whether `key` is this binding. A letter is compared in lower case: the terminal
    /// reports Ctrl+Shift+L as an upper case L carrying shift, and the two halves of that
    /// say the same thing twice.
    fn matches(&self, key: KeyEvent) -> bool {
        let code = match key.code {
            KeyCode::Char(character) => KeyCode::Char(character.to_ascii_lowercase()),
            code => code,
        };
        code == self.code && key.modifiers & NAMED == self.modifiers
    }
}

/// The key a binding names, by the name the config uses for it.
fn code(name: &str) -> Option<KeyCode> {
    let mut characters = name.chars();
    if let (Some(character), None) = (characters.next(), characters.next()) {
        return Some(KeyCode::Char(character.to_ascii_lowercase()));
    }
    if let Some((_, code)) = NAMED_KEYS.iter().find(|(known, _)| *known == name) {
        return Some(*code);
    }
    Some(KeyCode::F(name.strip_prefix('f')?.parse().ok()?))
}

/// Which key each command is on. Held in the order of [`COMMANDS`], so the two are read
/// together and neither can name a command the other does not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    keys: Vec<Binding>,
}

impl Default for Keymap {
    fn default() -> Self {
        let keys = COMMANDS
            .iter()
            .map(|command| {
                let unreadable = || {
                    panic!("{} is on an unreadable key: {}", command.name, command.default)
                };
                Binding::parse(command.default).unwrap_or_else(unreadable)
            })
            .collect();
        Keymap { keys }
    }
}

impl Keymap {
    /// Put the command the config calls `name` on `binding`. Whether there is such a
    /// command is the answer, so that the config can say which line it was.
    pub fn bind(&mut self, name: &str, binding: Binding) -> bool {
        let canonical = match name {
            // Names from keymaps installed before commands were made explicit. Reading
            // them keeps upgrades working; rendered maps always use the clearer names.
            "copy" => "copy_selection",
            "search" => "find",
            "grammar" => "toggle_grammar",
            "reading" => "toggle_reading",
            "bold" => "bold_selection",
            "italic" => "italic_selection",
            "strikethrough" => "strike_selection",
            "link" => "link_selection",
            "image" => "insert_image",
            "accept" => "accept_suggestion",
            "learn" => "learn_word",
            name => name,
        };
        let Some(at) = COMMANDS.iter().position(|command| command.name == canonical) else {
            return false;
        };
        self.keys[at] = binding;
        true
    }

    /// What `key` commands, where it commands anything. The first binding that takes it
    /// wins, so a key the config has put two commands on does the earlier of them.
    pub fn command(&self, key: KeyEvent) -> Option<Action> {
        let at = self.keys.iter().position(|binding| binding.matches(key))?;
        Some(COMMANDS[at].action.clone())
    }

    pub fn bindings(&self) -> impl Iterator<Item = (&'static str, &'static str, Binding)> + '_ {
        COMMANDS
            .iter()
            .zip(&self.keys)
            .map(|(command, binding)| (command.section, command.name, *binding))
    }
}

/// What a keystroke means while the writer is in the document.
pub fn editing(key: KeyEvent) -> Action {
    if key.kind == KeyEventKind::Release {
        return Action::Nothing;
    }
    if let Some(action) = config::get().keys.command(key) {
        return action;
    }
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match (key.code, control, shift) {
        // The two ways out that are there whatever the config has done with the quit
        // key: the one every terminal program takes, and the one every writer tries
        // first. Both go through the same prompt when there is unsaved work.
        (KeyCode::Esc, false, _) => Action::Quit,
        (KeyCode::Char('d'), true, _) => Action::Quit,

        (KeyCode::Left, true, _) => Action::Move(Motion::Word(-1), shift),
        (KeyCode::Right, true, _) => Action::Move(Motion::Word(1), shift),
        (KeyCode::Home, true, _) => Action::Move(Motion::Document(-1), shift),
        (KeyCode::End, true, _) => Action::Move(Motion::Document(1), shift),
        (KeyCode::Left, false, _) => Action::Move(Motion::Character(-1), shift),
        (KeyCode::Right, false, _) => Action::Move(Motion::Character(1), shift),
        (KeyCode::Home, false, _) => Action::Move(Motion::LineEdge(-1), shift),
        (KeyCode::End, false, _) => Action::Move(Motion::LineEdge(1), shift),
        (KeyCode::Up, false, _) => Action::Row(-1, shift),
        (KeyCode::Down, false, _) => Action::Row(1, shift),
        (KeyCode::PageUp, _, _) => Action::Page(-1),
        (KeyCode::PageDown, _, _) => Action::Page(1),

        (KeyCode::Backspace, false, _) => Action::Delete(-1),
        (KeyCode::Delete, false, _) => Action::Delete(1),
        (KeyCode::Enter, false, _) => Action::Enter,
        (KeyCode::Tab, false, _) => Action::Type("\t".into()),
        (KeyCode::Char(character), false, _) => Action::Type(character.to_string()),
        _ => Action::Nothing,
    }
}

/// What a keystroke means while the search bar holds the keyboard. The bar is where the
/// writer is still typing the word they are looking for, so most keys are its own.
pub fn searching(key: KeyEvent) -> Action {
    if key.kind == KeyEventKind::Release {
        return Action::Nothing;
    }
    // The key that opened the bar is the third way of closing it, wherever it is.
    if config::get().keys.command(key) == Some(Action::OpenSearch) {
        return Action::CloseSearch;
    }
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match (key.code, control) {
        // Either way of saying the word is typed: the bar goes away and the cursor is
        // left on the occurrence the search walked to.
        (KeyCode::Esc | KeyCode::Enter, _) => Action::CloseSearch,
        (KeyCode::Up, true) => Action::CycleSearch(-1),
        (KeyCode::Down, true) => Action::CycleSearch(1),
        (KeyCode::Backspace, false) => Action::Delete(-1),
        // Undo is the document's. Taken here so that the bar does not answer it by
        // unwinding the word typed into it, which is not something the writer wrote.
        (KeyCode::Char(_), true) => Action::Nothing,
        (KeyCode::Char(character), false) => Action::Type(character.to_string()),
        _ => Action::Nothing,
    }
}

/// What a keystroke means while the quit prompt is up. It is answered by keystroke, so
/// it takes every one of them.
pub fn quitting(key: KeyEvent) -> Action {
    if key.kind == KeyEventKind::Release {
        return Action::Nothing;
    }
    match key.code {
        KeyCode::Char('y' | 'Y') | KeyCode::Enter => Action::SaveAndQuit,
        KeyCode::Char('n' | 'N') => Action::DiscardAndQuit,
        KeyCode::Esc => Action::Cancel,
        _ => Action::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn plain(code: KeyCode) -> Action {
        editing(press(code, KeyModifiers::NONE))
    }

    fn control(code: KeyCode) -> Action {
        editing(press(code, KeyModifiers::CONTROL))
    }

    #[test]
    fn reads_the_shapes_a_key_is_written_in() {
        let binding = |text| Binding::parse(text).expect(text);
        let key = |code, modifiers| Binding { code, modifiers };
        assert_eq!(binding("ctrl+s"), key(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert_eq!(binding("f5"), key(KeyCode::F(5), KeyModifiers::NONE));
        assert_eq!(binding("esc"), key(KeyCode::Esc, KeyModifiers::NONE));
        // A key that is itself a plus, which is what the splitting has to be careful of.
        assert_eq!(binding("ctrl++"), key(KeyCode::Char('+'), KeyModifiers::CONTROL));
        assert_eq!(binding("ctrl+shift+l").modifiers, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        assert_eq!(binding("+"), key(KeyCode::Char('+'), KeyModifiers::NONE));
        assert_eq!(Binding::parse("hyper+s"), None);
        // A key was meant to follow that plus.
        assert_eq!(Binding::parse("ctrl+"), None);
        assert_eq!(Binding::parse("pgdn"), None);
    }

    #[test]
    fn takes_a_letter_however_the_terminal_spells_it() {
        let binding = Binding::parse("ctrl+shift+l").expect("a binding");
        let both = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        // The terminal says shift twice: in the modifiers and in the case of the letter.
        assert!(binding.matches(press(KeyCode::Char('L'), both)));
        assert!(binding.matches(press(KeyCode::Char('l'), both)));
        // And a modifier no binding names is not one that stops it matching.
        assert!(binding.matches(press(KeyCode::Char('L'), both | KeyModifiers::META)));
        assert!(!binding.matches(press(KeyCode::Char('L'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn types_what_was_typed() {
        assert_eq!(plain(KeyCode::Char('x')), Action::Type("x".into()));
        assert_eq!(
            editing(press(KeyCode::Char('X'), KeyModifiers::SHIFT)),
            Action::Type("X".into())
        );
    }

    #[test]
    fn wraps_and_links_from_the_control_keys() {
        assert_eq!(control(KeyCode::Char('b')), Action::Surround("**"));
        assert_eq!(control(KeyCode::Char('i')), Action::Surround("*"));
        assert_eq!(control(KeyCode::Char('u')), Action::Surround("~~"));
        let both = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(editing(press(KeyCode::Char('L'), both)), Action::Link(""));
        assert_eq!(editing(press(KeyCode::Char('I'), both)), Action::Link("!"));
    }

    #[test]
    fn carries_a_selection_on_shift_and_not_otherwise() {
        assert_eq!(plain(KeyCode::Right), Action::Move(Motion::Character(1), false));
        assert_eq!(
            editing(press(KeyCode::Right, KeyModifiers::SHIFT)),
            Action::Move(Motion::Character(1), true)
        );
        assert_eq!(editing(press(KeyCode::Down, KeyModifiers::SHIFT)), Action::Row(1, true));
    }

    #[test]
    fn gives_control_with_the_arrows_to_the_checker() {
        assert_eq!(control(KeyCode::Up), Action::CycleLint(-1));
        assert_eq!(control(KeyCode::Enter), Action::AcceptLint);
        assert_eq!(
            editing(press(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::SHIFT)),
            Action::Learn
        );
    }

    #[test]
    fn toggles_grammar_and_reading_modes() {
        assert_eq!(control(KeyCode::Char('g')), Action::ToggleGrammar);
        assert_eq!(control(KeyCode::Char('r')), Action::ToggleReading);
    }

    #[test]
    fn copies_and_pastes_on_the_keys_every_other_editor_uses() {
        assert_eq!(control(KeyCode::Char('c')), Action::Copy);
        assert_eq!(control(KeyCode::Char('v')), Action::Paste);
    }

    #[test]
    fn always_has_a_way_out() {
        assert_eq!(plain(KeyCode::Esc), Action::Quit);
        assert_eq!(control(KeyCode::Char('d')), Action::Quit);
        // And the prompt that Quit puts up is not itself left by the same keys.
        assert_eq!(quitting(press(KeyCode::Esc, KeyModifiers::NONE)), Action::Cancel);
        assert_eq!(quitting(press(KeyCode::Char('d'), KeyModifiers::CONTROL)), Action::Nothing);
    }

    #[test]
    fn lets_a_key_release_alone() {
        let mut key = press(KeyCode::Char('x'), KeyModifiers::NONE);
        key.kind = KeyEventKind::Release;
        assert_eq!(editing(key), Action::Nothing);
        assert_eq!(searching(key), Action::Nothing);
        assert_eq!(quitting(key), Action::Nothing);
    }

    #[test]
    fn gives_the_search_bar_the_keys_while_it_is_open() {
        assert_eq!(searching(press(KeyCode::Char('x'), KeyModifiers::NONE)), Action::Type("x".into()));
        assert_eq!(searching(press(KeyCode::Enter, KeyModifiers::NONE)), Action::CloseSearch);
        assert_eq!(searching(press(KeyCode::Esc, KeyModifiers::NONE)), Action::CloseSearch);
        assert_eq!(searching(press(KeyCode::Down, KeyModifiers::CONTROL)), Action::CycleSearch(1));
        // Undo belongs to the document, not to the word being typed into the bar.
        assert_eq!(searching(press(KeyCode::Char('z'), KeyModifiers::CONTROL)), Action::Nothing);
    }

    #[test]
    fn answers_the_quit_prompt_three_ways() {
        assert_eq!(quitting(press(KeyCode::Char('y'), KeyModifiers::NONE)), Action::SaveAndQuit);
        assert_eq!(quitting(press(KeyCode::Char('n'), KeyModifiers::NONE)), Action::DiscardAndQuit);
        assert_eq!(quitting(press(KeyCode::Esc, KeyModifiers::NONE)), Action::Cancel);
    }
}
