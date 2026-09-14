//! A keystroke as the config writes it, and whether a keystroke that arrived is that one.
//!
//! Nothing here knows what any key commands: this is the spelling alone, read both ways,
//! so that a map written out is a map that reads back.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fmt;

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
    pub(super) fn matches(&self, key: KeyEvent) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
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
}
