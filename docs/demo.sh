#!/usr/bin/env bash
# Film the sample document for the README: a headless Sway, foot inside it, and a frame
# grabbed after every keystroke. Nothing here belongs to this machine, so the CI workflow
# makes the same film and the README stays in step with the editor.
#
#     ./docs/demo.sh [out.gif]
#
# Needs sway, foot, grim, wtype, jq, ffmpeg and a monospace font.
set -euo pipefail

cd "$(dirname "$0")/.."
out=${1:-docs/demo.gif}
document=sample/demo.md
# The letter the sample's typo is missing. The typo is the first word of its line, so the
# cursor walking down the first column stands in it and the checker says so; the film
# stops if the walk goes by without a word from the checker.
letter=g
# The screen the editor is drawn on, in characters. Wide enough for the text column,
# tall enough for the sample document and the checker's line under it.
size=90x31
# A cap on the walk, not its length: the cursor is walked down until the document ends,
# so a longer or shorter sample film themselves.
cap=40
# How long a frame is held, in seconds. A letter of the command goes by at typing speed,
# a block is held long enough to read, and the ends of the film are held a beat longer.
typing=0.09
reading=0.55
mending=0.9
holding=1.3

cargo build --release

work=$(mktemp -d)
chmod 700 "$work"
sway=
# Where the frames and the compositor's log are left when something goes wrong. A film
# made somewhere nobody can watch it — CI — is otherwise impossible to argue with.
keep_frames=${KEEP_FRAMES:-}
# The compositor started here is the only one spoken to: an inherited SWAYSOCK would
# address the one the writer is sitting in, and telling that to exit logs them out.
cleanup() {
    [ -n "$sway" ] && kill "$sway" 2>/dev/null
    wait 2>/dev/null || true
    if [ -n "$keep_frames" ]; then
        mkdir -p "$keep_frames"
        cp "$work"/frame-*.png "$work"/film.txt "$work"/sway.log "$keep_frames/" 2>/dev/null || true
    fi
    rm -rf "$work"
}
trap cleanup EXIT

# The writer's own settings stay out of the picture, and so does the cursor the editor
# remembers for this file: a film shot here must not depend on where it was last left.
# The theme is markatui's own dark one, not whatever the terminal happened to be wearing.
mkdir -p "$work/config/markatui"
# The heading is the editor's own name, lower case and meant to stay that way, so the
# check that would have it in title case is turned off and the name is a word the
# dictionary knows. What is left for the checker to find is the typo further down.
cat > "$work/config/markatui/config.toml" <<CONF
theme = "dark"

[checks]
UseTitleCase = false
CONF
printf 'markatui\n' > "$work/config/markatui/dictionary"
# The film opens on the heading. Left to itself the editor opens a file where it was
# last left, which here is nowhere, and that is its end.
mkdir -p "$work/state/markatui"
printf '0\t%s\n' "$PWD/$document" > "$work/state/markatui/cursors"

# A shell rather than the editor: the film opens on an empty terminal and the editor is
# started from it, the way anyone would start it.
cat > "$work/sway.conf" <<CONF
output HEADLESS-1 resolution 1920x1440
default_border none
for_window [app_id="markatui-shot"] floating enable
exec env PS1='\$ ' PATH="$PWD/target/release:$PATH" \
    foot --app-id=markatui-shot --font="monospace:size=11" --window-size-chars=$size \
    -- bash --norc --noprofile
CONF

env -u WAYLAND_DISPLAY -u SWAYSOCK -u DISPLAY \
    XDG_RUNTIME_DIR="$work" XDG_CONFIG_HOME="$work/config" XDG_STATE_HOME="$work/state" \
    WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
    sway -c "$work/sway.conf" > "$work/sway.log" 2>&1 &
sway=$!

export XDG_RUNTIME_DIR="$work"
export WAYLAND_DISPLAY=wayland-1
window='.. | objects | select(.app_id? == "markatui-shot")
    | "\(.rect.x),\(.rect.y) \(.rect.width)x\(.rect.height)"'
for _ in $(seq 60); do
    export SWAYSOCK=$(echo "$work"/sway-ipc.*.sock)
    geometry=$(swaymsg -t get_tree 2>/dev/null | jq -r "$window" || true)
    [ -n "$geometry" ] && break
    sleep 0.25
done

die() {
    echo "$1" >&2
    cat "$work/sway.log" >&2
    exit 1
}

[ -n "${geometry:-}" ] || die "the terminal never opened"

# The foot of the screen, one row of it: the checker's line is the only thing drawn
# there, and it is empty until the cursor stands in a word the dictionary does not know.
IFS=', x' read -r x y width height <<< "$geometry"
row=$((height / ${size#*x}))
band="$x,$((y + height - row)) ${width}x$row"

# What is on the screen now, as far as the film knows. A frame is a copy of it, taken
# once the editor has finished answering a key rather than on a clock: a picture is read
# off the event loop and drawn a frame or two behind the text it sits under, and a grab
# taken in between belongs to neither. Two identical grabs in a row are the screen
# holding still.
screen=$work/screen.png
settle() {
    local before=$work/before.png
    rm -f "$before"
    for _ in $(seq 40); do
        sleep 0.15
        grim -g "$geometry" "$screen"
        cmp -s "$screen" "$before" && return
        cp "$screen" "$before"
    done
}

# Send a key and wait for what it draws, or answer that the screen never moved. A virtual
# keyboard's press can be delivered before the compositor has given it the focus, and a
# press nobody received is a step the film skips without saying so — which is the whole of
# the difference between one run and the next — so a key that moves nothing is sent again,
# and a few times over: a press is lost outright rather than delivered late, and the one
# after it usually lands.
press() {
    local held=$work/held.png
    cp "$screen" "$held"
    for _ in $(seq 8); do
        # A press is sent with a moment on either side of it: a virtual keyboard that is
        # torn down in the same breath as the press can take the press down with it.
        wtype -s 50 "$@" -s 150
        for _ in $(seq 8); do
            sleep 0.15
            grim -g "$geometry" "$screen"
            cmp -s "$screen" "$held" || { settle; return 0; }
        done
    done
    return 1
}

# A key that is not in the film and does not have to move anything: Home puts the caret on
# the left edge whether or not it was already there. Sent twice over rather than made to
# prove it moved — the same caret either way, and two chances at a key the compositor
# might drop.
nudge() {
    wtype -s 50 "$@" -s 150
    wtype -s 50 "$@" -s 150
    settle
}

# The film is a list of stills with a time against each: a letter of a command and a
# block of prose are not worth the same time on the screen.
frames=0
list=$work/film.txt
keep() {
    local shot
    shot=$(printf '%s/frame-%03d.png' "$work" "$frames")
    cp "$screen" "$shot"
    printf "file '%s'\nduration %s\n" "$shot" "$1" >> "$list"
    frames=$((frames + 1))
}
show() {
    local held=$1
    shift
    press "$@" && keep "$held"
}

# The opening shot waits for the terminal to be there at all: a terminal that has not
# started holds as still as an empty one.
sleep 2
settle
keep "$holding"

# The editor is started the way anyone starts it, a letter at a time.
command="markatui $document"
for ((at = 0; at < ${#command}; at++)); do
    show "$typing" "${command:at:1}" || die "the terminal would not take the command"
done
# The frame the Return itself draws is a shell that has taken a command and an editor
# that is not up yet, which is nobody's screen: the terminal has to draw the editor, and
# the picture in the document is read and drawn after the text is.
press -k Return || die "the command never ran"
sleep 1
settle
keep "$holding"

# The caret goes to the left edge before the walk starts, and is not in the film: Down
# keeps the column it set out from, and a walk down the first column is a walk through the
# first word of every row — which is where the sample keeps its typo.
nudge -k Home

# The checker's line with nothing in it, to tell the frames where it speaks from the ones
# where it says nothing. Taken on the heading, which it has no opinion about.
quiet=$work/quiet.png
grim -g "$band" "$quiet"

# The walk: a row at a time down the document, a frame to a row. Every block turns to raw
# source as the cursor arrives and renders again as it leaves, which is the whole of what
# there is to see here. The checker speaks where the cursor stands in a word it does not
# know, so the foot of the screen is read after every row: a row it has something to say
# about is the row with the typo in it.
step=0
mended=
while [ "$step" -lt "$cap" ]; do
    # A Down that moves nothing is the end of the document, and the end of the walk.
    show "$reading" -k Down || break
    step=$((step + 1))
    [ -n "$mended" ] && continue
    grim -g "$band" "$work/band.png"
    cmp -s "$work/band.png" "$quiet" && continue
    # The checker has spoken, so the cursor is standing in the misspelt word: the end of
    # it is a word to the right, and the letter that was dropped goes in there.
    show "$mending" -M ctrl -k Right -m ctrl || die "the caret would not cross the typo"
    show "$mending" "$letter" || die "the typo would not take the letter"
    mended=yes
done
[ -n "$mended" ] || die "the checker never spoke: the typo in $document was not found"

# The walk ends where the document does — but the last Down may have had nothing left to
# move, the cursor already sitting at the end of the last block, and how far a cursor gets
# on the last line of a document is the terminal's business as much as the editor's. So
# the end is asked for outright, and kept as a frame only where it turns out to be
# somewhere the walk had not already reached.
show "$reading" -M ctrl -k End -m ctrl || true

# Ctrl+Home carries the cursor back to the top, which is where the walk began: the film
# closes on the document it opened, and the last frame is held a beat.
show "$holding" -M ctrl -k Home -m ctrl || die "the cursor would not come back to the top"

# The last frame again, without a time against it: that is how a list of stills says it
# has come to an end.
printf "file '%s'\n" "$(printf '%s/frame-%03d.png' "$work" "$((frames - 1))")" >> "$list"

# One palette for the whole film, so the picture in the document does not shift colour
# from frame to frame.
ffmpeg -loglevel error -y -f concat -safe 0 -i "$list" \
    -filter_complex '[0:v] split [a][b];[a] palettegen [p];[b][p] paletteuse' \
    -loop 0 "$out"
echo "$out"
