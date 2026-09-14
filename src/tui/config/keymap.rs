//! The editable keymap at `~/.config/markatui/keymap.toml`.

use super::syntax;
use crate::storage;
use crate::tui::keys::{Binding, Keymap};

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DEFAULT: &str = include_str!("../../../assets/keymap.toml");

pub fn path() -> io::Result<PathBuf> {
    storage::keymap_file().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

/// Install the shipped keymap when this user does not have one yet.
fn install() -> io::Result<PathBuf> {
    let path = path()?;
    if !path.exists() {
        write_shipped(&path)?;
    }
    Ok(path)
}

/// Put the keys back to the ones that shipped, and say which file that was. The file is
/// written rather than removed: it is the writer's to read and edit, and a reset that
/// leaves them nothing there until the next start is not the one they asked for.
pub fn reset() -> Result<PathBuf, String> {
    let written = path().and_then(|path| write_shipped(&path).map(|()| path));
    written.map_err(|error| error.to_string())
}

fn write_shipped(path: &Path) -> io::Result<()> {
    let directory = path.parent().expect("the keymap has a config directory");
    fs::create_dir_all(directory)?;
    storage::write_atomic(path, DEFAULT.as_bytes())
}

/// Read the user's map, with anything wrong with it to say out loud: a bad line, and a
/// key two commands have landed on.
pub fn load() -> (Keymap, Vec<String>) {
    let (keymap, mut problems) = read_file();
    problems.extend(clashes(&keymap));
    (keymap, problems)
}

/// The same, minus the clashes — which is what rebinding reads, because rebinding is how
/// a writer gets out of one.
fn read_file() -> (Keymap, Vec<String>) {
    let path = match install() {
        Ok(path) => path,
        Err(error) => {
            return (Keymap::default(), vec![format!("{}: {error}", storage::KEYMAP_FILE)]);
        }
    };
    match fs::read_to_string(&path) {
        Ok(text) => read(&text),
        Err(error) => (Keymap::default(), vec![format!("{}: {error}", path.display())]),
    }
}

/// The whole map for a writer to read. A file with a bad line in it is an error and
/// nothing else; a key two commands have landed on is a map that still works, so it is
/// listed and the clash is said underneath, where a long list has not scrolled it away.
pub fn show() -> Result<String, String> {
    let (keymap, problems) = read_file();
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    let listing = format_for_display(&keymap);
    match clashes(&keymap) {
        clashes if clashes.is_empty() => Ok(listing),
        clashes => Ok(format!("{listing}\n\n{}", clashes.join("\n"))),
    }
}

/// The keys no config can move, and so no config lists. A writer looking up what a key
/// does should find every one of them in the same place, and a writer who has bound
/// themselves into a corner should find the way out here.
const FIXED: &str = "\
Always
  quit                 esc, ctrl+d
  move                 arrows, home, end, pageup, pagedown
  by word              ctrl+left, ctrl+right
  to the ends          ctrl+home, ctrl+end
  select               shift with any of them

In the find bar
  move between fields  tab
  replace this one     enter
  replace them all     ctrl+a
  next, previous       ctrl+down, ctrl+up
  close                esc";

/// A key written down: `absent` where the config has moved it off this command and on to
/// another, which the file and the listing say in their own ways.
fn spell(binding: Option<Binding>, absent: &str) -> String {
    binding.map_or_else(|| absent.to_string(), |binding| binding.to_string())
}

/// A line of the map as it is written down: the head of a section, or one command under
/// it.
enum Row {
    Section(&'static str),
    Binding(&'static str, Option<Binding>),
}

/// The map in the sections it is read in, a head before each. The listing and the file are
/// both written out of this walk: a file the writer has edited and a file the editor has
/// rewritten should be the same file.
fn by_section(keymap: &Keymap) -> impl Iterator<Item = Row> + '_ {
    let mut previous_section = None;
    keymap.bindings().flat_map(move |(section, name, binding)| {
        let head = (previous_section != Some(section)).then_some(Row::Section(section));
        previous_section = Some(section);
        head.into_iter().chain([Row::Binding(name, binding)])
    })
}

fn format_for_display(keymap: &Keymap) -> String {
    let width = keymap.bindings().map(|(_, name, _)| name.len()).max().unwrap_or(0);
    let mut lines = Vec::new();
    for row in by_section(keymap) {
        match row {
            // A blank line between the sections, but not above the first of them.
            Row::Section(section) => {
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                lines.push(section.to_string());
            }
            Row::Binding(name, binding) => {
                lines.push(format!("  {name:<width$}  {}", spell(binding, "no key")));
            }
        }
    }
    lines.push(String::new());
    lines.push(FIXED.to_string());
    lines.join("\n")
}

pub fn set(command: &str, key: &str) -> Result<(), String> {
    let binding = Binding::parse(key).ok_or_else(|| format!("{key:?} is not a key"))?;
    let (mut keymap, problems) = read_file();
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    if !keymap.bind(command, Some(binding)) {
        return Err(format!("there is no keymap command called {command:?}"));
    }
    let path = path().map_err(|error| error.to_string())?;
    storage::write_atomic(&path, render(&keymap).as_bytes()).map_err(|error| error.to_string())
}

fn read(text: &str) -> (Keymap, Vec<String>) {
    let mut keymap = Keymap::default();
    let problems = syntax::walk(storage::KEYMAP_FILE, text, |line| {
        let Some((command, value)) = line.split_once('=') else {
            return Err(format!("{line:?} is not `command = \"key\"`"));
        };
        let (command, value) = (command.trim(), value.trim());
        let Some(text) = syntax::quoted(value) else {
            return Err(format!("{value} is not a key in quotes"));
        };
        // Nothing between the quotes is a command with no key on it, which is how a key
        // the config has moved elsewhere is written back down.
        let binding = match text.is_empty() {
            true => None,
            false => match Binding::parse(text) {
                Some(binding) => Some(binding),
                None => return Err(format!("{value} is not a key in quotes")),
            },
        };
        match keymap.bind(command, binding) {
            true => Ok(()),
            false => Err(format!("there is no keymap command called {command:?}")),
        }
    });
    (keymap, problems)
}

/// Two commands the config has put on one key. The first of them answers it and the
/// second never does, which is worth saying out loud — in the fewest words that say it, since the foot of
/// the screen is where it lands: a writer who has moved a key rarely means to have taken
/// another one away.
fn clashes(keymap: &Keymap) -> Vec<String> {
    let bindings: Vec<_> = keymap.bindings().filter(|(.., key)| key.is_some()).collect();
    let mut problems = Vec::new();
    for (at, (_, name, binding)) in bindings.iter().enumerate() {
        let Some((_, first, _)) = bindings[..at].iter().find(|(_, _, other)| other == binding)
        else {
            continue;
        };
        problems.push(format!("{} goes to {first}, not {name}", spell(*binding, "")));
    }
    problems
}

/// The map written back out, in the sections it is read in: a file the writer has edited
/// and a file the editor has rewritten should be the same file.
fn render(keymap: &Keymap) -> String {
    let mut text = String::from(
        "# markatui keymap. Change this file or run `markatui -keymap <command> <key>`.\n",
    );
    for row in by_section(keymap) {
        match row {
            Row::Section(section) => text.push_str(&format!("\n# {section}\n")),
            Row::Binding(name, binding) => {
                text.push_str(&format!("{name} = \"{}\"\n", spell(binding, "")));
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_file_is_the_default_map() {
        let (keymap, problems) = read(DEFAULT);
        assert!(problems.is_empty(), "{problems:?}");
        let keys: Vec<_> = keymap.bindings().collect();
        assert_eq!(keys, Keymap::default().bindings().collect::<Vec<_>>());
    }

    #[test]
    fn an_edited_binding_changes_what_the_key_does() {
        use crate::tui::keys::Action;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let (keymap, problems) = read("find = \"alt+f\"\n");
        assert!(problems.is_empty(), "{problems:?}");
        assert!(render(&keymap).contains("find = \"alt+f\""));
        assert_eq!(
            keymap.command(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT)),
            Some(Action::OpenSearch)
        );
        assert_ne!(
            keymap.command(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)),
            Some(Action::OpenSearch)
        );
    }

    #[test]
    fn reads_names_from_an_existing_keymap() {
        let (keymap, problems) = read("bold = \"alt+b\"\nalign_center = \"alt+c\"\n");
        assert!(problems.is_empty(), "{problems:?}");
        let text = render(&keymap);
        assert!(text.contains("bold_selection = \"alt+b\""));
        assert!(text.contains("align_centre = \"alt+c\""));
    }

    #[test]
    fn show_has_aligned_sections() {
        let text = format_for_display(&Keymap::default());
        assert!(text.contains("Find\n  find                 ctrl+f"), "{text}");
        assert!(text.contains("Formatting\n  bold_selection       ctrl+b"), "{text}");
        assert!(text.contains("  strike_selection     alt+s"), "{text}");
        assert!(text.contains("Blocks\n  bullet_list          ctrl+shift+b"), "{text}");
        assert!(text.contains("  move_section_up      alt+up"), "{text}");
        assert!(text.contains("Align\n  align_left           ctrl+shift+l"), "{text}");
        assert!(text.contains("Suggestions\n  previous_suggestion  ctrl+up"), "{text}");
    }

    /// The keys the config cannot move are listed all the same: a writer looking one up
    /// should not have to find out that this list is only some of them.
    #[test]
    fn show_lists_the_keys_no_config_can_move() {
        let text = format_for_display(&Keymap::default());
        assert!(text.contains("quit                 esc, ctrl+d"), "{text}");
        assert!(text.contains("replace them all     ctrl+a"), "{text}");
    }

    /// A key the config asks for is the config's, whatever shipped on it. The command
    /// that shipped on it is left with no key rather than with the argument.
    #[test]
    fn a_key_the_config_asks_for_is_taken_off_the_shipped_command() {
        use crate::tui::keys::Action;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let (keymap, problems) = read("strike_selection = \"ctrl+u\"\n");
        assert!(problems.is_empty(), "{problems:?}");
        assert!(clashes(&keymap).is_empty(), "{:?}", clashes(&keymap));
        assert_eq!(
            keymap.command(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            Some(Action::Surround("~~"))
        );
        assert!(
            keymap.bindings().any(|(_, name, key)| name == "underline_selection" && key.is_none()),
            "the underline is left on no key"
        );
    }

    /// Two lines of the config on one key is another matter: the writer wrote both, and
    /// the second of them never answers.
    #[test]
    fn says_when_two_commands_land_on_one_key() {
        let text = "strike_selection = \"ctrl+u\"\nunderline_selection = \"ctrl+u\"\n";
        let (keymap, _) = read(text);
        let problems = clashes(&keymap);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("ctrl+u goes to underline_selection"), "{problems:?}");
        assert!(problems[0].contains("not strike_selection"), "{problems:?}");
    }

    /// A map written out and read back is the same map, the keys the config moved off a
    /// command included.
    #[test]
    fn a_displaced_key_survives_being_written_back_out() {
        let (keymap, _) = read("strike_selection = \"ctrl+u\"\n");
        let (again, problems) = read(&render(&keymap));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(again.bindings().collect::<Vec<_>>(), keymap.bindings().collect::<Vec<_>>());
    }

    #[test]
    fn reports_bad_lines() {
        let (_, problems) = read("replace = \"ctrl+f\"\nsearch ctrl+f\n");
        assert_eq!(problems.len(), 2);
        assert!(problems[0].contains("replace"));
        assert!(problems[1].contains("line 2"));
    }
}
