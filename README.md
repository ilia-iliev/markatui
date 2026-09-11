# markatui

A Markdown editor for the terminal. Markdown is rendered whilst typing. Keyboard-first with configurable keymap with defaults that work like traditional editors (unlike vim).

Mostly WYSIWYG - the block under the cursor is shown as raw Markdown in some cases. Renders images if the terminal supports it.

![markatui started from a terminal and the cursor walking down the sample document: each block shows its raw Markdown as the cursor arrives and renders again as it leaves, the checker names a misspelt word at the foot of the screen and the letter it is missing is typed in, and the picture is drawn by the terminal](docs/demo.gif)

This is based off [blogawrite](https://github.com/ilia-iliev/blogawrite) - similar concept, but a standalone editor.

Linux only. The installer, the desktop entry and following a link all go through Linux tooling.

## Install

A static binary is attached to each [release](https://github.com/ilia-iliev/markatui/releases) - download it, unpack it, put it on your PATH.

From a checkout, the installer builds a release binary, puts it in `~/.local/bin` and registers its desktop entry:

```sh
./packaging/install.sh
```

Building from source needs Rust 1.95 or newer.

## Keys

```sh
markatui -keymap                  # list them
markatui -keymap bold_selection ctrl+b
markatui -keymap default          # put them all back
```

The defaults are conventional: Ctrl+S saves, Ctrl+Z undoes, Ctrl+B bolds. Ctrl+K on a link follows it instead of making one, handing it to `xdg-open`. Ctrl+R switches to reading mode, where the block under the cursor renders like every other one and the checker stays quiet. Copy/paste uses the machine's clipboard.

**Most of the defaults need a terminal that speaks the kitty keyboard protocol.** See below.

## Terminals

I use foot, and I have verified markatui works on alacritty and kitty. All three answer the kitty keyboard protocol, which is what lets a terminal tell Ctrl+Shift+B from Ctrl+B, Ctrl+Enter from Enter, and Ctrl+I from Tab.

On a terminal that does not - gnome-terminal, xterm, Konsole, the VS Code terminal, tmux without extended keys - those keys collapse:

| Default | Arrives as | So |
| --- | --- | --- |
| Ctrl+Shift+*letter* | Ctrl+*letter* | the block commands, the alignments and insert-image are unreachable |
| Ctrl+*digit* | usually nothing | the headings are unreachable |
| Ctrl+Enter | Enter | accept-suggestion is unreachable |
| Ctrl+Shift+Enter | Enter | learn-word is unreachable |
| Ctrl+I | Tab | italics indent instead |

Enter is the one case the editor works around: where it cannot tell Shift+Enter from Enter, the first press leaves a line break and the second ends the block.

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

The editor also keeps notes to itself in `~/.local/state/markatui`: the block the cursor was left in per file, and the mode the editor was last closed in. Nothing there is yours to edit, and removing it loses nothing but the cursor.

## Spelling and Grammar

American-English spelling and style checking are built in through Harper, with a personal dictionary at `~/.config/markatui/dictionary`

Alt+G turns off the check under the cursor. To see the whole list, or to turn one off without waiting for it to fire:

```sh
markatui -checks
markatui -checks off SentenceCapitalization
markatui -checks on SentenceCapitalization
```

## Images

Images are rendered if the terminal allows it. Otherwise, a dummy box is used. Images can also be pasted from clipboard - this will create an image file and the document will reference it.

`MARKATUI_PICTURES=halfblocks` forces the mosaic, which is the only way to see the fallback on a terminal that has a protocol of its own.

## Things to know

These are deliberate. If they bother you, say so in an issue rather than a bug report.

- **An inline image is hoisted into a paragraph of its own when the file opens.** That is the only place a terminal can draw it. It rewrites the file, so such a document opens dirty.
- **Esc quits**, and so do Ctrl+Q and Ctrl+D. A document with unsaved work asks first.
- **Ctrl+C copies.** Nothing interrupts the editor; use Esc.
- **Ctrl+S saves whether anything changed or not.**
- **A file that would not open is never saved over.** The empty document you see is the failure, not the file.
- **A file that changed on disk since you opened it stops the first save.** The editor says so; save again and it goes over it.
- **A document reached through a symlink is saved through it**, so the link survives.

## License

MIT
