//! The little format `config.toml` and `keymap.toml` are both written in: one
//! `name = value` to a line, `#` starting a comment except inside quotes, and a line that
//! will not read named rather than guessed at. Neither file is TOML enough to be worth a
//! parser; what the two of them share is this walk and the shape of what it says.

/// Walk the lines of a settings file, handing each one that is neither blank nor a
/// comment to `read_line`. What comes back is every line that would not read, named by
/// the file and the line it was on.
pub fn walk(
    file: &str,
    text: &str,
    mut read_line: impl FnMut(&str) -> Result<(), String>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = without_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        if let Err(complaint) = read_line(line) {
            problems.push(format!("{file} line {}: {complaint}", number + 1));
        }
    }
    problems
}

/// What is left of a line once a comment is taken off it. A `#` inside a string is part
/// of the string — a colour is written with one.
fn without_comment(line: &str) -> &str {
    let mut quoted = false;
    for (at, character) in line.char_indices() {
        match character {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..at],
            _ => {}
        }
    }
    line
}

/// What is inside a pair of quotes, where the value is in them.
pub fn quoted(value: &str) -> Option<&str> {
    value.strip_prefix('"').and_then(|rest| rest.strip_suffix('"'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_hash_that_is_part_of_a_colour() {
        assert_eq!(without_comment("accent = \"#112233\" # green"), "accent = \"#112233\" ");
        assert_eq!(without_comment("# all of it"), "");
    }

    #[test]
    fn names_the_file_and_the_line_a_complaint_belongs_to() {
        let problems = walk("config.toml", "\nfine\n\nbad\n", |line| match line {
            "bad" => Err("no".to_string()),
            _ => Ok(()),
        });
        assert_eq!(problems, ["config.toml line 4: no"]);
    }
}
