use markatui::tui;
use std::path::PathBuf;

const HELP: &str = "Usage:
  markatui <file.md>
  markatui keymap show
  markatui keymap <command> <key>

Available options:
  -h, --help  Print help";

fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(mut first) = arguments.next() else {
        usage();
    };

    if matches!(first.as_str(), "-h" | "--help" | "help") {
        println!("{HELP}");
        return;
    }

    if first.starts_with('-') && first != "--" {
        invalid_option(&first);
    }

    if first == "--" {
        let Some(path) = arguments.next() else {
            usage();
        };
        first = path;
    }

    if first == "keymap" {
        let result = match (arguments.next(), arguments.next(), arguments.next()) {
            (Some(command), None, None) if command == "show" => {
                tui::config::keymap::show().map(|text| println!("{text}"))
            }
            (Some(command), Some(key), None) => tui::config::keymap::set(&command, &key),
            _ => usage(),
        };
        if let Err(error) = result {
            eprintln!("markatui: {error}");
            std::process::exit(1);
        }
        return;
    }

    if arguments.next().is_some() {
        usage();
    }
    if let Err(error) = tui::run(&PathBuf::from(first)) {
        eprintln!("markatui: {error}");
        std::process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!("{HELP}");
    std::process::exit(2);
}

fn invalid_option(option: &str) -> ! {
    eprintln!("markatui: unknown option '{option}'\n\n{HELP}");
    std::process::exit(2);
}
