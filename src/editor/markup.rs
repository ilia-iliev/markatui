//! The markers the writer puts round their words, and the blocks that are markup rather
//! than prose: emphasis, headings, links, rules, tables, pictures.
//!
//! All of it is the same two steps — change the block under the cursor, then note the
//! edit — so what is here is which change each key asks for. Reading a link is here too:
//! what a link looks like is the same question whether one is being written or followed.

use crate::active::Active;
use crate::blocks;
use crate::editor::Editor;
use crate::marks::{self, Align, Mark};
use crate::parse;
use crate::text::byte_offset;

use std::sync::Arc;

/// The table a writer is given to fill in. Two columns and one row of them, which is the
/// smallest thing that still reads as a table once it is drawn.
const TABLE: &str = "| Heading | Heading |\n| --- | --- |\n|  |  |";

impl Editor {
    pub fn surround(&mut self, marker: &str) {
        self.clear_spanning_selection();
        self.active.surround(marker);
        self.record_edit();
    }

    /// Underline, which markdown has no marker of its own for and HTML does.
    pub fn wrap(&mut self, tag: &str) {
        self.clear_spanning_selection();
        self.active.wrap(tag);
        self.record_edit();
    }

    /// Put a heading, a bullet, a number or a quote at the head of the lines the writer
    /// is standing on, or take it off them.
    pub fn mark(&mut self, mark: Mark) {
        self.clear_spanning_selection();
        self.active.mark_lines(mark);
        self.record_edit();
    }

    pub fn fence(&mut self) {
        self.clear_spanning_selection();
        self.active.fence();
        self.record_edit();
    }

    /// Set the column the cursor is in to read left, centre or right. Only a table has
    /// columns; anywhere else the key does nothing rather than something surprising.
    pub fn align(&mut self, align: Align) {
        let Some((table, cursor)) = marks::aligned(self.active.text(), self.active.cursor(), align)
        else {
            return;
        };
        self.clear_spanning_selection();
        self.active = Active::new(&table, cursor);
        self.record_edit();
    }

    /// A rule of its own, with an empty paragraph under it for what comes next: a writer
    /// asking for a rule is between two things, not at the end of the document.
    pub fn insert_rule(&mut self) {
        self.add_block("---");
        self.add_block("");
        self.record_edit();
    }

    /// A table to fill in, with the first heading selected to be typed over.
    pub fn insert_table(&mut self) {
        self.add_block(TABLE);
        self.active.select(2, 9);
        self.record_edit();
    }

    /// Put `text` in as a block of its own after the one the cursor is in, and move into
    /// it. A block with nothing in it is used rather than pushed down: an empty paragraph
    /// is where the writer already is.
    fn add_block(&mut self, text: &str) {
        self.clear_spanning_selection();
        self.store_active();
        if !self.blocks[self.index].trim().is_empty() {
            self.blocks.insert(self.index + 1, Arc::new(String::new()));
            self.gaps.insert(self.index + 1, Arc::new(blocks::PARAGRAPH.to_string()));
            self.index += 1;
        }
        self.blocks[self.index] = Arc::new(text.to_string());
        self.active = Active::new(&self.blocks[self.index], 0);
    }

    pub fn insert_link(&mut self, prefix: &str) {
        self.clear_spanning_selection();
        self.active.insert_link(prefix);
        self.record_edit();
    }

    /// Where the link under the cursor points, if it is standing in one. The address is
    /// as the writer wrote it; a relative one is resolved against the document, which is
    /// the directory it was written relative to.
    pub fn link_at_cursor(&self) -> Option<String> {
        self.link_in(self.index, self.active.cursor())
    }

    /// The link `at` characters into block `index`, which is what a click on one asks
    /// after: the block clicked in is not always the block the cursor is in.
    pub fn link_in(&self, index: usize, at: usize) -> Option<String> {
        let text = self.block(index);
        let url = parse::link_at(text, byte_offset(text, at))?;
        if url.contains("://") || url.starts_with('#') || url.starts_with("mailto:") {
            return Some(url);
        }
        let beside = self.path.parent()?.join(&url);
        Some(match beside.exists() {
            true => beside.to_string_lossy().into_owned(),
            false => url,
        })
    }

    /// A picture that has just been written beside the document, named by the block it
    /// goes in.
    pub fn insert_picture(&mut self, file: &str) {
        self.clear_spanning_selection();
        self.active.insert_picture(file);
        self.record_edit();
    }

    /// A selection that has left this block is let go before an edit that only makes
    /// sense inside one: there is no wrapping a marker round several blocks.
    fn clear_spanning_selection(&mut self) {
        if self.anchor.is_some() {
            self.anchor = None;
            self.active.drop_selection();
        }
    }
}
