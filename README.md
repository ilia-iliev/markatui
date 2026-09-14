# markatui

A Markdown editor for the terminal. Markdown is rendered whilst typing. Keyboard-first with configurable keymap with defaults that work like traditional editors (unlike vim).

Mostly WYSIWYG - the block under the cursor is shown as raw Markdown in some cases. Renders images if the terminal supports it. Linux

![markatui started from a terminal and the cursor walking down the sample document: each block shows its raw Markdown as the cursor arrives and renders again as it leaves, the checker names a misspelt word at the foot of the screen and the letter it is missing is typed in, and the picture is drawn by the terminal](docs/demo.gif)

## Install

A static binary is attached to each [release](https://github.com/ilia-iliev/markatui/releases) - download it, unpack it, put it on your PATH.

From a checkout, the installer builds a release binary, puts it in `~/.local/bin` and registers its desktop entry:

```sh
./packaging/install.sh
```

Building from source needs Rust 1.95 or newer.

`mrk` is installed beside `markatui` as a link to it, so `mrk README.md` is the short way in. Either name works.

## Keys

```sh
markatui -keymap                  # list them
markatui -keymap bold_selection ctrl+b
markatui -keymap default          # put them all back
```

The defaults are conventional: Ctrl+S saves, Ctrl+Z undoes, Ctrl+B bolds. Alt+Up and Alt+Down move the section under the cursor - the list item and whatever is nested under it, the table row, the line of code - and the whole block where its lines have nowhere left to go. Ctrl+K on a link follows it instead of making one, handing it to `xdg-open`. Ctrl+R switches to reading mode, where the block under the cursor renders like every other one and the checker stays quiet. Copy/paste uses the machine's clipboard.

## Terminals

I use foot, and I have verified markatui works on alacritty and kitty. All three answer the kitty keyboard protocol.

On a terminal that does not - gnome-terminal, xterm, Konsole, the VS Code terminal - there might be keymapping issues.

For everything else, bind the command to a key your terminal can send:

```sh
markatui -keymap heading_1 alt+1
markatui -keymap accept_suggestion alt+enter
```

Under tmux, ask for extended keys and the protocol comes through:

```tmux
set -g extended-keys on
set -s extended-keys-format csi-u
```

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

The editor also keeps notes to itself in `~/.local/state/markatui`: the block the cursor was left in per file, and the mode the editor was last closed in.

## Spelling and Grammar

American-English spelling and style checking are built in through Harper, with a personal dictionary at `~/.config/markatui/dictionary`

Alt+G turns off the check under the cursor. To see the whole list, or to turn one off without waiting for it to fire:

```sh
markatui -checks
markatui -checks off SentenceCapitalization
markatui -checks on SentenceCapitalization
```

## Images

Images are rendered if the terminal allows it. Otherwise, a dummy mosaic is used. Images can also be pasted from clipboard - this will create an image file and the document will reference it.

## License

MIT
