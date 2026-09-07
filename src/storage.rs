use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

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
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique temporary file",
    ))
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

/// Replace one of the editor's own small stores — the writer's dictionary, the cursor
/// it left in each file — making the directory it lives in if this is the first time.
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

    #[test]
    fn replaces_a_file_without_leaving_the_temporary_one() {
        let directory = std::env::temp_dir().join(format!(
            "markatui-storage-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("post.md");
        fs::write(&path, "old").unwrap();

        write_atomic(&path, b"new").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }
}
