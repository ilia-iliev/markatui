//! The checks the writer has turned off, kept in `config.toml` under `[checks]`:
//!
//! ```toml
//! [checks]
//! UseTitleCase = false
//! ```
//!
//! A name here is one of the checker's own, which `markatui -checks` prints beside the one
//! line saying what each looks for. Turning one off in the editor — which asks first, the
//! file being the writer's own — writes the same line, so that the two ways of doing it
//! leave the same file.

use crate::lint;
use crate::storage;
use crate::tui::config;

use std::fs;
use std::io;

const TABLE: &str = "[checks]";

/// Every check there is, with the ones the writer has turned off marked. Sorted by name,
/// because the name is what they will be looking for it under.
pub fn show() -> Result<String, String> {
    let problems = config::load();
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    let checks = &config::get().checks;
    let listed = lint::rules();
    let width = listed.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    let lines = listed.into_iter().map(|(name, description)| {
        let off = match checks.get(&name) {
            Some(false) => "  [off]",
            _ => "",
        };
        format!("{name:<width$}  {description}{off}")
    });
    Ok(lines.collect::<Vec<String>>().join("\n"))
}

/// Turn a check on or off for good. The line goes into the writer's own config, where
/// they can change it again by hand; nothing else in the file is touched.
pub fn set(name: &str, on: bool) -> Result<(), String> {
    if !lint::has_rule(name) {
        return Err(format!("there is no check called {name:?}"));
    }
    let path = config::path()?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        // No config yet is the ordinary case, and this is the first line of one.
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    storage::write_atomic(&path, spoken_for(&text, name, on).as_bytes())
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// `text` with `name` turned on or off in it. A line that already spoke for this check is
/// the one rewritten — putting a second one in would leave the file saying both
/// things — and otherwise the setting goes under `[checks]`, opening the table if there
/// is none.
fn spoken_for(text: &str, name: &str, on: bool) -> String {
    let setting = format!("{name} = {on}");
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut header = None;
    let mut inside = false;
    for (at, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == TABLE;
            header = header.or(inside.then_some(at));
            continue;
        }
        if inside && trimmed.split('=').next().is_some_and(|key| key.trim() == name) {
            lines[at] = setting;
            return joined(lines);
        }
    }
    match header {
        Some(at) => lines.insert(at + 1, setting),
        None => {
            if lines.last().is_some_and(|line| !line.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.extend([TABLE.to_string(), setting]);
        }
    }
    joined(lines)
}

fn joined(lines: Vec<String>) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turning a check off is what this is nearly always for; the tests read better for
    /// saying so once.
    fn muted(text: &str, name: &str) -> String {
        spoken_for(text, name, false)
    }

    #[test]
    fn opens_the_table_where_there_is_none() {
        assert_eq!(muted("", "UseTitleCase"), "[checks]\nUseTitleCase = false\n");
        assert_eq!(
            muted("content_width = 80\n", "UseTitleCase"),
            "content_width = 80\n\n[checks]\nUseTitleCase = false\n"
        );
    }

    /// The setting goes into the table it belongs to, not at the end of a file whose last
    /// table is somebody else's.
    #[test]
    fn puts_the_setting_in_the_table_it_belongs_to() {
        let text = "[checks]\nSpelledNumbers = false\n\n[palette]\naccent = \"#112233\"\n";
        assert_eq!(
            muted(text, "UseTitleCase"),
            "[checks]\nUseTitleCase = false\nSpelledNumbers = false\n\n[palette]\naccent = \"#112233\"\n"
        );
    }

    #[test]
    fn rewrites_a_check_the_file_already_spoke_for() {
        assert_eq!(
            muted("[checks]\nUseTitleCase = true\n", "UseTitleCase"),
            "[checks]\nUseTitleCase = false\n"
        );
        // A key of the same name outside the table is nothing to do with this check.
        assert_eq!(
            muted("UseTitleCase = true\n", "UseTitleCase"),
            "UseTitleCase = true\n\n[checks]\nUseTitleCase = false\n"
        );
    }

    #[test]
    fn puts_a_check_back_on_where_it_was_turned_off() {
        assert_eq!(
            spoken_for("[checks]\nUseTitleCase = false\n", "UseTitleCase", true),
            "[checks]\nUseTitleCase = true\n"
        );
    }

    #[test]
    fn refuses_a_check_the_checker_does_not_have() {
        assert!(set("NoSuchRule", false).is_err());
    }
}
