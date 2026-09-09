//! The editable keymap at `~/.config/markatui/keymap.toml`.

use super::lines;
use crate::storage;
use crate::tui::keys::{Binding, Keymap};

use std::fs;
use std::io;
use std::path::PathBuf;

const DEFAULT: &str = include_str!("../../../assets/keymap.toml");

pub fn path() -> io::Result<PathBuf> {
    storage::keymap_file().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

/// Install the shipped keymap when this user does not have one yet.
pub fn install() -> io::Result<PathBuf> {
    let path = path()?;
    if !path.exists() {
        let directory = path.parent().expect("the keymap has a config directory");
        fs::create_dir_all(directory)?;
        storage::write_atomic(&path, DEFAULT.as_bytes())?;
    }
    Ok(path)
}

/// Read the user's map. Good bindings still apply when another line is bad.
pub fn load() -> (Keymap, Vec<String>) {
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

pub fn show() -> Result<String, String> {
    let (keymap, problems) = load();
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    Ok(format_for_display(&keymap))
}

fn format_for_display(keymap: &Keymap) -> String {
    let width = keymap.bindings().map(|(_, name, _)| name.len()).max().unwrap_or(0);
    let mut lines = Vec::new();
    let mut previous_section = None;
    for (section, name, binding) in keymap.bindings() {
        if previous_section != Some(section) {
            if previous_section.is_some() {
                lines.push(String::new());
            }
            lines.push(section.to_string());
            previous_section = Some(section);
        }
        lines.push(format!("  {name:<width$}  {binding}"));
    }
    lines.join("\n")
}

pub fn set(command: &str, key: &str) -> Result<(), String> {
    let binding = Binding::parse(key).ok_or_else(|| format!("{key:?} is not a key"))?;
    let (mut keymap, problems) = load();
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    if !keymap.bind(command, binding) {
        return Err(format!("there is no keymap command called {command:?}"));
    }
    let path = path().map_err(|error| error.to_string())?;
    storage::write_atomic(&path, render(&keymap).as_bytes()).map_err(|error| error.to_string())
}

fn read(text: &str) -> (Keymap, Vec<String>) {
    let mut keymap = Keymap::default();
    let problems = lines::walk(storage::KEYMAP_FILE, text, |line| {
        let Some((command, value)) = line.split_once('=') else {
            return Err(format!("{line:?} is not `command = \"key\"`"));
        };
        let (command, value) = (command.trim(), value.trim());
        let Some(binding) = lines::quoted(value).and_then(Binding::parse) else {
            return Err(format!("{value} is not a key in quotes"));
        };
        match keymap.bind(command, binding) {
            true => Ok(()),
            false => Err(format!("there is no keymap command called {command:?}")),
        }
    });
    (keymap, problems)
}

fn render(keymap: &Keymap) -> String {
    let mut text = String::from(
        "# markatui keymap. Change this file or run `markatui keymap <command> <key>`.\n",
    );
    for (_, name, binding) in keymap.bindings() {
        text.push_str(&format!("{name} = \"{binding}\"\n"));
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
        assert_eq!(keymap, Keymap::default());
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
        let (keymap, problems) = read("bold = \"alt+b\"\nstrikethrough = \"alt+s\"\n");
        assert!(problems.is_empty(), "{problems:?}");
        let text = render(&keymap);
        assert!(text.contains("bold_selection = \"alt+b\""));
        assert!(text.contains("strike_selection = \"alt+s\""));
    }

    #[test]
    fn show_has_aligned_sections() {
        let text = format_for_display(&Keymap::default());
        assert!(text.contains("Find\n  find                 ctrl+f"), "{text}");
        assert!(text.contains("Formatting\n  bold_selection       ctrl+b"), "{text}");
        assert!(text.contains("  strike_selection     ctrl+u"), "{text}");
        assert!(text.contains("Suggestions\n  previous_suggestion  ctrl+up"), "{text}");
    }

    #[test]
    fn reports_bad_lines() {
        let (_, problems) = read("replace = \"ctrl+f\"\nsearch ctrl+f\n");
        assert_eq!(problems.len(), 2);
        assert!(problems[0].contains("replace"));
        assert!(problems[1].contains("line 2"));
    }
}
