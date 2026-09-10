use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// The names of the two files the writer edits. Kept here beside the paths they are
/// joined onto, because they are also what a complaint about a bad line names.
pub const CONFIG_FILE: &str = "config.toml";
pub const KEYMAP_FILE: &str = "keymap.toml";

/// The colours and the column: `~/.config/markatui/config.toml`.
pub fn config_file() -> Option<PathBuf> {
    Some(config_dir()?.join(CONFIG_FILE))
}

/// Which key each command is on: `~/.config/markatui/keymap.toml`.
pub fn keymap_file() -> Option<PathBuf> {
    Some(config_dir()?.join(KEYMAP_FILE))
}

/// The writer's own words — names, jargon, the title of the thing they are writing
/// about — one to a line, `#` for a comment.
pub fn dictionary() -> Option<PathBuf> {
    Some(config_dir()?.join("dictionary"))
}

/// The block the cursor was left in, a file to a line. Kept apart from the config
/// because the writer never writes it and never has to keep it.
pub fn cursors() -> Option<PathBuf> {
    Some(state_dir()?.join("cursors"))
}

/// The mode the editor was last left in, and nothing else: one word, the whole file. It
/// is the writer's doing but not their setting — they change it with a key, not by
/// editing anything — so it lives here rather than in the config.
pub fn mode() -> Option<PathBuf> {
    Some(state_dir()?.join("mode"))
}

/// Where the writer's own settings live: their config, their keymap, their dictionary.
fn config_dir() -> Option<PathBuf> {
    Some(home("XDG_CONFIG_HOME", ".config")?.join("markatui"))
}

/// Where the editor's own notes to itself live.
fn state_dir() -> Option<PathBuf> {
    Some(home("XDG_STATE_HOME", ".local/state")?.join("markatui"))
}

fn home(variable: &str, default: &str) -> Option<PathBuf> {
    match std::env::var_os(variable) {
        Some(directory) => Some(PathBuf::from(directory)),
        None => Some(PathBuf::from(std::env::var_os("HOME")?).join(default)),
    }
}

/// Replace `path` atomically with `contents`. The temporary file sits beside the target,
/// so rename cannot cross filesystems. Existing permissions are retained.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let (temporary, mut file) = temporary_file(path)?;
    let result = write_and_replace(path, &temporary, &mut file, contents);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_file(path: &Path) -> io::Result<(PathBuf, File)> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("document");

    for _ in 0..100 {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let candidate = directory.join(format!(".{name}.{}.{}.tmp", std::process::id(), id));
        match OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "could not create a unique temporary file"))
}

fn write_and_replace(
    path: &Path,
    temporary: &Path,
    file: &mut File,
    contents: &[u8],
) -> io::Result<()> {
    if let Ok(metadata) = fs::metadata(path) {
        file.set_permissions(metadata.permissions())?;
    }
    file.write_all(contents)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;

    // Linux requires the directory itself to be synced for the rename to survive a crash.
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    File::open(directory)?.sync_all()
}

/// Put a pasted picture beside `document` and say what it ended up called. The name is
/// the document's own with a number after it — `post-1.png`, then `post-2.png` — so that
/// the pictures of a post sort next to it and a second paste never writes over the first.
/// The name alone is the answer: a relative path is what the document names a picture by,
/// and is read back from the directory the document is in.
pub fn write_picture(document: &Path, png: &[u8]) -> io::Result<String> {
    let directory = document.parent().unwrap_or_else(|| Path::new("."));
    let stem = document.file_stem().and_then(|name| name.to_str()).unwrap_or("picture");
    let name = (1..)
        .map(|number| format!("{stem}-{number}.png"))
        .find(|name| !directory.join(name).exists())
        .expect("the numbers outlast the disk");
    write_atomic(&directory.join(&name), png)?;
    Ok(name)
}

/// Replace one of the editor's own small stores — the writer's dictionary, the cursor it
/// left in each file, the mode it was closed in — making the directory it lives in if
/// this is the first time.
/// These are written behind the writer's back, so a failure is reported and let go
/// rather than raised: none of it is their text.
pub fn replace(path: &Path, contents: &[u8]) {
    if let Some(directory) = path.parent()
        && let Err(error) = fs::create_dir_all(directory)
    {
        eprintln!("markatui: {}: {error}", directory.display());
        return;
    }
    if let Err(error) = write_atomic(path, contents) {
        eprintln!("markatui: {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "markatui-storage-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        directory
    }

    #[test]
    fn replaces_a_file_without_leaving_the_temporary_one() {
        let directory = directory();
        let path = directory.join("post.md");
        fs::write(&path, "old").unwrap();

        write_atomic(&path, b"new").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    /// Every pasted picture gets a name of its own, so the second one does not land on
    /// the first.
    #[test]
    fn names_a_pasted_picture_after_the_document_and_counts_up() {
        let directory = directory();
        let document = directory.join("post.md");

        assert_eq!(write_picture(&document, b"one").unwrap(), "post-1.png");
        assert_eq!(write_picture(&document, b"two").unwrap(), "post-2.png");
        assert_eq!(fs::read(directory.join("post-1.png")).unwrap(), b"one");
        assert_eq!(fs::read(directory.join("post-2.png")).unwrap(), b"two");
        fs::remove_dir_all(directory).unwrap();
    }
}
