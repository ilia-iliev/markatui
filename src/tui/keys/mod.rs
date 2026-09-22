//! The keymap: a keystroke turned into what the writer meant by it, and nothing else.
//! Nothing here touches the document, so the whole map can be pressed in a test.
//!
//! The keys are the Qt front end's, key for key, and the commands among them are the
//! config's to move: the commands table keeps each name, default key, section, and
//! action in one place. Moving in the document — the arrows, Home and End, a page at a
//! time — is not bindable, because there is nothing to argue about in it. Nor are Esc and
//! Ctrl+D, which always leave: a writer who cannot find the way out of an editor is stuck
//! in it, and no config should be able to arrange that.
//!
//! The bindings assume the kitty keyboard protocol, which foot, Alacritty and kitty all
//! answer: it is what tells Ctrl+I from Tab, Ctrl+Enter from Enter, Ctrl+Shift+B from
//! Ctrl+B, and Ctrl+1 from a typed 1. A terminal that does not answer it still runs, but
//! the shifted and numbered bindings fold onto their unshifted neighbours and are lost;
//! the probe says which terminal is which.

mod binding;
mod commands;

pub use binding::Binding;
pub use commands::Keymap;

use crate::active::Step;
use crate::editor::Motion;
use crate::marks::{Align, Mark};
use crate::tui::config;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Type(String),
    /// A movement the editor can make on its own.
    Move(Motion, Extend),
    /// Up or down, which crosses wrapped rows and so needs the layout to resolve.
    Row(Step, Extend),
    /// A screenful at a time, window and caret together.
    Page(Step, Extend),
    SelectAll,
    Delete(Step),
    /// Ctrl with Backspace and Delete: the same, as far as Ctrl with the arrows walks.
    DeleteWord(Step),
    Enter,
    /// Shift+Enter: a line break inside the block, wherever Enter has something else to
    /// do — a list item, a table row, the end of a heading. In a paragraph Enter leaves
    /// the same break, and the press after it is what ends the block.
    LineBreak,
    /// Tab, which moves between the two halves of the search bar, and in the document
    /// takes a list item in or out a level, walks a table's cells, or types a tab.
    Tab(Step),
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
    /// The section under the cursor one place up or down: the list item, the table row,
    /// the line of code, or the whole block where the lines cannot move among themselves.
    MoveSection(Step),
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
type Extend = bool;

/// What a keystroke means while the writer is in the document.
pub fn editing(key: KeyEvent) -> Action {
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

        // The key the config had this on before Ctrl+V became the default, kept so that
        // the hands of anyone who learned it are not wrong.
        (KeyCode::Char('p'), true, _) => Action::Paste,

        (KeyCode::Left, true, _) => Action::Move(Motion::Word(-1), shift),
        (KeyCode::Right, true, _) => Action::Move(Motion::Word(1), shift),
        // Control with Up and Down walks the blocks the way control with Left and Right
        // walks the words. Unshifted the pair is the checker's while it is on, which is
        // what the bound command above answers; shifted it is the document's throughout,
        // so that a selection can be drawn a block at a time either way.
        (KeyCode::Up, true, _) => Action::Move(Motion::Block(-1), shift),
        (KeyCode::Down, true, _) => Action::Move(Motion::Block(1), shift),
        (KeyCode::Home, true, _) => Action::Move(Motion::Document(-1), shift),
        (KeyCode::End, true, _) => Action::Move(Motion::Document(1), shift),
        (KeyCode::Left, false, _) => Action::Move(Motion::Character(-1), shift),
        (KeyCode::Right, false, _) => Action::Move(Motion::Character(1), shift),
        (KeyCode::Home, false, _) => Action::Move(Motion::LineEdge(-1), shift),
        (KeyCode::End, false, _) => Action::Move(Motion::LineEdge(1), shift),
        (KeyCode::Up, false, _) => Action::Row(-1, shift),
        (KeyCode::Down, false, _) => Action::Row(1, shift),
        (KeyCode::PageUp, _, _) => Action::Page(-1, shift),
        (KeyCode::PageDown, _, _) => Action::Page(1, shift),

        (KeyCode::Backspace, true, _) => Action::DeleteWord(-1),
        (KeyCode::Delete, true, _) => Action::DeleteWord(1),
        (KeyCode::Backspace, false, _) => Action::Delete(-1),
        (KeyCode::Delete, false, _) => Action::Delete(1),
        (KeyCode::Enter, false, true) => Action::LineBreak,
        (KeyCode::Enter, false, _) => Action::Enter,
        (KeyCode::Tab, false, _) => Action::Tab(1),
        (KeyCode::BackTab, _, _) => Action::Tab(-1),
        (KeyCode::Char(character), false, _) => Action::Type(character.to_string()),
        _ => Action::Nothing,
    }
}

/// What a keystroke means while the search bar holds the keyboard. The bar is where the
/// writer is still typing the word they are looking for, so most keys are its own.
pub fn searching(key: KeyEvent) -> Action {
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
        (KeyCode::Tab, _) => Action::Tab(1),
        (KeyCode::BackTab, _) => Action::Tab(-1),
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

/// The keys every question at the foot of the screen is answered by, as the foot of the
/// screen says them. [`prompt`] is what answers them, so the two are read together.
pub const ANSWERS: &str = "[y] [n] [esc]";

/// What a keystroke means while a question is up at the foot of the screen: `yes` or
/// `no` as the question spells them, and Esc backing out of it. Every question takes the
/// same three answers, so that one at the foot of the screen is always answered the same
/// way.
pub fn prompt(key: KeyEvent, yes: Action, no: Action) -> Action {
    match key.code {
        KeyCode::Char('y' | 'Y') | KeyCode::Enter => yes,
        KeyCode::Char('n' | 'N') => no,
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

    fn quitting(key: KeyEvent) -> Action {
        prompt(key, Action::SaveAndQuit, Action::DiscardAndQuit)
    }

    fn muting(key: KeyEvent) -> Action {
        prompt(key, Action::MuteCheck, Action::Cancel)
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

    /// Alt with the arrows moves the section rather than the cursor, which the plain
    /// arrows keep.
    #[test]
    fn moves_a_section_on_alt_with_the_arrows() {
        assert_eq!(editing(press(KeyCode::Up, KeyModifiers::ALT)), Action::MoveSection(-1));
        assert_eq!(editing(press(KeyCode::Down, KeyModifiers::ALT)), Action::MoveSection(1));
        assert_eq!(plain(KeyCode::Up), Action::Row(-1, false));
        assert_eq!(control(KeyCode::Up), Action::CycleLint(-1));
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

    /// Enter ends the block and Shift+Enter breaks the line inside it, which only a
    /// terminal answering the kitty protocol can tell apart. Tab and Shift+Tab are the
    /// same key either way.
    #[test]
    fn tells_shift_enter_from_enter_and_shift_tab_from_tab() {
        assert_eq!(editing(press(KeyCode::Enter, KeyModifiers::NONE)), Action::Enter);
        assert_eq!(editing(press(KeyCode::Enter, KeyModifiers::SHIFT)), Action::LineBreak);
        assert_eq!(editing(press(KeyCode::Tab, KeyModifiers::NONE)), Action::Tab(1));
        assert_eq!(editing(press(KeyCode::BackTab, KeyModifiers::SHIFT)), Action::Tab(-1));
    }

    /// Control with Up and Down walks the blocks the way control with Left and Right
    /// walks the words. The checker takes the unshifted pair while it is on, and hands
    /// them back to the document when it is not; shift is the document's either way.
    #[test]
    fn walks_the_blocks_on_control_with_the_up_and_down_arrows() {
        let both = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(editing(press(KeyCode::Up, both)), Action::Move(Motion::Block(-1), true));
        assert_eq!(editing(press(KeyCode::Down, both)), Action::Move(Motion::Block(1), true));
        assert_eq!(control(KeyCode::Left), Action::Move(Motion::Word(-1), false));
    }

    /// Ctrl with Backspace and Delete takes out a word, the way Ctrl with Left and
    /// Right walks one.
    #[test]
    fn deletes_a_word_on_control_with_the_delete_keys() {
        assert_eq!(control(KeyCode::Backspace), Action::DeleteWord(-1));
        assert_eq!(control(KeyCode::Delete), Action::DeleteWord(1));
        assert_eq!(plain(KeyCode::Backspace), Action::Delete(-1));
        assert_eq!(plain(KeyCode::Delete), Action::Delete(1));
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
        // The key the default map has it on now, and the one it had before.
        assert_eq!(control(KeyCode::Char('v')), Action::Paste);
        assert_eq!(control(KeyCode::Char('p')), Action::Paste);
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
    fn gives_the_search_bar_the_keys_while_it_is_open() {
        assert_eq!(
            searching(press(KeyCode::Char('x'), KeyModifiers::NONE)),
            Action::Type("x".into())
        );
        assert_eq!(searching(press(KeyCode::Esc, KeyModifiers::NONE)), Action::CloseSearch);
        // Enter and Tab belong to the two halves of the bar: what they do depends on
        // which of them the writer is typing into.
        assert_eq!(searching(press(KeyCode::Enter, KeyModifiers::NONE)), Action::Enter);
        assert_eq!(searching(press(KeyCode::Tab, KeyModifiers::NONE)), Action::Tab(1));
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
