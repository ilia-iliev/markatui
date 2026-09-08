use std::fs;
use std::path::Path;

/// Files whose cursor position is worth remembering, most recent first.
const LIMIT: usize = 200;

fn entries() -> Vec<(String, usize)> {
    let Some(store) = crate::storage::cursors() else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(store) else {
        return Vec::new();
    };
    text.lines().filter_map(parse_entry).collect()
}

fn parse_entry(line: &str) -> Option<(String, usize)> {
    let (index, path) = line.split_once('\t')?;
    Some((path.to_string(), index.parse().ok()?))
}

/// The block the cursor was left in last time this file was open.
pub fn recall(path: &Path) -> Option<usize> {
    let path = path.to_string_lossy();
    entries()
        .into_iter()
        .find(|(known, _)| *known == path)
        .map(|(_, index)| index)
}

pub fn remember(path: &Path, index: usize) {
    let Some(store) = crate::storage::cursors() else {
        return;
    };
    let path = path.to_string_lossy().to_string();
    let mut kept = vec![(path.clone(), index)];
    kept.extend(entries().into_iter().filter(|(known, _)| *known != path));
    kept.truncate(LIMIT);

    let text: String = kept
        .iter()
        .map(|(path, index)| format!("{index}\t{path}\n"))
        .collect();
    crate::storage::replace(&store, text.as_bytes());
}

/// The mode the editor was left in, where a run has left one.
pub fn recall_mode() -> Option<String> {
    let store = crate::storage::mode()?;
    let word = fs::read_to_string(store).ok()?;
    Some(word.trim().to_string())
}

/// Note the mode so the next run opens the way this one closed.
pub fn remember_mode(mode: &str) {
    let Some(store) = crate::storage::mode() else {
        return;
    };
    crate::storage::replace(&store, format!("{mode}\n").as_bytes());
}
