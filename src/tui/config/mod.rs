//! The config file, `~/.config/markatui/config.toml`, and what it can say:
//!
//! ```toml
//! content_width = 72
//! theme = "terminal"
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

use crate::tui::theme::{Palette, Theme};
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
    /// Whether the terminal preset leaves the terminal's own ground and ink visible.
    /// Kept separately for compatibility with older config files.
    pub inherit_background: bool,
    pub palette: Palette,
    pub keys: Keymap,
    /// The checker's rules the writer has spoken for, on or off. What is not named here
    /// stays as the checker ships it.
    pub checks: lint::Checks,
}

impl Default for Config {
    fn default() -> Self {
        let (inherit_background, palette) = Theme::Terminal.settings();
        Config {
            content_width: 72,
            inherit_background,
            palette,
            keys: Keymap::default(),
            checks: lint::Checks::new(),
        }
    }
}

impl Config {
    fn apply(&mut self, theme: Theme) {
        (self.inherit_background, self.palette) = theme.settings();
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

/// Select a preset for future runs and leave the rest of the config untouched.
pub fn set_theme(name: &str) -> Result<(), String> {
    let theme = Theme::parse(name).ok_or_else(|| {
        format!("there is no theme called {name:?}; choose light, dark, or terminal")
    })?;
    let path = path()?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    storage::write_atomic(&path, theme_in(&text, theme).as_bytes())
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn theme_in(text: &str, theme: Theme) -> String {
    let setting = format!("theme = \"{}\"", theme.name());
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut in_root = true;
    let mut found = false;
    lines.retain_mut(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_root = false;
        }
        if !in_root {
            return true;
        }
        let key = trimmed.split('=').next().map(str::trim);
        if key == Some("theme") {
            if !found {
                *line = setting.clone();
                found = true;
                return true;
            }
            return false;
        }
        // This older setting would otherwise undo a selected preset later in the file.
        key != Some("inherit_background")
    });
    if !found {
        lines.insert(0, setting);
    }
    let mut result = lines.join("\n");
    result.push('\n');
    result
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
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
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
            "theme" => config.apply(theme(value)?),
            // Kept for existing config files. A named theme is clearer for new ones.
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

fn theme(value: &str) -> Result<Theme, String> {
    let name = string(value)?;
    Theme::parse(name)
        .ok_or_else(|| format!("{name:?} is not a theme; choose light, dark, or terminal"))
}

fn boolean(value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{value} is not true or false")),
    }
}

fn width(value: &str) -> Result<u16, String> {
    let width: u16 = value
        .parse()
        .map_err(|_| format!("{value} is not a number"))?;
    match WIDTHS.contains(&width) {
        true => Ok(width),
        false => Err(format!(
            "{width} columns is outside {}–{}",
            WIDTHS.start(),
            WIDTHS.end()
        )),
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
    fn every_display_setting_takes_effect() {
        use crate::layout::Cell;
        use crate::style;
        use crate::tui::{theme, view};
        use ratatui::layout::Rect;
        use ratatui::style::Modifier;

        let config = read_well(
            r##"
            content_width = 100   # room for a wide screen
            inherit_background = false

            [palette]
            accent = "#010101"
            muted = "#020202"
            lint = "#030303"
            lint_ink = "#040404"
            code = "#050505"
            prompt = "#060606"
            prompt_ink = "#070707"
            paper = "#080808"
            ink = "#090909"
            "##,
        );
        let colour = |channel| Color::Rgb(channel, channel, channel);
        let cell = |bits| Cell {
            text: "x".to_string(),
            width: 1,
            bits,
            source: Some(0),
        };

        assert_eq!(theme::content_width_for(&config), 100);
        assert_eq!(
            view::column_for(Rect::new(10, 0, 200, 1), config.content_width),
            Rect::new(60, 0, 100, 1)
        );

        let base = theme::base_for(&config);
        assert_eq!(base.bg, Some(colour(8)));
        assert_eq!(base.fg, Some(colour(9)));
        let mut inherited = config.clone();
        inherited.inherit_background = true;
        assert_eq!(
            theme::base_for(&inherited),
            ratatui::style::Style::default()
        );

        let heading = theme::of_for(&config, &cell(style::HEADING), 0);
        assert_eq!(heading.fg, Some(colour(1)));
        assert!(heading.add_modifier.contains(Modifier::BOLD));
        let link = theme::of_for(&config, &cell(style::LINK), 0);
        assert_eq!(link.fg, Some(colour(1)));
        assert!(link.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(
            theme::of_for(&config, &cell(style::MARKER), 0).fg,
            Some(colour(2))
        );

        let lint = theme::of_for(&config, &cell(style::LINT), 0);
        assert_eq!(lint.bg, Some(colour(3)));
        assert_eq!(lint.fg, Some(colour(4)));
        assert_eq!(
            theme::of_for(&config, &cell(0), style::CODE).bg,
            Some(colour(5))
        );

        let prompt = theme::prompt_for(&config);
        assert_eq!(prompt.bg, Some(colour(6)));
        assert_eq!(prompt.fg, Some(colour(7)));
    }

    #[test]
    fn themes_are_presets_and_palette_lines_can_adjust_them() {
        let light = read_well("theme = \"light\"\n");
        let dark = read_well("theme = \"dark\"\n");
        let terminal = read_well("theme = \"terminal\"\n");

        assert!(!light.inherit_background);
        assert!(!dark.inherit_background);
        assert!(terminal.inherit_background);
        assert_ne!(light.palette.paper, dark.palette.paper);
        assert_ne!(light.palette.ink, dark.palette.ink);

        let adjusted = read_well("theme = \"dark\"\n[palette]\naccent = \"#010203\"\n");
        assert_eq!(adjusted.palette.accent, Color::Rgb(1, 2, 3));
        assert_eq!(adjusted.palette.paper, dark.palette.paper);
    }

    #[test]
    fn writes_a_theme_at_the_root_and_removes_the_old_background_switch() {
        let text =
            "content_width = 80\ninherit_background = true\n\n[palette]\naccent = \"#010203\"\n";
        assert_eq!(
            theme_in(text, Theme::Dark),
            "theme = \"dark\"\ncontent_width = 80\n\n[palette]\naccent = \"#010203\"\n"
        );
        assert_eq!(
            theme_in("theme = \"light\"\n", Theme::Terminal),
            "theme = \"terminal\"\n"
        );
    }

    #[test]
    fn refuses_an_unknown_theme() {
        let (_, problems) = read("theme = \"midnight\"\n");
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("light, dark, or terminal"));
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
        assert!(
            problems[1].contains("inherit_backgrund"),
            "{:?}",
            problems[1]
        );
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
