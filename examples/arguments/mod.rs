//! What the measurement examples read off their own command lines. They are built and run
//! by hand rather than through a front end, so a flag and the word after it is as far as
//! this goes.

/// The number written after `flag`, where one is and it reads as a number.
pub fn number(arguments: &[String], flag: &str) -> Option<usize> {
    let at = arguments.iter().position(|argument| argument == flag)?;
    arguments.get(at + 1)?.parse().ok()
}
