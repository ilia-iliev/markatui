//! The keymap: a keystroke turned into what the writer meant by it, and nothing else.
//! Nothing here touches the document, so the whole map can be pressed in a test.
//!
//! The keys are the Qt front end's, key for key. Where the kitty protocol is not
//! answered, Ctrl+I arrives as Tab and Ctrl+Enter as Enter, and italics and accepting a
//! suggestion are lost with them; the probe says which terminal is which.

use crate::active::Step;
use crate::editor::Motion;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

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
    Surround(&'static str),
    /// `[]()`, or `![]()` for an image.
    Link(&'static str),
    Save,
    Quit,
    OpenSearch,
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

/// What a keystroke means while the writer is in the document.
pub fn editing(key: KeyEvent) -> Action {
    if key.kind == KeyEventKind::Release {
        return Action::Nothing;
    }
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match (key.code, control, shift) {
        (KeyCode::Char('s'), true, false) => Action::Save,
        (KeyCode::Char('q'), true, false) => Action::Quit,
        (KeyCode::Char('z'), true, false) => Action::Undo,
        (KeyCode::Char('a'), true, false) => Action::SelectAll,
        (KeyCode::Char('c'), true, false) => Action::Copy,
        (KeyCode::Char('f'), true, false) => Action::OpenSearch,
        (KeyCode::Char('b'), true, false) => Action::Surround("**"),
        (KeyCode::Char('i'), true, false) => Action::Surround("*"),
        (KeyCode::Char('u'), true, false) => Action::Surround("~~"),
        (KeyCode::Char('i' | 'I'), true, true) => Action::Link("!"),
        (KeyCode::Char('l' | 'L'), true, true) => Action::Link(""),

        // Control walks the suggestions the checker offered rather than the text; where
        // it has offered none, the caller lets it move the cursor as it always did.
        (KeyCode::Up, true, false) => Action::CycleLint(-1),
        (KeyCode::Down, true, false) => Action::CycleLint(1),
        (KeyCode::Enter, true, false) => Action::AcceptLint,
        // The word is spelled the way the writer meant it, and the dictionary is the one
        // that is wrong. It keeps the word from here on.
        (KeyCode::Enter, true, true) => Action::Learn,

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
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match (key.code, control) {
        // Either way of saying the word is typed: the bar goes away and the cursor is
        // left on the occurrence the search walked to.
        (KeyCode::Esc | KeyCode::Enter, _) | (KeyCode::Char('f'), true) => Action::CloseSearch,
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
