//! The files a pasted picture leaves beside the document, and keeping them agreeing with
//! what the document says about them.
//!
//! Markdown has no way of holding a picture, so a paste writes a PNG of its own and puts
//! the line naming it into the block. From then on the writer can rename the file by
//! editing that line, or drop the picture by deleting it — and the file on disk has to
//! follow, but only when the document is saved. Renaming on every keystroke would mean a
//! filename half-typed is a file half-named.

use crate::parse;
use crate::storage;
use crate::tui::App;

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// A picture written since the editor opened, and what the document called it when it was
/// written. `ordinal` is which image of the document it was, which is what makes editing
/// only the name in the line count as a rename rather than as a different picture.
pub(super) struct PastedPicture {
    path: PathBuf,
    reference: String,
    ordinal: usize,
    saved: bool,
}

/// The renames a save has worked out, and the ones it has already made. Held on to until
/// the document itself is written: a document that would not save has to leave the files
/// as it found them, or the references still in it would be naming files that had moved.
pub(super) struct Renamed {
    desired: Vec<Option<String>>,
    targets: Vec<Option<PathBuf>>,
    done: Vec<(PathBuf, PathBuf)>,
}

impl Renamed {
    /// Every rename put back, newest first, which is the order that undoes them.
    pub(super) fn undo(&self) {
        for (from, to) in self.done.iter().rev() {
            let _ = fs::rename(from, to);
        }
    }
}

impl App {
    /// A picture, which markdown has no way of holding: it is written beside the document
    /// as a PNG of its own, and what goes into the block is the line naming that file.
    pub(super) fn paste_picture(&mut self, png: &[u8]) {
        match storage::write_picture(self.editor.path(), png) {
            Ok(file) => {
                self.editor.error = None;
                self.edit(|editor| editor.insert_picture(&file));
                let ordinal = parse::image_paths(&self.editor.source())
                    .iter()
                    .position(|reference| reference == &file)
                    .expect("the picture reference was just inserted");
                let path =
                    self.editor.path().parent().unwrap_or_else(|| Path::new(".")).join(&file);
                self.pictures.push(PastedPicture { path, reference: file, ordinal, saved: false });
            }
            Err(error) => self.editor.error = Some(format!("Could not save the picture: {error}")),
        }
    }

    /// Move the pasted files to the names the document now gives them, ready for it to be
    /// written. Nothing where a name could not be used: the writer is told why, and the
    /// files are left as they were.
    pub(super) fn rename_pictures(&mut self) -> Option<Renamed> {
        let desired = self.picture_references();
        let targets = self.picture_targets(&desired)?;

        let mut done = Vec::new();
        for (picture, target) in self.pictures.iter().zip(&targets) {
            let Some(target) = target else { continue };
            if target == &picture.path {
                continue;
            }
            if let Err(error) = fs::rename(&picture.path, target) {
                let renamed = Renamed { desired, targets, done };
                renamed.undo();
                self.editor.error = Some(format!("Could not rename the picture: {error}"));
                return None;
            }
            done.push((target.clone(), picture.path.clone()));
        }
        Some(Renamed { desired, targets, done })
    }

    /// Where each pasted picture is headed, worked out before anything moves. Nothing
    /// where a name will not do — one that is not a plain filename, or one already taken
    /// by a file this save did not write — so that a save either makes every rename or
    /// makes none.
    fn picture_targets(&mut self, desired: &[Option<String>]) -> Option<Vec<Option<PathBuf>>> {
        let mut targets = Vec::with_capacity(desired.len());
        for (picture, reference) in self.pictures.iter().zip(desired) {
            let Some(reference) = reference else {
                targets.push(None);
                continue;
            };
            let Ok(target) = picture_target(self.editor.path(), reference) else {
                self.editor.error = Some(format!("Could not rename the picture to {reference}"));
                return None;
            };
            if target != picture.path && target.exists() {
                self.editor.error = Some(format!(
                    "Could not rename the picture: {} already exists",
                    target.display()
                ));
                return None;
            }
            targets.push(Some(target));
        }
        Some(targets)
    }

    /// The document is written, so the renames stand: every picture takes the name the
    /// document gives it, and the files it no longer refers to go.
    pub(super) fn settle_pictures(&mut self, renamed: Renamed) {
        let Renamed { desired, targets, .. } = renamed;
        for ((picture, reference), target) in self.pictures.iter_mut().zip(&desired).zip(targets) {
            if let (Some(reference), Some(target)) = (reference, target) {
                picture.path = target;
                picture.reference = reference.clone();
                picture.saved = true;
            }
        }
        for (picture, reference) in self.pictures.iter().zip(&desired) {
            if reference.is_none()
                && let Err(error) = fs::remove_file(&picture.path)
                && error.kind() != io::ErrorKind::NotFound
            {
                self.editor.error =
                    Some(format!("Could not remove the unreferenced picture: {error}"));
            }
        }
        let mut index = 0;
        self.pictures.retain(|_| {
            let keep = desired[index].is_some();
            index += 1;
            keep
        });
    }

    /// Match a pasted picture by its current name first, then by the image's position.
    /// The latter is what makes editing only the destination count as a rename.
    fn picture_references(&self) -> Vec<Option<String>> {
        let references = parse::image_paths(&self.editor.source());
        let mut claimed = vec![false; references.len()];
        let mut desired = vec![None; self.pictures.len()];

        for (index, picture) in self.pictures.iter().enumerate() {
            let found = references
                .iter()
                .enumerate()
                .filter(|(at, reference)| {
                    !reference.is_empty() && !claimed[*at] && *reference == &picture.reference
                })
                .min_by_key(|(at, _)| at.abs_diff(picture.ordinal));
            if let Some((at, reference)) = found {
                claimed[at] = true;
                desired[index] = Some(reference.clone());
            }
        }
        for (index, picture) in self.pictures.iter().enumerate() {
            if desired[index].is_none()
                && picture.ordinal < references.len()
                && !references[picture.ordinal].is_empty()
                && !claimed[picture.ordinal]
            {
                claimed[picture.ordinal] = true;
                desired[index] = Some(references[picture.ordinal].clone());
            }
        }
        desired
    }

    /// Pictures introduced since the last successful save belong to discarded edits.
    pub(super) fn discard_pictures(&mut self) {
        for picture in &self.pictures {
            if !picture.saved {
                let _ = fs::remove_file(&picture.path);
            }
        }
        self.pictures.retain(|picture| picture.saved);
    }
}

/// The file a reference in the document names, beside the document. Only a plain filename
/// will do: a reference with a directory in it is the writer pointing at a picture of
/// their own, and moving a pasted file out from under them is not what they asked for.
fn picture_target(document: &Path, reference: &str) -> io::Result<PathBuf> {
    let relative = Path::new(reference);
    let mut components = relative.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "picture names must be filenames"));
    }
    Ok(document.parent().unwrap_or_else(|| Path::new(".")).join(relative))
}
