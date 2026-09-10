use std::process::Command;

#[test]
fn help_flag_prints_help_without_opening_a_file() {
    let output =
        Command::new(env!("CARGO_BIN_EXE_markatui")).arg("-h").output().expect("markatui runs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"), "{stdout}");
    assert!(stdout.contains("-h, --help"), "{stdout}");
    assert!(output.stderr.is_empty());
}

#[test]
fn long_help_flag_prints_help_without_opening_a_file() {
    let output =
        Command::new(env!("CARGO_BIN_EXE_markatui")).arg("--help").output().expect("markatui runs");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
    assert!(output.stderr.is_empty());
}

#[test]
fn unknown_flag_reports_the_available_flags() {
    let output = Command::new(env!("CARGO_BIN_EXE_markatui"))
        .arg("-nonexistant_flag")
        .output()
        .expect("markatui runs");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown option '-nonexistant_flag'"), "{stderr}");
    assert!(stderr.contains("Available options:"), "{stderr}");
    assert!(stderr.contains("-h, --help"), "{stderr}");
}

#[test]
fn theme_command_writes_the_selected_preset_to_config() {
    let config = std::env::temp_dir().join(format!("markatui-theme-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_markatui"))
        .args(["theme", "dark"])
        .env("XDG_CONFIG_HOME", &config)
        .output()
        .expect("markatui runs");

    assert!(output.status.success(), "{output:?}");
    let written = std::fs::read_to_string(config.join("markatui/config.toml")).expect("a config");
    assert_eq!(written, "theme = \"dark\"\n");
    std::fs::remove_dir_all(config).expect("the temporary config goes");
}

/// A config put back to its defaults is a config that is not there: what the editor
/// reads then is the defaults themselves. The file it removed is named, because nothing
/// else the command line does takes a file the writer wrote away.
#[test]
fn config_default_takes_the_config_away_and_says_which_one() {
    let config = std::env::temp_dir().join(format!("markatui-reset-{}", std::process::id()));
    let run = |arguments: [&str; 2]| {
        Command::new(env!("CARGO_BIN_EXE_markatui"))
            .args(arguments)
            .env("XDG_CONFIG_HOME", &config)
            .output()
            .expect("markatui runs")
    };

    assert!(run(["theme", "dark"]).status.success());
    let written = config.join("markatui/config.toml");
    assert!(written.exists(), "there is a config to put back");

    let output = run(["-config", "default"]);

    assert!(output.status.success(), "{output:?}");
    assert!(!written.exists(), "the config is still there");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("reset"), "{stdout}");
    assert!(stdout.contains("config.toml"), "{stdout}");

    // A config that was never there is the state the reset was after, not a failure.
    assert!(run(["-config", "default"]).status.success());
    std::fs::remove_dir_all(config).expect("the temporary config goes");
}

/// The foot of the screen is settled in the config file alone now.
#[test]
fn the_footer_is_no_longer_a_command_line_setting() {
    let output = Command::new(env!("CARGO_BIN_EXE_markatui"))
        .args(["-footer", "paper"])
        .output()
        .expect("markatui runs");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown option '-footer'"), "{stderr}");
    assert!(!stderr.contains("band|invert|paper"), "{stderr}");
}

#[test]
fn theme_flag_rejects_an_unknown_preset() {
    let output = Command::new(env!("CARGO_BIN_EXE_markatui"))
        .args(["-theme", "sepia"])
        .output()
        .expect("markatui runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("choose light, dark, or terminal"),
        "{output:?}"
    );
}

/// Turning a check off writes it into the config, and the listing then says so. The two
/// halves are one test because the second only means anything after the first.
#[test]
fn turns_a_check_off_and_says_so_in_the_listing() {
    let config = std::env::temp_dir().join(format!("markatui-checks-{}", std::process::id()));
    let markatui = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_markatui"));
        command.env("XDG_CONFIG_HOME", &config);
        command
    };

    let turned_off =
        markatui().args(["-checks", "off", "UseTitleCase"]).output().expect("markatui runs");
    assert!(turned_off.status.success(), "{turned_off:?}");

    let written = std::fs::read_to_string(config.join("markatui/config.toml")).expect("a config");
    assert!(written.contains("[checks]\nUseTitleCase = false"), "{written}");

    let listed = markatui().arg("-checks").output().expect("markatui runs");
    let stdout = String::from_utf8_lossy(&listed.stdout);
    let line =
        stdout.lines().find(|line| line.starts_with("UseTitleCase ")).expect("the check is listed");
    assert!(line.ends_with("[off]"), "{line}");

    std::fs::remove_dir_all(config).expect("the temporary config goes");
}

/// A check nobody has heard of is a mistake worth stopping for, not a line to write.
#[test]
fn refuses_to_turn_off_a_check_that_does_not_exist() {
    let output = Command::new(env!("CARGO_BIN_EXE_markatui"))
        .args(["-checks", "off", "NoSuchRule"])
        .output()
        .expect("markatui runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no check called \"NoSuchRule\""),
        "{output:?}"
    );
}
