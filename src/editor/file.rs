//! The document's other end: reading a file into blocks, and writing the blocks back out.
//!
//! Between those two the editor never touches the filesystem, which is what keeps typing
//! cheap. It is also where the two ways a save can lose text are caught — a file that
//! would not open, and a file somebody else has written since — because both are
//! questions about the file rather than about the document.

use crate::active::Active;
use crate::blocks;
use crate::editor::{Editor, LintState, SearchState};
use crate::parse;
use crate::storage;

use std::collections::VecDeque;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

impl Editor {
    /// Open `path`, resolved against the working directory. A path that does not exist
    /// yet starts an empty document that [`Editor::save`] will create.
    pub fn open(path: &Path) -> Self {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        };
        let path = resolve(&path);
        let (source, error) = match fs::read_to_string(&path) {
            Ok(source) => (source, None),
            Err(error) if error.kind() == ErrorKind::NotFound => (String::new(), None),
            Err(error) => {
                (String::new(), Some(format!("Could not open {}: {error}", path.display())))
            }
        };

        let seen = modified(&path);
        let mut editor = Editor::read(&source, path, error);
        editor.unreadable = editor.error.is_some();
        editor.seen = seen;
        let last = editor.blocks.len() - 1;
        // Pick up where the last session left off in this file, or at its end.
        editor.settle_in(storage::recall(&editor.path).unwrap_or(last).min(last));
        editor.record_cursor();
        editor
    }

    /// A document made out of `source`. Any picture the writer left among the words is
    /// broken out into a paragraph of its own before the cursor ever reaches the block it
    /// was left in: that is where a terminal can draw it, and the writer opened the file
    /// to look at the picture. The document then says something the file does not, which
    /// is why it opens with work to save.
    ///
    /// The blank lines a run of them left between two blocks become empty paragraphs at
    /// the same time. That one costs nothing to save: the source is cut in more places,
    /// not changed.
    pub(super) fn read(source: &str, path: PathBuf, error: Option<String>) -> Self {
        let mut segments = parse::segments(source);
        let hoisted = blocks::normalise(&mut segments);
        let blocks: Vec<Arc<String>> = segments.blocks.into_iter().map(Arc::new).collect();
        let mut editor = Editor {
            active: Active::new(&blocks[0], usize::MAX),
            blocks,
            gaps: segments.gaps.into_iter().map(Arc::new).collect(),
            index: 0,
            anchor: None,
            undo: VecDeque::new(),
            redo: Vec::new(),
            revision: u64::from(hoisted),
            saved_revision: 0,
            edit_run: None,
            settled: true,
            hoisted,
            path,
            unreadable: false,
            seen: None,
            insisted: false,
            error,
            lint: LintState::default(),
            search: SearchState::default(),
        };
        editor.record_cursor();
        editor
    }

    /// The document as it would be written out.
    pub fn source(&self) -> String {
        let mut blocks = self.blocks.clone();
        blocks[self.index] = Arc::new(self.active.text().to_string());
        blocks::source(&blocks, &self.gaps)
    }

    /// Whether somebody else has written the file since it was opened, so that saving
    /// would go over them. It stays true until the writer says to write over it.
    pub fn changed_on_disk(&self) -> bool {
        !self.insisted && self.seen != modified(&self.path)
    }

    /// The writer's answer to that: write over the changes. The next save goes through,
    /// and the one after it asks again.
    pub fn overwrite(&mut self) {
        self.insisted = true;
    }

    /// Write the document out, unless doing so would lose text the editor never had.
    /// A file that would not read is never written over, and one somebody else has
    /// written is not either until [`Editor::overwrite`] says so — the asking is the
    /// caller's, because it is a question and not an error.
    pub fn save(&mut self) -> bool {
        if self.unreadable {
            self.error = Some(format!(
                "Not saving over {}: it would not open, so this document is not it",
                self.path.display()
            ));
            return false;
        }
        if self.changed_on_disk() {
            return false;
        }
        self.store_active();
        if let Err(error) = storage::write_atomic(&self.path, self.source().as_bytes()) {
            let message = format!("Could not save {}: {error}", self.path.display());
            eprintln!("markatui: {message}");
            self.error = Some(message);
            return false;
        }
        self.error = None;
        self.seen = modified(&self.path);
        self.insisted = false;
        self.remember_position();
        self.saved_revision = self.revision;
        true
    }

    /// Note where the cursor is so the next session can pick it up.
    pub fn remember_position(&self) {
        storage::remember(&self.path, self.index);
    }
}

/// When the file was last written, or nothing where there is no file to ask about. The
/// two cases a filesystem can answer with — no file, and no clock — are the same answer
/// here: nothing to compare a later look against.
fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

/// The path with the symbolic links along it followed, so that saving replaces the file
/// a link points at rather than the link itself. A file that does not exist yet is
/// resolved as far as the directory it will be made in.
fn resolve(path: &Path) -> PathBuf {
    if let Ok(resolved) = fs::canonicalize(path) {
        return resolved;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => match fs::canonicalize(parent) {
            Ok(parent) => parent.join(name),
            Err(_) => path.to_path_buf(),
        },
        _ => path.to_path_buf(),
    }
}
