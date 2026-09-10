//! A Markdown editor for the terminal. The block under the cursor shows its source; every
//! other block is drawn as it reads, so the file on disk is exactly what was typed.
//!
//! Three layers, and the line between them is where a terminal starts mattering:
//!
//! - the core — [`blocks`], [`parse`], [`marks`], [`search`], [`spell`], [`lint`],
//!   [`state`], [`storage`], [`text`], [`style`], [`active`], [`editor`], [`layout`] —
//!   knows nothing about terminals and is tested without one;
//! - [`tui`] draws and reads the keyboard and the mouse through any terminal;
//! - [`tui::probe`] is the only place that asks what this one can do.

pub mod active;
pub mod blocks;
pub mod editor;
pub mod layout;
pub mod lint;
pub mod marks;
pub mod parse;
pub mod search;
pub mod spell;
pub mod state;
pub mod storage;
pub mod style;
pub mod text;
pub mod tui;
