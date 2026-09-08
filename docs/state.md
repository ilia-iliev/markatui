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
  rule, selection inside a block and across them, copy and paste, the surround and link
  keys, and search. Up and down move by the rows on the screen rather than by the lines of
  the source, which is what wrapping made necessary.
- **Phase 3**, checking: the 400 ms settle rule on the event loop's own deadline, the wash,
  the footer prompt, cycling and accepting suggestions, and the personal dictionary.
- **Phase 4**, images: a lone image is the picture, drawn through kitty's protocol, sixel
  or a mosaic of half-blocks as the terminal answers. It is read, scaled and encoded on a
  thread; the block says what it is of until that comes back, and for good where there is
  no file. Rows appearing under a picture that arrives late are taken off the scroll, so
  the block being written in does not move. `tui::images` is the whole of it.
- **Phase 5**, shipping: `~/.config/markatui/config.toml` holds the column width, the
  palette, whether the terminal's own background shows through, and which key each command
  is on. It is read once before the first frame; whatever it says that cannot be read is
  named, with its line, at the foot of the screen until the first keystroke. Everything Qt
  the plan had left to remove was never here. `packaging/install.sh` builds statically
  against musl where that target is installed and puts the binary on the path.
- **The screenshot** the README opens with: foot at 900×1090, taken in a nested headless
  sway so that no desktop is in it. It is the whole sample document with the cursor in the
  heading — the hash shown because the block holds the cursor, the picture as sixel, and
  the checker taking exception to the title at the foot of the screen.

Nothing of the plan is left undone.

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
- **What the plan called `images` and `cell_px` are `ratatui_image::Picker`, and it is
  asked for after the alternate screen is entered rather than beside the keyboard.** The
  picker is the answer to both questions and the thing that draws, so splitting it into
  two fields on `Capabilities` would have been a copy of what it already holds. It is
  asked late because the query is answered on the screen the cursor is on, and that screen
  should be ours. `MARKATUI_PICTURES=halfblocks` forces the mosaic, which is the only way
  to see the fallback on a terminal that has a protocol of its own.
- **A picture is encoded once for the column it is in, not resized as it is drawn.** The
  plan named `StatefulProtocol`, which resizes at render time and so would block the event
  loop on the frame it lands. `SlicedProtocol` is encoded on the thread for a known number
  of cells and re-encoded only when the file or the column changes, which is also the
  answer to the sixel cost in the known issues. Being sliced is what lets a picture that
  is half off the top of the screen still draw the rows that are on it.
- **Paste reads the machine's clipboard; copy writes to both it and OSC 52.** OSC 52 is
  write-only — a terminal will take text but will not hand any back — so a Ctrl+V that had
  only OSC 52 behind it could paste nothing but what the editor itself had copied. Reading
  the real clipboard means `arboard`, which is the one dependency here that exists because
  of what the machine is rather than what the editor does. Copy still goes out over OSC 52
  as well, and last, because a terminal that takes it owns the clipboard from then on and
  its ownership outlives the editor: `arboard`'s does not, which is also why the handle is
  held open for the life of the editor rather than made per copy — the process that copied
  has to be alive to hand the text over when it is asked for. Where there is no display
  server, paste falls back on the last thing copied here, and the terminal's own paste key
  still brings in everything else.
- **The config file is read, not parsed.** The plan named `serde` and `toml`. What the
  file holds is two tables of `key = value` and three kinds of value — a string in quotes,
  a whole number, and true or false — which is sixty lines and no dependencies, against
  four crates for a format none of the rest of it uses. A line it cannot read is named
  rather than guessed at, and the lines around it are still read: a colour spelled wrong
  should not cost the writer their keymap. An array or a table inside a table is the point
  at which `serde` would earn its place, and there is neither.
- **Only the commands are bindable, and they are one list.** `keys::COMMANDS` is the name
  the config uses, the key it comes with and what pressing it means, in one row each, so a
  command cannot be given a name without a default or a default without a meaning. Moving
  around — the arrows, Home, End, the page keys — is not in it: there is nothing to argue
  about in what an arrow does, and leaving them out keeps the tangle of Ctrl and Shift with
  the arrows in one place.
- **The background is the terminal's own, and the config is what changes that.** Painting the warm paper colour over a dark
  terminal jars, and `inherit_background` was to be the config's answer to that. Until
  there is a config, inheriting is the default; the accent, the muted grey and the wash
  are picked to read on either.
- **The foot of the screen keeps its row whether or not it has anything to say.** A sixel
  drawn on the very last row of the terminal makes it scroll a line to make room for what
  it thinks comes next; the row that goes off the top is gone, and every frame after it —
  drawn as a difference from what the editor believes is on the screen — is one row out.
  The checker speaking up used to take a row off the document, which put the bottom of a
  picture on that last row, so walking on and off a misspelled word scrolled the screen a
  line at a time. Keeping the row means the document ends above the last row of the
  terminal and the writing does not move as the foot fills and empties.
