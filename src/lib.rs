//! A Markdown editor for the terminal. The block under the cursor shows its source; every
//! other block is drawn as it reads, so the file on disk is exactly what was typed.
//!
//! Three layers, and the line between them is where a terminal starts mattering:
//!
//! - the core — [`editor`], [`active`], [`parse`], [`marks`], [`lint`], [`layout`],
//!   [`storage`], [`text`], [`style`] — knows nothing about terminals and is tested
//!   without one; the pieces only one of them uses sit inside it, so blocks and search
//!   live under [`editor`] and the spell checker under [`lint`];
//! - [`tui`] draws and reads the keyboard and the mouse through any terminal;
//! - `tui::probe` is the only place that asks what this one can do.

pub mod active;
pub mod editor;
pub mod layout;
pub mod lint;
pub mod marks;
pub mod parse;
pub mod storage;
pub mod style;
pub mod text;
pub mod tui;
