# markatui

A Markdown editor for the terminal. You type Markdown and it renders. Keyboard-first, one
file per instance, no mouse.

The block under the cursor is raw Markdown. Every other block is drawn as it reads —
headings coloured and ruled, bullets as bullets, quotes with a bar in the gutter, code on
its own ground, tables as tables. Move the cursor into a block and it opens back up into
the source, so what you edit is always the text that is on disk.

This is [blogawrite](https://github.com/ilia-iliev/blogawrite) without Qt. Same block
model, same keys, same checker, same file-in file-out guarantee: the bytes you did not
touch come back byte for byte, blank lines and all.

```sh
markatui post.md
```

## Keys

| | |
|---|---|
| `ctrl+s` | save |
| `ctrl+q` | quit — asks first if there is unsaved work |
| `ctrl+z` | undo |
| `ctrl+a` | select the whole document |
| `ctrl+c` | copy the selection (OSC 52) |
| `ctrl+f` | find a word; `ctrl+↑` `ctrl+↓` walk the occurrences, `esc` closes |
| `ctrl+b` `ctrl+i` `ctrl+u` | bold, italic, strikethrough |
| `ctrl+shift+l` `ctrl+shift+i` | insert a link, insert an image |
| `ctrl+↑` `ctrl+↓` | walk the checker's suggestions |
| `ctrl+enter` | take the suggestion on show |
| `ctrl+shift+enter` | take the word into your dictionary |
| `enter` twice | end the block |
| `ctrl+←` `ctrl+→` | a word at a time |
| `pgup` `pgdn` | a screenful at a time |

Shift with any movement key draws a selection, across blocks as well as inside one.

## Spelling and grammar

American-English spelling and style checking are built in, with a personal dictionary at
`~/.config/markatui/dictionary` — one word per line, `#` for a comment. A word or a phrase
the checker objects to gets a wash behind it; the foot of the screen says what is wrong
and what to put there instead.

Nothing is marked in a block while you are typing in it. The checker has its say once you
pause.

## Terminals

Written against foot; Alacritty and kitty work too. Everything terminal-specific is
decided once at startup in `tui::probe`, and the rest of the editor reads that answer
rather than asking which terminal it is in.

Where the kitty keyboard protocol is not answered, `ctrl+i` arrives as Tab and
`ctrl+enter` as Enter, so italics and accepting a suggestion are lost. Copy goes through
OSC 52, which a terminal with it turned off ignores silently.

## Build

```sh
cargo build --release
install -Dm755 target/release/markatui ~/.local/bin/markatui
```

Rust 1.95 or newer. No C++, no Qt, no system libraries.

## Not there yet

Images draw as a line naming the file rather than as a picture; the graphics protocols are
the next piece of work. There is no config file, so the palette, the column width and the
keymap are what the source says they are. See `docs/tui-plan.md`.

## License

MIT
