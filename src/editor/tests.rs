use super::*;
use crate::lint;
use crate::marks::{Align, Mark};
use std::fs;

/// A document made without touching the disk, opened the way a file is. The path is
/// never written to, and the cursor starts at the top rather than wherever some other
/// session left it. Nothing has settled: the checker has its say once a test asks.
pub(super) fn document(source: &str) -> Editor {
    let mut editor = Editor::read(source, PathBuf::from("/nowhere/post.md"), None);
    editor.settled = false;
    editor.active.place(0);
    editor.record_cursor();
    editor
}

pub(super) fn texts(editor: &Editor) -> Vec<String> {
    (0..editor.blocks.len()).map(|index| editor.block(index).to_string()).collect()
}

#[test]
fn types_into_the_block_the_cursor_is_in() {
    let mut editor = document("one\n\ntwo");
    editor.activate(1, 0);
    editor.insert("X");
    assert_eq!(texts(&editor), ["one", "Xtwo"]);
    assert_eq!(editor.source(), "one\n\nXtwo");
}

/// The first Enter breaks the line inside the paragraph and the second ends the
/// block, so that a writer who means a line break never has to reach for a second
/// key to get one.
#[test]
fn breaks_the_line_first_and_the_block_second_on_enter() {
    let mut editor = document("one two");
    editor.activate(0, 3);
    editor.enter();
    assert_eq!(texts(&editor), ["one\n two"]);
    assert_eq!(editor.index(), 0);

    editor.enter();
    assert_eq!(texts(&editor), ["one", " two"]);
    assert_eq!(editor.index(), 1);
    assert_eq!(editor.source(), "one\n\n two");
}

/// Shift+Enter is the line break Enter used to leave: one paragraph, written on two
/// lines.
#[test]
fn leaves_a_line_break_in_the_block_on_shift_enter() {
    let mut editor = document("one two");
    editor.activate(0, 3);
    editor.line_break();
    assert_eq!(texts(&editor), ["one\n two"]);
    assert_eq!(editor.index(), 0);
}

/// A blank line in code is a blank line of code: the fence is what closes a code
/// block, and Enter inside one never breaks it in half.
#[test]
fn writes_lines_of_code_on_enter_inside_a_fence() {
    let mut editor = document("```rust\nfn main() {}\n```\n\nafter");
    editor.activate(0, 20);
    editor.enter();
    editor.enter();
    assert_eq!(texts(&editor), ["```rust\nfn main() {}\n\n\n```", "after"]);
    assert_eq!(editor.index(), 0);
}

/// Past the closing fence there is no more code to write, so Enter means the
/// paragraph after it — otherwise a code block at the foot of a document is a room
/// with no door.
#[test]
fn ends_a_code_block_on_enter_past_its_closing_fence() {
    let mut editor = document("```rust\nfn main() {}\n```");
    editor.activate(0, 24);
    editor.enter();
    assert_eq!(texts(&editor), ["```rust\nfn main() {}\n```", ""]);
    assert_eq!(editor.index(), 1);
}

/// A table goes on by the row, with as many cells as the row the cursor is in.
#[test]
fn opens_another_row_on_enter_in_a_table() {
    let mut editor = document("| a | b |\n| --- | --- |\n| 1 | 2 |");
    editor.activate(0, 28);
    editor.enter();
    assert_eq!(texts(&editor), ["| a | b |\n| --- | --- |\n| 1 | 2 |\n|  |  |"]);
    // In the first cell of it, which is where the writer types next.
    assert_eq!(editor.active.cursor(), 36);
    // The row left empty says the table is done, the way an empty item ends a list.
    editor.enter();
    assert_eq!(texts(&editor), ["| a | b |\n| --- | --- |\n| 1 | 2 |", ""]);
    assert_eq!(editor.index(), 1);
}

/// The row of dashes belongs to the heading above it: a row opened from the heading
/// goes under the dashes, not between them and the heading.
#[test]
fn opens_a_row_under_the_dashes_when_the_cursor_is_in_the_heading() {
    let mut editor = document("| a | b |\n| --- | --- |");
    editor.activate(0, 3);
    editor.enter();
    assert_eq!(texts(&editor), ["| a | b |\n| --- | --- |\n|  |  |"]);
}

#[test]
fn takes_a_list_item_in_and_out_a_level_on_tab() {
    let mut editor = document("- one\n- two");
    editor.activate(0, 8);
    editor.tab(1);
    assert_eq!(texts(&editor), ["- one\n  - two"]);
    editor.tab(-1);
    assert_eq!(texts(&editor), ["- one\n- two"]);
    // Nothing left to give back, and nothing typed in its place.
    editor.tab(-1);
    assert_eq!(texts(&editor), ["- one\n- two"]);
}

#[test]
fn walks_the_cells_of_a_table_on_tab() {
    let mut editor = document("| a | b |\n| --- | --- |\n| 1 | 2 |");
    editor.activate(0, 2);
    editor.tab(1);
    assert_eq!(editor.active.cursor(), 6);
    editor.tab(-1);
    assert_eq!(editor.active.cursor(), 2);
    // Nothing is typed into a table by Tab, whichever way it goes.
    assert_eq!(texts(&editor), ["| a | b |\n| --- | --- |\n| 1 | 2 |"]);
}

#[test]
fn types_a_tab_where_there_is_nothing_to_walk() {
    let mut editor = document("words");
    editor.activate(0, 5);
    editor.tab(1);
    assert_eq!(texts(&editor), ["words\t"]);
    editor.tab(-1);
    assert_eq!(texts(&editor), ["words\t"]);
}

/// Ctrl with Backspace and Delete takes out a whole word, and at the edge of the
/// block it joins the neighbour the plain keys join.
#[test]
fn deletes_a_word_at_a_time_on_control_with_the_delete_keys() {
    let mut editor = document("one two three\n\nfour");
    editor.activate(0, 13);
    editor.delete_word(-1);
    assert_eq!(texts(&editor), ["one two ", "four"]);
    editor.delete_word(-1);
    assert_eq!(texts(&editor), ["one ", "four"]);
    // At the start of a block there is no word left to take, so the blocks join.
    editor.activate(1, 0);
    editor.delete_word(-1);
    assert_eq!(texts(&editor), ["one four"]);
}

/// Delete at the end of a block pulls the next one up, which is the mirror of
/// Backspace at the start of the one below.
#[test]
fn pulls_the_next_block_up_on_delete_at_the_end_of_one() {
    let mut editor = document("one\n\ntwo\n\nthree");
    editor.activate(0, 3);
    editor.delete(1);
    assert_eq!(texts(&editor), ["onetwo", "three"]);
    assert_eq!(editor.index(), 0);
    assert_eq!(editor.active.cursor(), 3);
    assert_eq!(editor.source(), "onetwo\n\nthree");
    // At the end of the last block there is nothing to pull up.
    editor.activate(1, 5);
    editor.delete(1);
    assert_eq!(texts(&editor), ["onetwo", "three"]);
}

/// A quote goes on by the line: the markers come along, and a line left empty ends it.
#[test]
fn carries_the_quote_markers_onto_the_next_line() {
    let mut editor = document("> quoted");
    editor.activate(0, 8);
    editor.enter();
    assert_eq!(texts(&editor), ["> quoted\n> "]);
    editor.enter();
    assert_eq!(texts(&editor), ["> quoted", ""]);
    assert_eq!(editor.index(), 1);
}

/// A run of blank lines in a file is what a run of Enters writes, and it comes back
/// as the empty paragraphs it was typed as — the same rows on the screen the writer
/// left, and the same bytes on the disk.
#[test]
fn opens_the_blank_lines_of_a_file_into_empty_paragraphs() {
    let editor = document("one\n\n\n\n\n\ntwo");
    assert_eq!(texts(&editor), ["one", "", "", "two"]);
    assert_eq!(editor.source(), "one\n\n\n\n\n\ntwo");
    assert!(!editor.dirty());
}

/// What Enter writes is what opening reads back, so a document does not grow a
/// paragraph every time it goes through the disk.
#[test]
fn reads_back_the_blank_paragraphs_it_wrote() {
    let mut editor = document("one");
    editor.activate(0, 3);
    // Twice to end the block — the break and the press that ends it — and twice
    // again for the paragraph left blank between them.
    for _ in 0..4 {
        editor.enter();
    }
    editor.insert("two");
    let source = editor.source();
    assert_eq!(texts(&editor), ["one", "", "two"]);
    assert_eq!(source, "one\n\n\n\ntwo");
    assert_eq!(texts(&document(&source)), ["one", "", "two"]);
}

/// A heading is one line, so Enter ends it where a paragraph would take a second
/// press: what follows a heading is a paragraph, with the one blank line under it
/// that a heading is written with.
#[test]
fn ends_a_heading_on_the_first_enter() {
    let mut editor = document("# Title");
    editor.activate(0, 7);
    editor.enter();
    assert_eq!(texts(&editor), ["# Title", ""]);
    assert_eq!(editor.index(), 1);
    editor.insert("body");
    assert_eq!(editor.source(), "# Title\n\nbody");
}

/// Enter halfway through a heading breaks it there: the words in front of the cursor
/// stay the heading, the words behind it become the paragraph under it.
#[test]
fn breaks_a_heading_where_the_cursor_stands() {
    let mut editor = document("# Big Title");
    editor.activate(0, 5);
    editor.enter();
    assert_eq!(texts(&editor), ["# Big", " Title"]);
    assert_eq!(editor.source(), "# Big\n\n Title");
}

#[test]
fn ends_a_setext_heading_on_the_first_enter() {
    let mut editor = document("Title\n=====");
    editor.activate(0, 11);
    editor.enter();
    assert_eq!(texts(&editor), ["Title\n=====", ""]);
    assert_eq!(editor.index(), 1);
}

#[test]
fn carries_the_list_marker_onto_the_next_line() {
    let mut editor = document("- one");
    editor.activate(0, 5);
    editor.enter();
    assert_eq!(texts(&editor), ["- one\n- "]);
    // The item left empty says the list is over: the marker goes, and what follows
    // is a paragraph of its own.
    editor.enter();
    assert_eq!(texts(&editor), ["- one", ""]);
    assert_eq!(editor.index(), 1);
}

#[test]
fn counts_the_next_item_of_a_numbered_list() {
    let mut editor = document("2. one");
    editor.activate(0, 6);
    editor.enter();
    assert_eq!(texts(&editor), ["2. one\n3. "]);
}

/// The one empty item goes and nothing is broken off: there is no line above it to
/// break away from.
#[test]
fn ends_a_list_that_was_only_ever_one_item() {
    let mut editor = document("");
    editor.activate(0, 0);
    editor.insert("- ");
    editor.enter();
    assert_eq!(texts(&editor), [""]);
}

#[test]
fn marks_the_lines_the_writer_is_standing_on() {
    let mut editor = document("one\ntwo");
    editor.activate(0, 1);
    editor.mark(Mark::Bullet);
    assert_eq!(texts(&editor), ["- one\ntwo"]);
    // The cursor keeps its place among the words rather than its place in the line.
    assert_eq!(editor.active().cursor(), 3);
    editor.mark(Mark::Bullet);
    assert_eq!(texts(&editor), ["one\ntwo"]);
}

#[test]
fn marks_every_line_a_selection_runs_through() {
    let mut editor = document("one\ntwo\nthree");
    editor.activate(0, 1);
    editor.active.select(1, 5);
    editor.mark(Mark::Numbered);
    assert_eq!(texts(&editor), ["1. one\n2. two\nthree"]);
}

#[test]
fn puts_a_block_in_a_fence_and_takes_it_out_again() {
    let mut editor = document("x = 1");
    editor.activate(0, 0);
    editor.fence();
    assert_eq!(texts(&editor), ["```\nx = 1\n```"]);
    // The cursor is where the language goes.
    assert_eq!(editor.active().cursor(), 3);
    editor.fence();
    assert_eq!(texts(&editor), ["x = 1"]);
}

#[test]
fn adds_a_rule_with_somewhere_to_go_on_writing() {
    let mut editor = document("one");
    editor.activate(0, 3);
    editor.insert_rule();
    assert_eq!(texts(&editor), ["one", "---", ""]);
    assert_eq!(editor.index(), 2);
    assert_eq!(editor.source(), "one\n\n---\n\n");
}

#[test]
fn adds_a_table_with_the_first_heading_ready_to_type_over() {
    let mut editor = document("one");
    editor.activate(0, 3);
    editor.insert_table();
    assert_eq!(editor.index(), 1);
    assert_eq!(editor.active().selected_text(), "Heading");
    editor.insert("When");
    assert_eq!(editor.block(1), "| When | Heading |\n| --- | --- |\n|  |  |");
}

#[test]
fn sets_the_column_the_cursor_is_in_and_leaves_a_paragraph_alone() {
    let mut editor = document("| a | b |\n| --- | --- |\n| 1 | 2 |");
    editor.activate(0, 7);
    editor.align(Align::Right);
    assert_eq!(editor.block(0), "| a | b |\n| --- | ---: |\n| 1 | 2 |");

    let mut editor = document("just words");
    editor.activate(0, 3);
    editor.align(Align::Centre);
    assert_eq!(texts(&editor), ["just words"]);
}

#[test]
fn underlines_with_the_only_marker_markdown_has_for_it() {
    let mut editor = document("one two");
    editor.activate(0, 0);
    editor.active.select(0, 3);
    editor.wrap("u");
    assert_eq!(texts(&editor), ["<u>one</u> two"]);
    editor.wrap("u");
    assert_eq!(texts(&editor), ["one two"]);
}

#[test]
fn puts_back_what_an_undo_took() {
    let mut editor = document("one");
    editor.activate(0, 3);
    editor.insert(" two");
    editor.undo();
    assert_eq!(texts(&editor), ["one"]);
    editor.redo();
    assert_eq!(texts(&editor), ["one two"]);
    // And a fresh edit is the end of what there was to put back.
    editor.undo();
    editor.insert("!");
    editor.redo();
    assert_eq!(texts(&editor), ["one!"]);
}

#[test]
fn swaps_one_occurrence_and_then_the_rest() {
    let mut editor = document("cat\n\na cat and a cat");
    editor.open_search();
    editor.search_for("cat");
    editor.search.replacement = "dog".to_string();
    editor.replace_found();
    assert_eq!(texts(&editor), ["dog", "a cat and a cat"]);
    editor.replace_all();
    assert_eq!(texts(&editor), ["dog", "a dog and a dog"]);
}

#[test]
fn follows_the_link_the_cursor_is_standing_in() {
    let mut editor = document("go [home](https://example.com) now");
    editor.activate(0, 5);
    assert_eq!(editor.link_at_cursor().as_deref(), Some("https://example.com"));
    editor.activate(0, 0);
    assert_eq!(editor.link_at_cursor(), None);
}

#[test]
fn merges_a_block_into_the_one_before_it() {
    let mut editor = document("one\n\ntwo");
    editor.activate(1, 0);
    editor.delete(-1);
    assert_eq!(texts(&editor), ["onetwo"]);
    assert_eq!(editor.active().cursor(), 3);
    assert_eq!(editor.source(), "onetwo");
}

#[test]
fn leaves_the_first_block_alone_at_its_start() {
    let mut editor = document("one");
    editor.activate(0, 0);
    editor.delete(-1);
    assert_eq!(texts(&editor), ["one"]);
}

#[test]
fn re_reads_a_block_typed_into_several_when_the_cursor_leaves_it() {
    let mut editor = document("one\n\ntwo");
    editor.activate(0, 3);
    editor.insert("\n\nhalf");
    editor.activate(1, 0);
    assert_eq!(texts(&editor), ["one", "half", "two"]);
    assert_eq!(editor.source(), "one\n\nhalf\n\ntwo");
}

#[test]
fn drops_a_block_emptied_out() {
    let mut editor = document("one\n\ntwo");
    editor.activate(0, 3);
    for _ in 0..3 {
        editor.delete(-1);
    }
    editor.activate(1, 0);
    assert_eq!(texts(&editor), ["two"]);
}

#[test]
fn keeps_inserted_blank_lines_as_the_cursor_walks_through_them() {
    let mut editor = document("one");
    editor.activate(0, 3);
    for _ in 0..10 {
        editor.enter();
    }
    let source = editor.source();

    for step in [-1, 1] {
        for _ in 0..5 {
            editor.move_cursor(Motion::Line(step), false);
            assert_eq!(editor.source(), source);
        }
    }
}

/// A terminal draws a picture into rows of its own, so a picture left among the words
/// is broken out into a paragraph of its own the moment the document is read — before
/// the writer ever sees the block it was left in.
#[test]
fn breaks_out_a_picture_left_inline_as_the_document_opens() {
    let editor = document("words ![a](1.png) more\n\nplain\n");
    assert_eq!(texts(&editor), ["words", "![a](1.png)", "more", "plain"]);
    assert_eq!(editor.source(), "words\n\n![a](1.png)\n\nmore\n\nplain\n");
    // The document no longer says what the file says, and the writer is told so.
    assert!(editor.dirty());
    assert!(editor.hoisted);
}

#[test]
fn says_nothing_about_a_document_with_no_picture_to_move() {
    let editor = document("![a](1.png)\n\nplain");
    assert!(!editor.hoisted);
    assert!(!editor.dirty());
}

/// A picture typed among the words is left where it was typed while the block is being
/// written in — the alt text is typed after the file name — and is broken out when the
/// cursor leaves, which is when the picture would be drawn.
#[test]
fn breaks_out_a_picture_typed_inline_when_the_cursor_leaves_the_block() {
    let mut editor = document("words\n\nplain");
    editor.activate(0, 5);
    editor.insert(" ![a](1.png)");
    assert_eq!(texts(&editor), ["words ![a](1.png)", "plain"]);
    assert!(!editor.hoisted);

    editor.activate(1, 0);
    assert_eq!(texts(&editor), ["words", "![a](1.png)", "plain"]);
    assert!(editor.hoisted);
    // The cursor keeps to the block it was sent to, wherever the split moved it.
    assert_eq!(editor.index(), 2);
    assert_eq!(editor.block(editor.index()), "plain");
}

/// Enter breaks a block in two, and a picture left among the words of either half is
/// broken out with it. The cursor still lands at the start of the second half.
#[test]
fn breaks_out_a_picture_left_inline_by_a_split() {
    let mut editor = document("one two");
    editor.activate(0, 3);
    editor.insert(" ![a](1.png)");
    editor.enter();
    editor.enter();
    assert_eq!(texts(&editor), ["one", "![a](1.png)", " two"]);
    assert_eq!(editor.index(), 2);
    assert!(editor.hoisted);
}

/// The screen asks once and hears once: the message is the foot of the screen's to
/// show until the next keystroke, not something it is told again on every frame.
#[test]
fn says_a_picture_moved_only_once() {
    let mut editor = document("words ![a](1.png)");
    assert!(editor.take_hoisted());
    assert!(!editor.take_hoisted());
}

/// Right off the end of a block goes on into the next one, and Left off the start
/// comes back: a block boundary is a place the writing carries over, not a wall.
#[test]
fn steps_from_one_block_into_the_next_and_back() {
    let mut editor = document("alpha\n\nbeta");
    editor.activate(0, 5);
    editor.move_cursor(Motion::Character(1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (1, 0));

    editor.move_cursor(Motion::Character(-1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (0, 5));
}

/// Ctrl with Left and Right at the edge of a block carries on into the next one, the way
/// a plain arrow does, rather than holding the cursor where it is.
#[test]
fn walks_a_word_on_into_the_next_block() {
    let mut editor = document("alpha\n\nbeta");
    editor.activate(0, 5);
    editor.move_cursor(Motion::Word(1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (1, 0));

    editor.move_cursor(Motion::Word(-1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (0, 5));
}

/// Ctrl with the arrows walks the blocks the way Ctrl with Left and Right walks the
/// words: one press to the edge of the block the cursor stands in, and every press
/// after it a whole block along.
#[test]
fn cycles_forward_through_the_blocks() {
    let mut editor = document("alpha\n\nbeta\n\ngamma");
    editor.activate(0, 2);
    editor.move_cursor(Motion::Block(1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (0, 5));

    editor.move_cursor(Motion::Block(1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (1, 4));

    editor.move_cursor(Motion::Block(1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (2, 5));

    // The end of the document is where the walk stops.
    editor.move_cursor(Motion::Block(1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (2, 5));
}

#[test]
fn cycles_back_through_the_blocks() {
    let mut editor = document("alpha\n\nbeta\n\ngamma");
    editor.activate(2, 3);
    editor.move_cursor(Motion::Block(-1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (2, 0));

    editor.move_cursor(Motion::Block(-1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (1, 0));

    editor.move_cursor(Motion::Block(-1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (0, 0));

    editor.move_cursor(Motion::Block(-1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (0, 0));
}

/// A block a press walks past is a block the selection takes in whole, gaps and all.
#[test]
fn takes_a_whole_block_in_as_it_cycles_past_it() {
    let mut editor = document("alpha\n\nbeta\n\ngamma");
    editor.activate(0, 5);
    editor.move_cursor(Motion::Block(1), true);
    assert_eq!(editor.selected_text(), "\n\nbeta");

    editor.move_cursor(Motion::Block(1), true);
    assert_eq!(editor.selected_text(), "\n\nbeta\n\ngamma");
}

/// The ends of the document have no block to go on to, so the cursor stays where the
/// text stops rather than going nowhere.
#[test]
fn stays_at_the_ends_of_the_document() {
    let mut editor = document("alpha\n\nbeta");
    editor.activate(0, 0);
    editor.move_cursor(Motion::Character(-1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (0, 0));

    editor.activate(1, 4);
    editor.move_cursor(Motion::Character(1), false);
    assert_eq!((editor.index(), editor.active().cursor()), (1, 4));
}

/// Down on the last line of the document has no line and no block to go to, so it
/// goes to the end of the text — which is what lets Shift take the last line in.
#[test]
fn draws_the_selection_out_to_the_end_of_the_document() {
    let mut editor = document("alpha\n\nbeta\ngamma");
    editor.activate(1, 5);
    editor.move_cursor(Motion::Line(1), true);
    assert_eq!(editor.selected_text(), "gamma");
}

/// The same movement without a selection: the caret lands at the end of the last line
/// rather than staying where it was.
#[test]
fn takes_the_cursor_to_the_end_of_the_document_from_the_last_line() {
    let mut editor = document("beta\ngamma");
    editor.activate(0, 5);
    editor.move_cursor(Motion::Line(1), false);
    assert_eq!(editor.active().cursor(), 10);
}

/// And the other end reads the same way: up from the first line goes to the start.
#[test]
fn draws_the_selection_back_to_the_start_of_the_document() {
    let mut editor = document("alpha\nbeta\n\ngamma");
    editor.activate(0, 3);
    editor.move_cursor(Motion::Line(-1), true);
    assert_eq!(editor.selected_text(), "alp");
}

/// Only the first and last lines do this. A line with one above and below it moves by
/// a line, keeping to the column it is aiming for.
#[test]
fn keeps_to_the_column_on_a_line_with_one_below_it() {
    let mut editor = document("alpha\nbeta\ngamma");
    editor.activate(0, 3);
    editor.move_cursor(Motion::Line(1), true);
    assert_eq!(editor.selected_text(), "ha\nbet");
}

#[test]
fn carries_a_selection_across_blocks_and_reads_it_with_its_gaps() {
    let mut editor = document("alpha\n\nbeta\n\ngamma");
    editor.activate(0, 2);
    editor.move_cursor(Motion::Line(1), true);
    editor.move_cursor(Motion::Line(1), true);
    assert_eq!(editor.selected_text(), "pha\n\nbeta\n\n");
}

#[test]
fn takes_out_a_selection_that_runs_through_several_blocks() {
    let mut editor = document("alpha\n\nbeta\n\ngamma");
    editor.activate(0, 2);
    editor.move_cursor(Motion::Line(1), true);
    editor.move_cursor(Motion::Line(1), true);
    editor.insert("X");
    // What was in front of the selection joined to what was behind it.
    assert_eq!(texts(&editor), ["alXgamma"]);
}

#[test]
fn types_over_the_whole_document() {
    let mut editor = document("alpha\n\nbeta");
    editor.select_all();
    editor.insert("X");
    assert_eq!(texts(&editor), ["X"]);
}

/// A selection drawn out of a block and back into it is the block's own again, pinned
/// where it started: what it covers is what it always covered.
#[test]
fn brings_a_selection_back_into_the_block_it_was_pinned_in() {
    let mut editor = document("alpha\n\nbeta");
    editor.activate(0, 2);
    editor.move_cursor(Motion::Line(1), true);
    assert_eq!(editor.selected_text(), "pha\n\n");
    editor.move_cursor(Motion::Line(-1), true);
    assert_eq!(editor.selected_text(), "pha");
    // And it is the block's own, so typing over it replaces exactly that much.
    editor.insert("X");
    assert_eq!(texts(&editor), ["alX", "beta"]);
}

#[test]
fn selects_the_whole_document_and_reads_it_back() {
    let mut editor = document("alpha\n\nbeta");
    editor.select_all();
    assert_eq!(editor.selected_text(), "alpha\n\nbeta");
}

#[test]
fn undoes_a_run_of_typing_as_one_thing() {
    let mut editor = document("one");
    editor.activate(0, 3);
    for letter in [" ", "t", "w", "o"] {
        editor.insert(letter);
    }
    assert_eq!(texts(&editor), ["one two"]);
    editor.undo();
    assert_eq!(texts(&editor), ["one"]);
}

#[test]
fn undoes_a_run_of_backspaces_as_one_thing() {
    let mut editor = document("one two");
    editor.activate(0, 7);
    for _ in 0..3 {
        editor.delete(-1);
    }
    assert_eq!(texts(&editor), ["one "]);
    editor.undo();
    assert_eq!(texts(&editor), ["one two"]);
}

#[test]
fn changing_delete_direction_starts_a_new_undo_step() {
    let mut editor = document("abcdef");
    editor.activate(0, 3);
    editor.delete(-1);
    editor.delete(-1);
    editor.delete(1);
    editor.delete(1);
    assert_eq!(texts(&editor), ["af"]);

    editor.undo();
    assert_eq!(texts(&editor), ["adef"]);
    editor.undo();
    assert_eq!(texts(&editor), ["abcdef"]);
}

#[test]
fn backspacing_across_a_block_boundary_stays_in_the_run() {
    let mut editor = document("one\n\ntwo");
    editor.activate(1, 1);
    editor.delete(-1);
    editor.delete(-1);
    assert_eq!(texts(&editor), ["onewo"]);
    editor.undo();
    assert_eq!(texts(&editor), ["one", "two"]);
}

#[test]
fn ends_a_run_of_typing_when_the_writer_pauses() {
    let mut editor = document("");
    editor.insert("a");
    editor.settle(true);
    editor.settle(false);
    editor.insert("b");
    editor.undo();
    assert_eq!(texts(&editor), ["a"]);
}

#[test]
fn undoes_a_split_and_a_merge_whole() {
    let mut editor = document("one two");
    editor.activate(0, 3);
    editor.line_break();
    editor.enter();
    assert_eq!(texts(&editor), ["one", " two"]);
    editor.undo();
    assert_eq!(texts(&editor), ["one\n two"]);
}

#[test]
fn is_dirty_only_while_it_differs_from_what_was_saved() {
    let mut editor = document("one");
    assert!(!editor.dirty());
    editor.insert("X");
    assert!(editor.dirty());
    editor.undo();
    assert!(!editor.dirty());
}

#[test]
fn wraps_a_selection_in_a_marker() {
    let mut editor = document("bold word");
    editor.activate(0, 0);
    editor.move_cursor(Motion::Word(1), true);
    editor.surround("**");
    assert_eq!(texts(&editor), ["**bold** word"]);
}

#[test]
fn walks_the_occurrences_of_a_word_and_selects_each_one() {
    let mut editor = document("a marker\n\nb marker");
    editor.search_for("marker");
    assert_eq!(editor.search.count, 2);
    assert_eq!(editor.index(), 0);
    assert_eq!(editor.active().selected_text(), "marker");
    editor.cycle_search(1);
    assert_eq!(editor.index(), 1);
    assert_eq!(editor.active().selected_text(), "marker");
}

#[test]
fn says_when_a_word_it_found_has_nowhere_to_walk_to() {
    let mut editor = document("only once");
    editor.search_for("once");
    editor.cycle_search(1);
    assert!(editor.search.alone);
}

/// The checker loads on threads of its own; this waits for it once.
fn checker() {
    lint::preload(&lint::Checks::new());
    while !lint::ready() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn takes_the_checkers_suggestion_in_one_key_and_gives_it_back_in_one() {
    checker();
    let mut editor = document("I recieve mail.");
    editor.settle(true);
    editor.activate(0, 4);
    assert_eq!(editor.lint.word, "recieve");
    assert_eq!(editor.lint.suggestion(), Some("receive"));

    editor.accept_lint();
    assert_eq!(texts(&editor), ["I receive mail."]);
    editor.undo();
    assert_eq!(texts(&editor), ["I recieve mail."]);
}

#[test]
fn walks_the_suggestions_both_ways_and_comes_back_round() {
    checker();
    let mut editor = document("I recieve mail.");
    editor.settle(true);
    editor.activate(0, 4);
    let offered = editor.lint.replacements.len();
    assert!(offered > 1, "the dictionary has more than one guess");

    editor.cycle_lint(1);
    assert_eq!(editor.lint.choice, 1);
    editor.cycle_lint(-1);
    assert_eq!(editor.lint.choice, 0);
    editor.cycle_lint(-1);
    assert_eq!(editor.lint.choice, offered - 1);
}

/// Nothing is said about a block while it is being typed into: a word half-written is
/// not a word spelled wrong.
#[test]
fn says_nothing_about_a_block_still_being_typed_into() {
    checker();
    let mut editor = document("I recieve mail.");
    editor.settle(true);
    editor.activate(0, 4);
    assert!(!editor.lint.message.is_empty());

    editor.settle(false);
    assert!(editor.lint.message.is_empty());
    editor.settle(true);
    assert!(!editor.lint.message.is_empty());
}

#[test]
fn keeps_the_source_it_was_given_byte_for_byte() {
    let editor = document("# Title\n\n\nBody  \n\n[home]: https://example.com\n");
    assert_eq!(editor.source(), "# Title\n\n\nBody  \n\n[home]: https://example.com\n");
}

/// A directory of its own for a test that has to touch the disk, so that one test
/// opening a file says nothing about what another one sees.
fn directory(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("markatui-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("a temporary directory");
    directory
}

/// A file the editor cannot read is not an empty file, and the empty document it puts
/// on the screen instead must never be written back over it.
#[test]
fn refuses_to_save_over_a_file_that_would_not_open() {
    let directory = directory("unreadable");
    let path = directory.join("post.md");
    // Latin-1: a byte no UTF-8 reader will take.
    fs::write(&path, b"caf\xe9\n").unwrap();

    let mut editor = Editor::open(&path);
    assert!(editor.error.is_some(), "the failed open is said out loud");

    assert!(!editor.save());
    assert!(!editor.save(), "insisting does not get past it either");
    assert_eq!(fs::read(&path).unwrap(), b"caf\xe9\n");
    fs::remove_dir_all(directory).unwrap();
}

/// A document reached through a symbolic link is the file at the end of the link.
/// Saving replaces that file and leaves the link alone.
#[test]
#[cfg(unix)]
fn saves_through_a_symbolic_link_rather_than_over_it() {
    let directory = directory("symlink");
    let real = directory.join("real.md");
    let link = directory.join("link.md");
    fs::write(&real, "one\n").unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let mut editor = Editor::open(&link);
    editor.insert("X");
    assert!(editor.save());

    assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_to_string(&real).unwrap(), "oneX\n");
    fs::remove_dir_all(directory).unwrap();
}

/// A file written by somebody else since it was opened stops the save. The caller
/// asks, and the save after the answer goes through: by then it is the writer's
/// decision.
#[test]
fn stops_the_first_save_over_a_file_that_changed_on_disk() {
    let directory = directory("changed");
    let path = directory.join("post.md");
    fs::write(&path, "one\n").unwrap();

    let mut editor = Editor::open(&path);
    editor.insert("X");
    fs::write(&path, "somebody else\n").unwrap();

    assert!(!editor.save());
    assert!(editor.changed_on_disk());
    assert!(editor.error.is_none(), "a question was written down as an error");
    assert_eq!(fs::read_to_string(&path).unwrap(), "somebody else\n");

    editor.overwrite();
    assert!(editor.save());
    assert_eq!(fs::read_to_string(&path).unwrap(), "oneX\n");
    fs::remove_dir_all(directory).unwrap();
}

/// Saving twice over is the ordinary case and says nothing: the editor's own write is
/// not somebody else's.
#[test]
fn saves_again_without_complaining_about_its_own_write() {
    let directory = directory("again");
    let path = directory.join("post.md");
    fs::write(&path, "one\n").unwrap();

    let mut editor = Editor::open(&path);
    editor.insert("X");
    assert!(editor.save());
    editor.insert("Y");
    assert!(editor.save());

    assert_eq!(fs::read_to_string(&path).unwrap(), "oneXY\n");
    fs::remove_dir_all(directory).unwrap();
}
