# markatui

A Markdown editor for the terminal. Markdown is rendered whilst typing. Keyboard-first with configurable keymap with defaults that work like traditional editors (unlike vim).

Mostly WYSIWYG - the block under the cursor is shown as raw Markdown in some cases. Renders images if the terminal supports it.

![markatui started from a terminal and the cursor walking down the sample document: each block shows its raw Markdown as the cursor arrives and renders again as it leaves, the checker names a misspelt word at the foot of the screen and the letter it is missing is typed in, and the picture is drawn by the terminal](docs/demo.gif)

This is based off [blogawrite](https://github.com/ilia-iliev/blogawrite) - similar concept, but a standalone editor.

## Keys

```sh
markatui -keymap
```

Most keys are configurable. The defaults are conventional.

Ctrl+K on a link follows it instead, handing it to `xdg-open`.

Copy/paste uses the machine's clipboard.

## Mouse

Markatui supports clicking and dragging with the mouse. I'm primarily a keyboard user, so there could be rough edges. 

## Config

`~/.config/markatui/config.toml` To choose a preset:

```sh
markatui theme light
markatui theme dark
markatui theme terminal  # inherit the terminal
```

The choice is written to the config file. `markatui -config` opens that file in the editor; what it says is read at the next start, and `markatui -config default` resets to default.

## Spelling and Grammar

American-English spelling and style checking are built in through Harper, with a personal dictionary at `~/.config/markatui/dictionary`

To turn off types of checks, use `alt+g`

## Terminals
I use foot and I have verified markatui works on alacritty and kitty.

## Images

Images are rendered if the terminal allows it. Otherwise, a dummy box is used. Images can also be pasted from clipboard - this will create an image file and the document will reference it. 

## Install from source (Linux)

Rust 1.95 or newer is required. From a checkout, the installer builds a release binary,
puts it in `~/.local/bin`, and registers its desktop entry:

```sh
./packaging/install.sh
```

## License

MIT
