//! Block surgery: the moves that change how many blocks the document is held as —
//! splitting one in two, merging one into its neighbour, and putting a block back as
//! whatever it re-parses into.

use crate::active::Active;
use crate::blocks;
use crate::editor::EditRun;
use crate::editor::Editor;
use std::sync::Arc;

impl Editor {
    /// Break the block at the cursor, dropping the newline the writer typed to get here.
    pub(super) fn split_block(&mut self) {
        let (before, after) = self.active.split();
        let (mut split, mut separators) = blocks::replacement(&before);
        self.hoist(&mut split, &mut separators);
        // Counted after the hoist: what the cursor moves on by is however many blocks the
        // half in front of it came to.
        let head = split.len();
        let (mut tail, mut tail_separators) = blocks::replacement(&after);
        self.hoist(&mut tail, &mut tail_separators);
        separators.push(blocks::PARAGRAPH.to_string());
        separators.extend(tail_separators);
        split.extend(tail);

        self.replace_block(self.index, split, separators);
        self.index += head;
        self.active = Active::new(&self.blocks[self.index], 0);
        self.record_edit();
    }

    pub(super) fn merge_with_previous(&mut self) {
        let Some(previous) = self.index.checked_sub(1) else { return };
        let cursor = crate::text::length(&self.blocks[previous]);
        let tail = self.active.text().to_string();

        Arc::make_mut(&mut self.blocks[previous]).push_str(&tail);
        // The source between the two blocks goes with the seam; what followed the second
        // one now follows the joined block.
        self.gaps[self.index] = self.gaps[self.index + 1].clone();
        self.remove_blocks(self.index, 1);
        self.index = previous;
        self.active = Active::new(&self.blocks[previous], cursor);
        self.record_run(EditRun::Deleting(-1));
    }

    /// Pull the block below into this one, which is what Delete at the end of a block
    /// means: the mirror of Backspace at the start of the one below it.
    pub(super) fn merge_with_next(&mut self) {
        let next = self.index + 1;
        if next >= self.blocks.len() {
            return;
        }
        let cursor = self.active.cursor();
        let mut joined = self.active.text().to_string();
        joined.push_str(&self.blocks[next]);
        self.blocks[self.index] = Arc::new(joined);
        // The source between the two goes with the seam; what followed the second block
        // now follows the joined one.
        self.gaps[next] = self.gaps[next + 1].clone();
        self.remove_blocks(next, 1);
        self.active = Active::new(&self.blocks[self.index], cursor);
        self.record_run(EditRun::Deleting(1));
    }

    /// Replace a selection running through more than one block with `insert`, joining
    /// what is left of the blocks at its two ends into one. `false` where the selection
    /// is inside a single block, which the block sees to itself.
    pub(super) fn take_spanning_selection(&mut self, insert: &str) -> bool {
        let Some(span) = self.anchor.and(self.selection()).filter(|span| span.first != span.last)
        else {
            return false;
        };
        let (kept, cursor) = blocks::spliced(&self.blocks, span, insert);
        self.blocks[span.first] = Arc::new(kept);
        self.gaps[span.first + 1] = self.gaps[span.last + 1].clone();
        self.remove_blocks(span.first + 1, span.last - span.first);
        self.anchor = None;
        self.index = span.first;
        self.active = Active::new(&self.blocks[span.first], cursor);
        self.record_edit();
        true
    }

    /// Re-read the block being edited now that the cursor is leaving it, splitting it
    /// where the writer has typed a blank line. Returns the change in the block count.
    pub(super) fn commit(&mut self) -> isize {
        self.store_active();
        let block = self.blocks[self.index].clone();
        // A block whose content was deleted disappears, unless it is all that is left.
        // Empty blocks opened by Enter are the writer's blank lines and stay put.
        if self.active.emptied() && block.trim().is_empty() {
            if self.blocks.len() == 1 {
                return 0;
            }
            self.remove_blocks(self.index, 1);
            return -1;
        }
        let (mut replacement, mut separators) = blocks::replacement(&block);
        self.hoist(&mut replacement, &mut separators);
        if replacement.len() == 1 && replacement[0] == *block {
            return 0;
        }
        self.replace_block(self.index, replacement, separators)
    }

    /// Break out any picture the writer left among the words: a terminal draws a picture
    /// into rows of its own, so a paragraph holding one becomes a paragraph for the words
    /// and a paragraph for the picture. It happens as the cursor leaves the block, which
    /// is when the picture would be drawn — while the block is being written in, what was
    /// typed stays where it was typed.
    pub(super) fn hoist(&mut self, blocks: &mut Vec<String>, separators: &mut Vec<String>) {
        self.hoisted |= blocks::hoist_images(blocks, separators);
    }

    /// Swap block `index` for the blocks it re-parsed into.
    pub(super) fn replace_block(
        &mut self,
        index: usize,
        replacement: Vec<String>,
        separators: Vec<String>,
    ) -> isize {
        let delta = replacement.len() as isize - 1;
        let mut replacement = replacement.into_iter();
        self.blocks[index] = Arc::new(replacement.next().unwrap_or_default());
        for (offset, block) in replacement.enumerate() {
            self.blocks.insert(index + 1 + offset, Arc::new(block));
        }
        for (offset, separator) in separators.into_iter().enumerate() {
            self.gaps.insert(index + 1 + offset, Arc::new(separator));
        }
        delta
    }

    pub(super) fn remove_blocks(&mut self, first: usize, count: usize) {
        self.blocks.drain(first..first + count);
        // Keep the separator before the removed span and discard the rest of the source
        // occupied by it. Callers may have replaced that kept separator first.
        self.gaps.drain(first + 1..first + count + 1);
    }
}
