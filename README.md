# markatui

A Markdown editor for the terminal. Markdown is rendered whilst typing. Keyboard-first with configurable keymap with defaults that work like traditional editors (not vim).

Mostly WYSIWYG - the block under the cursor is shown as raw Markdown in some cases. Renders images if the terminal supports it.

![The sample document in markatui: the heading under the cursor shows its hash, every other block is rendered, the picture is drawn as sixel, and the checker's suggestion is at the foot of the screen](docs/screenshot.png)

This is based off [blogawrite](https://github.com/ilia-iliev/blogawrite) - similar concept, but a standalone editor, not tui.

## Keys

```sh
markatui -keymap
```

Keymap is configurable


Copy/paste uses to the machine's clipboard. 

## Config

`~/.config/markatui/config.toml`, all of it optional. `markatui -config` opens it in the editor; what it says is read at the next start. What is below is the default:

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

Colours are `"#RRGGBB"`. Checks turned off go in a `[checks]` table; see below. 

## Spelling and Grammar

American-English spelling and style checking are built in, with a personal dictionary at `~/.config/markatui/dictionary`

Grammar can be turned off, default is `ctrl+g`. The mode you are in — plain, grammar off, or reading — is picked up again next time; a fresh install starts plain. 

### Turning off one check

Stand on a tip you never want to see again and press `alt+g`. It names the check and asks; `y` puts it in `[checks]` in your config, where it stays off.

By hand, or to put one back:

```sh
markatui -checks                         # every check and what it looks for
markatui -checks | grep -i 'title case'  # find the one you are after
markatui -checks off UseTitleCase
markatui -checks on UseTitleCase
```

## Terminals

Written against foot; Alacritty and kitty could work too. 

## Build

```sh
packaging/install.sh
```

Builds and puts the binary in `~/.local/bin`; `PREFIX=/usr/local packaging/install.sh` to put it somewhere else. 

Rust 1.95 or newer

## License

MIT
