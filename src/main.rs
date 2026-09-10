use markatui::tui;
use std::path::{Path, PathBuf};

const HELP: &str = "Usage:
  markatui <file.md>
  markatui -keymap
  markatui -keymap <command> <key>
  markatui -keymap default
  markatui -checks
  markatui -checks on|off <check>
  markatui theme light|dark|terminal
  markatui -config
  markatui -config default

Available options:
  -h, --help  Print help
  -keymap     List the keys, bind one to a command, or put them all back
  -checks     List the writing checks, or turn one on or off
  -theme      Set light, dark, or terminal colours
  -config     Edit the config file, or put it back to its defaults";

fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(mut first) = arguments.next() else {
        usage();
    };

    if first == "--" {
        let Some(path) = arguments.next() else {
            usage();
        };
        first = path;
    } else if first == "theme" {
        return report(theme(arguments));
    } else if let Some(flag) = flag(&first) {
        match flag {
            "h" | "help" => {
                println!("{HELP}");
                return;
            }
            "checks" => return report(checks(arguments)),
            "keymap" => return report(keymap(arguments)),
            "theme" => return report(theme(arguments)),
            "config" => return config(arguments),
            _ => invalid_option(&first),
        }
    }

    if arguments.next().is_some() {
        usage();
    }
    open(&PathBuf::from(first));
}

/// The name of a flag, one dash or two, or nothing if the argument is not one.
fn flag(argument: &str) -> Option<&str> {
    let name = argument.strip_prefix("--").or_else(|| argument.strip_prefix('-'))?;
    (!name.is_empty()).then_some(name)
}

fn checks(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    match (arguments.next(), arguments.next(), arguments.next()) {
        (None, None, None) => tui::config::checks::show().map(|text| println!("{text}")),
        (Some(state), Some(check), None) if state == "on" || state == "off" => {
            tui::config::checks::set(&check, state == "on")
        }
        _ => usage(),
    }
}

/// The writer's own config, opened in the editor like any other file. It is theirs to
/// edit, and what it says is read at the next start, not this one. `default` puts it back
/// to the settings it had before they set any, which is said out loud: nothing else is
/// removing a file they wrote.
fn config(mut arguments: impl Iterator<Item = String>) {
    match (arguments.next(), arguments.next()) {
        (None, None) => match tui::config::path() {
            Ok(path) => open(&path),
            Err(error) => fail(&error),
        },
        (Some(word), None) if word == "default" => match tui::config::reset() {
            Ok(path) => println!("reset {}", path.display()),
            Err(error) => fail(&error),
        },
        _ => usage(),
    }
}

fn theme(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    match (arguments.next(), arguments.next()) {
        (Some(name), None) => tui::config::set_theme(&name),
        _ => usage(),
    }
}

/// The keys, with `default` putting back the ones that shipped — said out loud, since
/// nothing else is overwriting a file the writer has edited.
fn keymap(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    match (arguments.next(), arguments.next(), arguments.next()) {
        (None, None, None) => tui::config::keymap::show().map(|text| println!("{text}")),
        (Some(word), None, None) if word == "default" => {
            tui::config::keymap::reset().map(|path| println!("reset {}", path.display()))
        }
        (Some(command), Some(key), None) => tui::config::keymap::set(&command, &key),
        _ => usage(),
    }
}

fn open(path: &Path) {
    if let Err(error) = tui::run(path) {
        fail(&error.to_string());
    }
}

fn report(result: Result<(), String>) {
    if let Err(error) = result {
        fail(&error);
    }
}

fn fail(error: &str) -> ! {
    eprintln!("markatui: {error}");
    std::process::exit(1);
}

fn usage() -> ! {
    eprintln!("{HELP}");
    std::process::exit(2);
}

fn invalid_option(option: &str) -> ! {
    eprintln!("markatui: unknown option '{option}'\n\n{HELP}");
    std::process::exit(2);
}
