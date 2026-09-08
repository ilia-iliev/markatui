//! The config file, `~/.config/markatui/config.toml`, and what it can say:
//!
//! ```toml
//! content_width = 72
//! inherit_background = true
//!
//! [palette]
//! accent = "#3E8E62"
//!
//! [checks]
//! UseTitleCase = false
//! ```
//!
//! What is read is a config file, not TOML: one table, `key = value`, and three kinds of
//! value — a string in quotes, a whole number, and `true` or `false`. Anything else is a
//! line the editor says it could not read rather than one it guesses at. The names of the
//! colours are [`Palette`]'s, and the names of the checks are [`checks`]'s. Keys live
//! separately in `keymap.toml`.
//!
//! There is one config for a run, read before the first frame: the writer is not editing
//! it in the window it would change.

pub mod checks;
pub mod keymap;
mod lines;

use crate::lint;
use crate::storage;
use crate::tui::keys::Keymap;

use crate::tui::theme::Palette;
use ratatui::style::Color;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Narrower than this is not a column of prose but a margin, and a number that wide is a
/// typo rather than a measure.
const WIDTHS: std::ops::RangeInclusive<u16> = 20..=500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub content_width: u16,
    /// Whether the terminal's own ground shows through. Painting paper of ours over a
    /// dark terminal jars, so the default is to leave the writer the palette they chose.
    pub inherit_background: bool,
    pub palette: Palette,
    pub keys: Keymap,
    /// The checker's rules the writer has spoken for, on or off. What is not named here
    /// stays as the checker ships it.
    pub checks: lint::Checks,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            content_width: 72,
            inherit_background: true,
            palette: Palette::default(),
            keys: Keymap::default(),
            checks: lint::Checks::new(),
        }
    }
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// The writer's own config, ready to be written to: the directory it lives in is made if
/// this is the first thing they have set. The file itself need not be there — an empty
/// one is what the defaults above say in longhand.
pub fn path() -> Result<PathBuf, String> {
    let path = storage::config_file().ok_or("HOME is not set")?;
    let directory = path.parent().expect("the config has a config directory");
    fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    Ok(path)
}

/// Read the config and keep it for the rest of the run. What comes back is what could not
/// be read, a line at a time, for the editor to put at the foot of the screen: a colour
/// spelled wrong should be said out loud, not silently left at its default.
pub fn load() -> Vec<String> {
    let mut config = Config::default();
    let mut problems = Vec::new();
    if let Some(path) = storage::config_file() {
        match fs::read_to_string(&path) {
            Ok(text) => (config, problems) = read(&text),
            // Having no config is the ordinary case, and the defaults are the answer to it.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => problems.push(format!("{}: {error}", path.display())),
        }
    }
    let (keys, key_problems) = keymap::load();
    config.keys = keys;
    problems.extend(key_problems);
    let _ = CONFIG.set(config);
    problems
}

/// The config this run is drawn with. Nothing has to have been loaded: a test, and the
/// example that prints a document, get the defaults.
pub fn get() -> &'static Config {
    CONFIG.get_or_init(Config::default)
}

fn read(text: &str) -> (Config, Vec<String>) {
    let mut config = Config::default();
    let mut table = String::new();
    let problems = lines::walk(storage::CONFIG_FILE, text, |line| {
        if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            table = name.trim().to_string();
            if !matches!(table.as_str(), "palette" | "checks") {
                return Err(format!("there is no [{table}] to put anything in"));
            }
            return Ok(());
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("{line:?} is not `key = value`"));
        };
        set(&mut config, &table, key.trim(), value.trim())
    });
    (config, problems)
}

fn set(config: &mut Config, table: &str, key: &str, value: &str) -> Result<(), String> {
    match table {
        "" => match key {
            "content_width" => config.content_width = width(value)?,
            "inherit_background" => config.inherit_background = boolean(value)?,
            _ => return Err(unknown(key)),
        },
        "palette" => *config.palette.slot(key).ok_or_else(|| unknown(key))? = colour(value)?,
        // Naming a check the checker does not have is worth saying out loud: the writer
        // meant to turn something off, and silence would leave it on.
        "checks" if !lint::has_rule(key) => {
            return Err(format!("there is no check called {key:?}"));
        }
        "checks" => {
            config.checks.insert(key.to_string(), boolean(value)?);
        }
        // The table itself was complained about at the line that opened it; saying so
        // again for every setting in it would bury the one line that matters.
        _ => {}
    }
    Ok(())
}

fn unknown(key: &str) -> String {
    format!("nothing is called {key:?}")
}

fn string(value: &str) -> Result<&str, String> {
    lines::quoted(value).ok_or_else(|| format!("{value} is not in quotes"))
}

fn boolean(value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{value} is not true or false")),
    }
}

fn width(value: &str) -> Result<u16, String> {
    let width: u16 = value.parse().map_err(|_| format!("{value} is not a number"))?;
    match WIDTHS.contains(&width) {
        true => Ok(width),
        false => Err(format!("{width} columns is outside {}–{}", WIDTHS.start(), WIDTHS.end())),
    }
}

/// `"#RRGGBB"`. Only the one way of writing a colour: the terminal's sixteen are the
/// writer's own and already show through everywhere nothing is painted.
fn colour(value: &str) -> Result<Color, String> {
    let text = string(value)?;
    let digits = text.strip_prefix('#').filter(|digits| digits.len() == 6);
    let channels = digits.and_then(|digits| {
        let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
        Some((channel(0)?, channel(2)?, channel(4)?))
    });
    match channels {
        Some((red, green, blue)) => Ok(Color::Rgb(red, green, blue)),
        None => Err(format!("{text:?} is not a colour like \"#3E8E62\"")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_well(text: &str) -> Config {
        let (config, problems) = read(text);
        assert!(problems.is_empty(), "{problems:?}");
        config
    }

    #[test]
    fn an_empty_file_is_the_defaults() {
        assert_eq!(read_well(""), Config::default());
        assert_eq!(read_well("# nothing but a comment\n"), Config::default());
    }

    #[test]
    fn reads_the_settings_a_writer_would_change() {
        let config = read_well(
            r##"
            content_width = 100   # room for a wide screen
            inherit_background = false

            [palette]
            accent = "#112233"
            "##,
        );
        assert_eq!(config.content_width, 100);
        assert!(!config.inherit_background);
        assert_eq!(config.palette.accent, Color::Rgb(0x11, 0x22, 0x33));
        // What was not spoken for is still what it was.
        assert_eq!(config.palette.muted, Palette::default().muted);
    }

    #[test]
    fn says_what_it_could_not_read_and_keeps_the_rest() {
        let (config, problems) = read(
            r##"
            content_width = wide
            inherit_backgrund = false
            [colours]
            accent = "green"
            [palette]
            muted = "#000000"
            "##,
        );
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].contains("line 2"), "{:?}", problems[0]);
        assert!(problems[1].contains("inherit_backgrund"), "{:?}", problems[1]);
        assert!(problems[2].contains("[colours]"), "{:?}", problems[2]);
        // The line after the ones it could not read is read all the same.
        assert_eq!(config.palette.muted, Color::Rgb(0, 0, 0));
        assert_eq!(config.content_width, Config::default().content_width);
    }

    /// A check named here is off for the run; one the checker has never heard of is a
    /// line to complain about, not one to act on.
    #[test]
    fn reads_the_checks_a_writer_turned_off() {
        let config = read_well("[checks]\nUseTitleCase = false\n");
        assert_eq!(config.checks.get("UseTitleCase"), Some(&false));

        let (config, problems) = read("[checks]\nUseTitleCse = false\n");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("UseTitleCse"), "{:?}", problems[0]);
        assert!(config.checks.is_empty());
    }

    #[test]
    fn refuses_a_width_that_is_not_a_column() {
        assert!(width("0").is_err());
        assert!(width("100000").is_err());
        assert_eq!(width("72"), Ok(72));
    }
}
