# markatui

A Markdown editor for the terminal. You type Markdown and it renders. Keyboard-first with more intuitive and configurable keymap.

Mostly WYSIWYG Markdown - the block under the cursor is shown as raw Markdown in some cases. Includes images.

![The sample document in markatui: the heading under the cursor shows its hash, every other block is rendered, the picture is drawn as sixel, and the checker's suggestion is at the foot of the screen](docs/screenshot.png)

This is [blogawrite](https://github.com/ilia-iliev/blogawrite) without Qt.

```sh
markatui post.md
```

## Keys

```sh
markatui keymap show
```

Keymapping is configurable


Copy/paste uses to the machine's clipboard. 

## Config

`~/.config/markatui/config.toml`, all of it optional. What is below is the default:

```toml
# How wide the column of text is, in cells. 20 to 500.
content_width = 72
# Leave the terminal's own background alone. false paints `paper` and `ink` over it.
inherit_background = true

[palette]
accent = "#3E8E62"      # headings and links
muted = "#8A8378"       # markers, bullets, rules, box drawing
lint = "#F3E4C3"        # the wash under something the checker objects to
lint_ink = "#2D2A26"
code = "#3A3733"        # the ground a fenced block sits on
prompt = "#26241F"      # the band at the foot of the screen
prompt_ink = "#FFFFFF"
paper = "#FAF6EC"       # both only used when inherit_background is false
ink = "#2D2A26"
```

Colours are `"#RRGGBB"`. 

## Spelling and Grammar

American-English spelling and style checking are built in, with a personal dictionary at `~/.config/markatui/dictionary` 

## Terminals

Written against foot; Alacritty and kitty could work too. Everything terminal-specific is 

## Build

```sh
packaging/install.sh
```

Builds and puts the binary in `~/.local/bin`; `PREFIX=/usr/local packaging/install.sh` to put it somewhere else. 

Rust 1.95 or newer

## License

MIT
