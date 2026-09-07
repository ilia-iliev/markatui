# blogawrite in the terminal

Move blogawrite from Qt to a terminal UI. Foot first. Alacritty and kitty later,
without rework: anything that differs between terminals is decided in one place, at
startup, and everything else reads that decision.

## What stays, what goes

Stays: one file per instance, the block model (raw block under the cursor, rendered
blocks everywhere else), markers hidden away from the cursor, spelling and grammar
checking with the personal dictionary, search, undo, cross-block selection, atomic
save, remembered cursor position, the keymap.

Changes: headings are bold, coloured and ruled rather than bigger. Images draw through
the terminal's graphics protocol, or as half-block mosaics where there is none. Copy
goes through OSC 52. Line height is a blank line between blocks.

Goes: mouse, link opening, proportional fonts, font sizes, Qt, C++, QML, the AppImage.

Deferred to a feature release: sized headings on kitty via its text sizing protocol.

## Where the terminal-specific line is

Three layers. The rule: only the probe names a terminal, and even it asks the terminal
what it can do rather than matching `$TERM` where a query exists.

### Core (no terminal, no Qt)

`blocks`, `parse`, `search`, `spell`, `lint`, `state`, `storage`, `text`, `style`, plus
three new modules:

- `editor`: what `DocumentRust` holds today minus the Qt types. Blocks, gaps, active
  index, selection, undo, revision, lint state, search state, file path. Every
  `#[qinvokable]` becomes a plain method. This is the Phase 0 extraction.
- `active`: the text of the block being edited, with its cursor and selection anchor.
  Qt's `TextEdit` owned this; nothing in Rust did. Insert, delete, move by character,
  word, line, home, end; `surround`, `insert_link`, `split`, `merge`, the marker-run
  logic from `ActiveBlock.qml`. Offsets are Unicode scalar values, not UTF-16.
- `layout`: turns a block into rows of styled cells at a given width. One entry
  point for every block, active or not: the mask from `style_mask`, and a cursor that
  is -1 for a block that does not hold it. The cursor changes only which markers are
  revealed and where the caret cell is. It keeps a map both ways between source
  offsets and cells, and cells may map to nothing: padding, box borders, bullets.
  Width is in cells via `unicode-width`, so wide characters take two.

All of it is tested without a terminal.

### TUI (any terminal)

`ratatui` + `crossterm`. Event loop, the scrolling column, the active block widget,
the footer with the lint prompt and the search bar, the quit prompt, the keymap, the
config file. This layer consumes a `Capabilities` value and never asks which terminal
it is in.

### Probe (terminal-specific)

One module, run once at startup, producing:

```
Capabilities {
    images:    Kitty | Sixel | HalfBlocks,
    keyboard:  KittyProtocol | Legacy,
    clipboard: Osc52 | None,
    cell_px:   Option<(u16, u16)>,
    sized_text: bool,      // unused until the kitty heading work
}
```

How each is found:

- `images`: `ratatui-image`'s `Picker` already does this: environment variables first,
  then a kitty graphics query and a DA1 query for sixel, falling back to half-blocks.
  Foot answers the sixel query. Alacritty answers neither and gets half-blocks. Kitty
  answers the kitty query. No code changes between the three.
- `keyboard`: push the kitty keyboard enhancement flags and query them back, which
  `crossterm` supports. Foot, Alacritty and kitty all answer yes. Legacy exists only so
  a terminal that says no still runs, with the collisions (Ctrl+I as Tab, Ctrl+Enter
  as Enter) documented rather than worked around.
- `clipboard`: OSC 52 is assumed on. There is no reliable query; a terminal that
  ignores it loses copy and nothing else.
- `cell_px`: `TIOCGWINSZ` pixel fields, which `Picker` reads. Needed to know how many
  rows an image takes. Missing under some multiplexers; then images fall back to
  half-blocks, which need no pixel size.

Adding a terminal later means checking what the probe reports in it, not touching the
TUI layer. Sized headings are the one planned feature that would add a branch outside
the probe: a layout entry point that reads `sized_text` and emits a scaled heading row.

## WYSIWYG

The source of truth is the markdown text. There is no rich-text model and no
conversion either way; what is on screen is a view of the text with markers hidden.
Typing `# ` at the start of a paragraph makes it a heading on the next frame because
the parser says so, not because the editor has a heading command.

`style_mask` already does this for inline markup: bold, italic, strike, code, link,
marker shown, marker hidden. It grows bits for block structure, and `layout` grows a
pass that can emit cells with no source behind them.

| Construct | Hidden away from cursor | Shown as |
|---|---|---|
| Heading `## ` | the hashes | bold, colour, rule under h1 and h2 |
| List `- `, `1. ` | the marker | bullet glyph or number, indent per nesting |
| Quote `> ` | the angle bracket | bar in the gutter |
| Code fences | both fence lines | box with the language as a label |
| Link `[text](url)` | brackets and url | accent text; url revealed when the cursor is inside |
| Image `![alt](path)` | the whole line | the image; raw line revealed above it when active |
| Table pipes | pipes and the dash row | box drawing with padded columns |
| Hard break `  ` | the two spaces | a newline |

Tables are the hard one: column widths come from the whole table, not from one
character, and padding cells map to nothing. They come last.

Rules that keep it editable, so nothing invisible ever vanishes:

- Markers that delimit the span the cursor stands in are shown, muted. This is the
  reveal rule the Qt version has.
- Left and Right step over a hidden marker as one unit, because it is not on screen.
- Backspace against a hidden marker reveals it first and deletes it on the second
  press.

Each construct gets tests in `style` saying which characters are marker, hidden or
content, with the cursor inside and outside the span.

## Phases

Each phase ends with something that runs. Qt keeps building behind a `qt` cargo
feature until Phase 5, so the extraction in Phase 0 can be checked against the working
app, and the TUI can be compared side by side.

### Phase 0: extract the editor from Qt

- New `src/editor.rs` with the state and methods of `DocumentRust`, no `QString`,
  `QUrl` or `Pin`. `document.rs` becomes a wrapper that converts at the boundary and
  emits the model signals.
- Offsets: the core moves to Unicode scalar values. The Qt wrapper converts to and
  from UTF-16 with `text.rs`, which already does that arithmetic. The TUI has no
  conversion.
- Strip the `cxx::bridge` blocks from `lint.rs` and `style.rs` into the Qt wrapper so
  the core compiles without `cxx`.
- Cargo features: `qt` gates `cxx`, `cxx-qt`, `cxx-qt-lib`, `qt-build-utils`,
  `cxx-qt-build` and `build.rs`. Default on for now.
- Done when: the Qt app behaves as before, and `cargo test --no-default-features`
  passes without Qt installed.

### Phase 1: read-only TUI

- `src/tui/` with `main` loop, `probe`, `keys`, `view`. `blogawrite <file.md>` opens
  in the terminal by default; `--qt` picks the old front-end while it exists.
- `layout` from the mask with a cursor of -1: paragraph, heading, list with nesting,
  quote, fenced code, rule, and the inline styles. Tables are drawn raw until Phase 2.
  Images are one line of alt text for now.
- The column: `content_width` cells centred, `block_spacing` as blank rows, scroll
  keeping the active block on screen, PageUp and PageDown as today.
- Enable the kitty keyboard protocol, raw mode, alternate screen, bracketed paste.
  Ctrl+S saves. Quit asks when dirty, with the three answers from `Main.qml`.
- Error line for a file that cannot be read or written.
- Done when: `sample/post.md` reads correctly at 80 and 120 columns and after a resize,
  checked with `TestBackend` snapshots plus a look in foot.

### Phase 2: editing

- `active` buffer; the active block goes through the same `layout` as the rest with
  the cursor passed in, so it is the rendered block with the reveal rule applied.
- Block-level bits in `style_mask`: heading hashes, list markers, quote brackets,
  fences, link targets, image lines, hard breaks. One construct at a time, each with
  its tests.
- The stepping and Backspace rules for hidden markers.
- Tables: column widths from the whole block, padding cells with no source offset,
  the cursor mapping through them. Last in the phase.
- Cursor movement across wrapped rows, Home, End, word jumps, Up and Down leaving the
  block at its edges, Left and Right at the ends, exactly the rules in
  `ActiveBlock.qml`.
- Typing, Backspace, Delete, Enter splitting the block, Backspace at the start merging
  with the previous one, Ctrl+Z through the editor's undo with the typing-run rule.
- Selection: Shift with movement, Ctrl+A, extending across blocks, delete and type
  over, copy via OSC 52 and the `blocks::selected_text` path that spans gaps.
- Ctrl+B, Ctrl+I, Ctrl+U surround; Ctrl+Shift+L and Ctrl+Shift+I insert a link or an
  image; both from `active`.
- Search: Ctrl+F opens the footer bar, occurrences highlighted, cycling, close.
- Done when: every branch of `ActiveBlock.qml`'s key handler has a test against
  `active` or `editor`, and a session of writing in foot loses no text.

### Phase 3: checking

- The settle rule: `request_check` runs 400 ms after typing stops. The event loop
  polls with a deadline instead of a Qt timer; `checker_generation` is read each tick
  to pick up results that finished off-thread.
- Lint wash as a background colour on rendered and active blocks alike, the
  `Unchecked` bit kept off code and links as now.
- Footer prompt: message, current suggestion, Ctrl+Enter accepts, Ctrl+Up and
  Ctrl+Down cycle, learn puts a word in the personal dictionary. Y, N and Escape as
  the QML has them.
- `lint::preload` on a thread at startup, before the first frame.
- Done when: a misspelling in `sample/post.md` is washed within half a second of
  typing it and accepted with one key.

### Phase 4: images

- `ratatui-image` with `Picker` from the probe. One `StatefulProtocol` per image
  block, cached by path and modification time, dropped when the block changes.
- Decode on a thread; the block shows alt text until the image is ready, then takes
  its rows. The active block stays anchored on screen when rows above it change, the
  same rule `Main.qml` has for images that load late.
- Height: image pixels over `cell_px` rows, capped at the column width. Half-blocks
  when there is no protocol: two pixels per cell, still capped to the column.
- Missing file: the alt text and the path, muted, as `ImageBlock.qml` shows now.
- Done when: `sample/post.md`'s image shows in foot as sixel, and as a mosaic under
  `TERM=dumb`-style forced fallback.

### Phase 5: ship

- `~/.config/blogawrite/config.toml`: keymap, palette, `content_width`,
  `inherit_background`. Defaults are the current theme.
- Static build with the musl target. `packaging/install.sh` drops its Qt and C++
  checks; `build-appimage.sh` goes.
- Remove the `qt` feature, `document.rs`, `theme.rs`, `cpp/`, `qml/`, `build.rs`.
- README and screenshot.

## Known issues

- Sixel is re-sent whenever its rows are redrawn. `ratatui`'s buffer diff skips an
  image whose cells did not change, so typing near an image is fine; scrolling past a
  big one re-encodes it. Cache the encoded bytes per size. Kitty uploads once and
  places by id, so this cost is foot's alone.
- Sixel is palette-based. Screenshots and diagrams are exact; photographs dither.
- Half-blocks are a mosaic, not a picture. Acceptable per the brief for terminals with
  no protocol.
- A terminal multiplexer breaks both graphics and the pixel query. Out of scope; the
  probe reports half-blocks and carries on.
- The whole-screen palette. Painting the warm light background over a dark terminal
  will jar. `inherit_background` in the config leaves the terminal's own; the wash and
  the accent are picked to read on either.
- Wide characters and combining marks. The layout must go through grapheme clusters
  and `unicode-width` from the start; retrofitting it is painful. `text.rs`'s tests
  for emoji are the model.
- Kitty keyboard protocol left on after a crash leaves the shell with odd keys. Install
  a panic hook that pops the flags and leaves raw mode before printing the panic.
- OSC 52 is silently ignored by a terminal that has it disabled. Nothing to detect;
  document it.
- Hidden structure is what makes WYSIWYG editors hated when it goes wrong. The reveal,
  stepping and Backspace rules above are the whole defence; do not add a construct to
  the mask without them.
- Sized headings, when they come, are the one feature that puts a branch in `layout`.
  Keep that entry point separate so the general path never reads `sized_text`.

## Dependencies

`ratatui`, `crossterm` (kitty keyboard flags), `ratatui-image` + `image`,
`unicode-width`, `unicode-segmentation`, `serde` + `toml` for the config. `harper-core`
and `pulldown-cmark` stay. Everything Qt leaves in Phase 5.
