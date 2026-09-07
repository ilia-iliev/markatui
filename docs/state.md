# Where this stands

The plan in `tui-plan.md` was written against blogawrite's tree, where the terminal front
end would grow beside the Qt one behind a cargo feature. This is a separate repository
instead, so blogawrite stays as it is and is the reference to check against. Phase 0 —
extracting the editor from Qt — is therefore not a step here: the core was written Qt-free
from the start, with `document.rs` read rather than converted.

## Done

- **The core**, all of it tested without a terminal: `blocks`, `parse`, `search`, `spell`,
  `lint`, `state`, `storage`, `text`, `style`, plus the three new modules the plan names —
  `editor`, `active`, `layout`. Offsets are Unicode scalar values throughout; nothing
  counts in UTF-16 any more, so `text.rs` is half the size it was.
- **Phase 1**, the read-only view: paragraphs, headings, lists with nesting, quotes, fenced
  code, rules, tables, and the inline styles. Wrapping goes through grapheme clusters and
  `unicode-width` from the start, so a wide character takes two columns. Snapshots in
  `tests/screen.rs` at 40, 80, 100 and 120 columns.
- **Phase 2**, editing: typing, deleting, splitting, merging, undo with the typing-run
  rule, selection inside a block and across them, copy over OSC 52, the surround and link
  keys, and search. Up and down move by the rows on the screen rather than by the lines of
  the source, which is what wrapping made necessary.
- **Phase 3**, checking: the 400 ms settle rule on the event loop's own deadline, the wash,
  the footer prompt, cycling and accepting suggestions, and the personal dictionary.

## Not done

- **Phase 4, images.** A lone image is drawn as one muted line naming the file. The probe
  has no `images` or `cell_px` yet: they go in when there is something reading them.
- **Phase 5, shipping.** No config file, so the palette, the column width and the keymap
  are the constants in `tui::theme` and `tui::keys`. No musl build, no packaging.

## Where the plan was departed from, and why

- **The stepping and backspace rules for hidden markers are not there, because they cannot
  fire.** The reveal rule already shows a span's markers as soon as the cursor reaches the
  span, either end included, so there is never an invisible character beside the cursor to
  step over or delete unseen. Writing the guards anyway would have been dead code on every
  keystroke. The invariant they rested on is written down instead, as
  `style::never_hides_a_marker_beside_the_cursor`, which walks every cursor position of
  several texts and asserts it.
- **Block structure did not become bits in `style_mask`.** What a line's structure is —
  a bullet, a number as the writer wrote it, how deep in quotes — is a property of the
  line, not of a character, and the layout needs the glyph as well as the fact. It is
  `style::prefix` instead: one function, its own tests, and the mask left to inline markup.
- **Tables and images open up into markdown under the cursor rather than mapping the
  cursor through padding cells.** A table's columns and an image's picture stand where no
  character does. This is the rule the Qt front end already had, and it takes the hardest
  part of the plan's Phase 2 out of it without costing the writer anything.
- **The background is the terminal's own.** Painting the warm paper colour over a dark
  terminal jars, and `inherit_background` was to be the config's answer to that. Until
  there is a config, inheriting is the default; the accent, the muted grey and the wash
  are picked to read on either.
