use std::process::Command;

#[test]
fn help_flag_prints_help_without_opening_a_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_markatui"))
        .arg("-h")
        .output()
        .expect("markatui runs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"), "{stdout}");
    assert!(stdout.contains("-h, --help"), "{stdout}");
    assert!(output.stderr.is_empty());
}

#[test]
fn help_command_prints_help_without_opening_a_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_markatui"))
        .arg("help")
        .output()
        .expect("markatui runs");

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
    assert!(
        stderr.contains("unknown option '-nonexistant_flag'"),
        "{stderr}"
    );
    assert!(stderr.contains("Available options:"), "{stderr}");
    assert!(stderr.contains("-h, --help"), "{stderr}");
}
