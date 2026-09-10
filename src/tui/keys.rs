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
//! The bindings assume the kitty keyboard protocol, which foot, Alacritty and kitty all
//! answer: it is what tells Ctrl+I from Tab, Ctrl+Enter from Enter, Ctrl+Shift+B from
//! Ctrl+B, and Ctrl+1 from a typed 1. A terminal that does not answer it still runs, but
//! the shifted and numbered bindings fold onto their unshifted neighbours and are lost;
//! the probe says which terminal is which.

use crate::active::Step;
use crate::editor::Motion;
use crate::marks::{Align, Mark};
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
    /// Tab, which moves between the two halves of the search bar and types a tab
    /// everywhere else.
    Tab,
    Undo,
    Redo,
    Copy,
    Cut,
    /// Whatever the clipboard holds: words to type in, or a picture to write beside the
    /// document and name in the block.
    Paste,
    Surround(&'static str),
    /// An HTML tag pair either side of the selection: `<u>`, which is the only underline
    /// markdown has.
    Tag(&'static str),
    /// `[]()`, or `![]()` for an image.
    Link(&'static str),
    /// Follow the link the cursor is standing in, or start one where it is not: one key
    /// for the two things a writer wants to do with a link.
    OpenOrLink,
    /// A heading, a bullet, a number or a quote at the head of the lines being stood on.
    Mark(Mark),
    /// The block in or out of a fenced code block.
    Fence,
    Rule,
    Table,
    Align(Align),
    Save,
    Quit,
    OpenSearch,
    ToggleGrammar,
    ToggleReading,
    CloseSearch,
    CycleSearch(Step),
    ReplaceFound,
    ReplaceAll,
    AcceptLint,
    CycleLint(Step),
    Learn,
    MuteCheck,
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

/// Every command a key can be put on, in the order `markatui -keymap` lists them.
pub const COMMANDS: &[Command] = &[
    Command { section: "Document", name: "save", default: "ctrl+s", action: Action::Save },
    Command { section: "Document", name: "quit", default: "ctrl+q", action: Action::Quit },
    Command { section: "Edit", name: "undo", default: "ctrl+z", action: Action::Undo },
    Command { section: "Edit", name: "redo", default: "ctrl+y", action: Action::Redo },
    Command { section: "Edit", name: "select_all", default: "ctrl+a", action: Action::SelectAll },
    Command { section: "Edit", name: "cut_selection", default: "ctrl+x", action: Action::Cut },
    Command { section: "Edit", name: "copy_selection", default: "ctrl+c", action: Action::Copy },
    // One paste for whatever the clipboard holds, words or picture, so there is no
    // second key to remember and no key that does nothing when the wrong thing is on it.
    // Ctrl+V is bound to it as well, below: the terminals that take that key for their
    // own paste never pass it on, so there is nothing to lose by having it here.
    Command { section: "Edit", name: "paste", default: "ctrl+p", action: Action::Paste },
    Command { section: "Find", name: "find", default: "ctrl+f", action: Action::OpenSearch },
    Command {
        section: "View",
        name: "toggle_grammar",
        default: "ctrl+g",
        action: Action::ToggleGrammar,
    },
    Command {
        section: "View",
        name: "toggle_reading",
        default: "ctrl+r",
        action: Action::ToggleReading,
    },
    Command {
        section: "Formatting",
        name: "bold_selection",
        default: "ctrl+b",
        action: Action::Surround("**"),
    },
    Command {
        section: "Formatting",
        name: "italic_selection",
        default: "ctrl+i",
        action: Action::Surround("*"),
    },
    // Ctrl+U is underline everywhere a writer has met it, and markdown has no underline
    // of its own: the one style here that is written as HTML. Strikethrough, which used
    // to have this key, is on the key the markdown editors give it.
    Command {
        section: "Formatting",
        name: "underline_selection",
        default: "ctrl+u",
        action: Action::Tag("u"),
    },
    Command {
        section: "Formatting",
        name: "strike_selection",
        default: "alt+s",
        action: Action::Surround("~~"),
    },
    Command {
        section: "Formatting",
        name: "inline_code",
        default: "ctrl+e",
        action: Action::Surround("`"),
    },
    Command {
        section: "Formatting",
        name: "link_selection",
        default: "ctrl+k",
        action: Action::OpenOrLink,
    },
    Command {
        section: "Formatting",
        name: "insert_image",
        default: "ctrl+shift+i",
        action: Action::Link("!"),
    },
    // A heading by its depth, which is how every editor that has the keys spells it, and
    // the paragraph key that takes whatever is there back off.
    Command {
        section: "Headings",
        name: "heading_1",
        default: "ctrl+1",
        action: Action::Mark(Mark::Heading(1)),
    },
    Command {
        section: "Headings",
        name: "heading_2",
        default: "ctrl+2",
        action: Action::Mark(Mark::Heading(2)),
    },
    Command {
        section: "Headings",
        name: "heading_3",
        default: "ctrl+3",
        action: Action::Mark(Mark::Heading(3)),
    },
    Command {
        section: "Headings",
        name: "heading_4",
        default: "ctrl+4",
        action: Action::Mark(Mark::Heading(4)),
    },
    Command {
        section: "Headings",
        name: "heading_5",
        default: "ctrl+5",
        action: Action::Mark(Mark::Heading(5)),
    },
    Command {
        section: "Headings",
        name: "heading_6",
        default: "ctrl+6",
        action: Action::Mark(Mark::Heading(6)),
    },
    Command {
        section: "Headings",
        name: "paragraph",
        default: "ctrl+0",
        action: Action::Mark(Mark::Heading(0)),
    },
    Command {
        section: "Blocks",
        name: "bullet_list",
        default: "ctrl+shift+b",
        action: Action::Mark(Mark::Bullet),
    },
    Command {
        section: "Blocks",
        name: "numbered_list",
        default: "ctrl+shift+n",
        action: Action::Mark(Mark::Numbered),
    },
    Command {
        section: "Blocks",
        name: "task_list",
        default: "ctrl+shift+x",
        action: Action::Mark(Mark::Task),
    },
    Command {
        section: "Blocks",
        name: "quote_block",
        default: "ctrl+shift+q",
        action: Action::Mark(Mark::Quote),
    },
    Command {
        section: "Blocks",
        name: "code_block",
        default: "ctrl+shift+k",
        action: Action::Fence,
    },
    Command { section: "Blocks", name: "horizontal_rule", default: "alt+r", action: Action::Rule },
    Command { section: "Blocks", name: "insert_table", default: "ctrl+t", action: Action::Table },
    // The word processors' three keys, and the only alignment markdown has any way of
    // writing down: which way a table's column reads.
    Command {
        section: "Align",
        name: "align_left",
        default: "ctrl+shift+l",
        action: Action::Align(Align::Left),
    },
    Command {
        section: "Align",
        name: "align_centre",
        default: "ctrl+shift+e",
        action: Action::Align(Align::Centre),
    },
    Command {
        section: "Align",
        name: "align_right",
        default: "ctrl+shift+r",
        action: Action::Align(Align::Right),
    },
    // Control walks the suggestions the checker offered rather than the text; where it
    // has offered none, the caller lets it move the cursor as it always did.
    Command {
        section: "Suggestions",
        name: "previous_suggestion",
        default: "ctrl+up",
        action: Action::CycleLint(-1),
    },
    Command {
        section: "Suggestions",
        name: "next_suggestion",
        default: "ctrl+down",
        action: Action::CycleLint(1),
    },
    Command {
        section: "Suggestions",
        name: "accept_suggestion",
        default: "ctrl+enter",
        action: Action::AcceptLint,
    },
    // The word is spelled the way the writer meant it, and the dictionary is the one
    // that is wrong. It keeps the word from here on.
    Command {
        section: "Suggestions",
        name: "learn_word",
        default: "ctrl+shift+enter",
        action: Action::Learn,
    },
    // The checker is right about the words and wrong about this writer. The rule that
    // objected goes into their config, and stops objecting for good.
    Command {
        section: "Suggestions",
        name: "mute_check",
        default: "alt+g",
        action: Action::MuteCheck,
    },
];

/// A keystroke as the config writes it: `ctrl+shift+l`, `f5`, `esc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    code: KeyCode,
    modifiers: KeyModifiers,
}

/// The modifiers a binding can name. The kitty protocol reports others — super, hyper,
/// meta — and a binding that names none of them should not be thrown by one arriving.
const NAMED: KeyModifiers =
    KeyModifiers::CONTROL.union(KeyModifiers::SHIFT).union(KeyModifiers::ALT);

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

/// Which key each command is on — where it is on one at all — and which of them the
/// config named for itself. Held in the order of [`COMMANDS`], so the three are read
/// together and none can name a command the others do not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    keys: Vec<Option<Binding>>,
    asked: Vec<bool>,
}

impl Default for Keymap {
    fn default() -> Self {
        let keys = COMMANDS
            .iter()
            .map(|command| {
                let unreadable =
                    || panic!("{} is on an unreadable key: {}", command.name, command.default);
                Some(Binding::parse(command.default).unwrap_or_else(unreadable))
            })
            .collect();
        Keymap { keys, asked: vec![false; COMMANDS.len()] }
    }
}

impl Keymap {
    /// Put the command the config calls `name` on `binding`, or, where there is none, on
    /// no key at all. Whether there is such a command is the answer, so that the config
    /// can say which line it was.
    ///
    /// A key another command holds only because it shipped that way is given up to this
    /// one: a writer who puts ctrl+u on the strikethrough means ctrl+u to strike, not to
    /// be told that the underline had it first. A key two lines of the config both name
    /// is a different thing and stays a clash.
    pub fn bind(&mut self, name: &str, binding: Option<Binding>) -> bool {
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
            // The one word this editor spells the British way that a writer may well
            // spell the other.
            "align_center" => "align_centre",
            "learn" => "learn_word",
            name => name,
        };
        let Some(at) = COMMANDS.iter().position(|command| command.name == canonical) else {
            return false;
        };
        for (key, asked) in self.keys.iter_mut().zip(&self.asked) {
            if !asked && *key == binding {
                *key = None;
            }
        }
        self.keys[at] = binding;
        self.asked[at] = true;
        true
    }

    /// What `key` commands, where it commands anything. The first binding that takes it
    /// wins, so a key the config has put two commands on does the earlier of them.
    pub fn command(&self, key: KeyEvent) -> Option<Action> {
        let at = self.keys.iter().position(|binding| binding.is_some_and(|on| on.matches(key)))?;
        Some(COMMANDS[at].action.clone())
    }

    pub fn bindings(
        &self,
    ) -> impl Iterator<Item = (&'static str, &'static str, Option<Binding>)> + '_ {
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

        // The key every writer reaches for to paste, alongside the one in the config. A
        // terminal that takes it for its own paste never passes it on, and what it sends
        // instead arrives as a paste event; there is nothing lost by answering it here.
        (KeyCode::Char('v'), true, _) => Action::Paste,

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
        (KeyCode::Tab, false, _) => Action::Tab,
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
        // Saying the word is typed: the bar goes away and the cursor is left on the
        // occurrence the search walked to.
        (KeyCode::Esc, _) => Action::CloseSearch,
        // Which is what Enter means in the half of the bar holding the word. In the
        // other half it is the swap being asked for, one occurrence at a time.
        (KeyCode::Enter, _) => Action::Enter,
        (KeyCode::Tab, _) => Action::Tab,
        (KeyCode::Char('a'), true) => Action::ReplaceAll,
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

/// What a keystroke means while the writer is being asked whether a check should go.
/// The same three answers as the quit prompt, so that a question at the foot of the
/// screen is always answered the same way.
pub fn muting(key: KeyEvent) -> Action {
    if key.kind == KeyEventKind::Release {
        return Action::Nothing;
    }
    match key.code {
        KeyCode::Char('y' | 'Y') | KeyCode::Enter => Action::MuteCheck,
        KeyCode::Char('n' | 'N') | KeyCode::Esc => Action::Cancel,
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
        assert_eq!(control(KeyCode::Char('e')), Action::Surround("`"));
        // Ctrl+U is underline, the way it is in a word processor, and markdown writes
        // that as HTML. Strikethrough has the key the markdown editors give it.
        assert_eq!(control(KeyCode::Char('u')), Action::Tag("u"));
        assert_eq!(editing(press(KeyCode::Char('s'), KeyModifiers::ALT)), Action::Surround("~~"));
        assert_eq!(control(KeyCode::Char('k')), Action::OpenOrLink);
        let both = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(editing(press(KeyCode::Char('I'), both)), Action::Link("!"));
    }

    #[test]
    fn marks_the_blocks_from_the_shifted_and_numbered_keys() {
        assert_eq!(control(KeyCode::Char('1')), Action::Mark(Mark::Heading(1)));
        assert_eq!(control(KeyCode::Char('0')), Action::Mark(Mark::Heading(0)));
        assert_eq!(control(KeyCode::Char('t')), Action::Table);
        let both = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(editing(press(KeyCode::Char('B'), both)), Action::Mark(Mark::Bullet));
        assert_eq!(editing(press(KeyCode::Char('N'), both)), Action::Mark(Mark::Numbered));
        assert_eq!(editing(press(KeyCode::Char('X'), both)), Action::Mark(Mark::Task));
        assert_eq!(editing(press(KeyCode::Char('Q'), both)), Action::Mark(Mark::Quote));
        assert_eq!(editing(press(KeyCode::Char('K'), both)), Action::Fence);
        assert_eq!(editing(press(KeyCode::Char('r'), KeyModifiers::ALT)), Action::Rule);
        // The shifted keys are the unshifted ones' neighbours, and must not be them.
        assert_eq!(control(KeyCode::Char('b')), Action::Surround("**"));
        assert_eq!(control(KeyCode::Char('q')), Action::Quit);
    }

    #[test]
    fn aligns_a_column_the_way_a_word_processor_does() {
        let both = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(editing(press(KeyCode::Char('L'), both)), Action::Align(Align::Left));
        assert_eq!(editing(press(KeyCode::Char('E'), both)), Action::Align(Align::Centre));
        assert_eq!(editing(press(KeyCode::Char('R'), both)), Action::Align(Align::Right));
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

    /// Turning a check off is asked about first, and the question takes the same three
    /// answers the quit prompt does.
    #[test]
    fn asks_before_turning_a_check_off() {
        assert_eq!(editing(press(KeyCode::Char('g'), KeyModifiers::ALT)), Action::MuteCheck);
        assert_eq!(muting(press(KeyCode::Char('y'), KeyModifiers::NONE)), Action::MuteCheck);
        assert_eq!(muting(press(KeyCode::Enter, KeyModifiers::NONE)), Action::MuteCheck);
        assert_eq!(muting(press(KeyCode::Char('n'), KeyModifiers::NONE)), Action::Cancel);
        assert_eq!(muting(press(KeyCode::Esc, KeyModifiers::NONE)), Action::Cancel);
    }

    #[test]
    fn pastes_whatever_the_clipboard_holds_on_one_key() {
        assert_eq!(control(KeyCode::Char('c')), Action::Copy);
        assert_eq!(control(KeyCode::Char('x')), Action::Cut);
        assert_eq!(control(KeyCode::Char('p')), Action::Paste);
        // And the key a writer reaches for first, where the terminal passes it on.
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
        assert_eq!(
            searching(press(KeyCode::Char('x'), KeyModifiers::NONE)),
            Action::Type("x".into())
        );
        assert_eq!(searching(press(KeyCode::Esc, KeyModifiers::NONE)), Action::CloseSearch);
        // Enter and Tab belong to the two halves of the bar: what they do depends on
        // which of them the writer is typing into.
        assert_eq!(searching(press(KeyCode::Enter, KeyModifiers::NONE)), Action::Enter);
        assert_eq!(searching(press(KeyCode::Tab, KeyModifiers::NONE)), Action::Tab);
        assert_eq!(searching(press(KeyCode::Char('a'), KeyModifiers::CONTROL)), Action::ReplaceAll);
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
