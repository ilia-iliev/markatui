//! Every command a key can be put on, and which key each one is on. The table is the
//! config's to move: it keeps each name, default key, section, and action in one place,
//! so the command list, config file, and `keymap show` cannot drift apart.

use crate::marks::{Align, Mark};
use crate::tui::keys::{Action, Binding};
use crossterm::event::KeyEvent;

/// Every command a key can be put on. Keeping its display section here means the command
/// list, config file, and `keymap show` cannot drift apart.
pub struct Command {
    pub section: &'static str,
    pub name: &'static str,
    default: &'static str,
    action: Action,
}

/// Every command a key can be put on, in the order `markatui -keymap` lists them.
const COMMANDS: &[Command] = &[
    Command { section: "Document", name: "save", default: "ctrl+s", action: Action::Save },
    Command { section: "Document", name: "quit", default: "ctrl+q", action: Action::Quit },
    Command { section: "Edit", name: "undo", default: "ctrl+z", action: Action::Undo },
    Command { section: "Edit", name: "redo", default: "ctrl+y", action: Action::Redo },
    Command { section: "Edit", name: "select_all", default: "ctrl+a", action: Action::SelectAll },
    Command { section: "Edit", name: "cut_selection", default: "ctrl+x", action: Action::Cut },
    Command { section: "Edit", name: "copy_selection", default: "ctrl+c", action: Action::Copy },
    // One paste for whatever the clipboard holds, words or picture, so there is no
    // second key to remember and no key that does nothing when the wrong thing is on it.
    // Ctrl+V is what a writer presses; a terminal that takes that key for its own paste
    // never passes it on, and what it sends instead arrives as a paste event anyway.
    Command { section: "Edit", name: "paste", default: "ctrl+v", action: Action::Paste },
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
    // Alt with the arrows, the way every editor that moves a line about spells it.
    Command {
        section: "Blocks",
        name: "move_section_up",
        default: "alt+up",
        action: Action::MoveSection(-1),
    },
    Command {
        section: "Blocks",
        name: "move_section_down",
        default: "alt+down",
        action: Action::MoveSection(1),
    },
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
    // Control walks the suggestions the checker offered rather than the blocks; where the
    // checker is off, the caller hands the pair back to the document.
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
