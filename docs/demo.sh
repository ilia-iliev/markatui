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
# The screen the editor is drawn on, in characters. Wide enough for the text column,
# tall enough for the sample document and the checker's line under it.
size=90x31
# A cap on the walk, not its length: the cursor is walked down until the document ends,
# so a longer or shorter sample film themselves. Every block turns to raw source as the
# cursor arrives and renders again as it leaves, which is the whole of what there is to
# see here.
cap=40
# How long a frame is held, in hundredths of a second — slow enough to read a block by.
delay=40

cargo build --release

work=$(mktemp -d)
chmod 700 "$work"
sway=
# Where the frames and the compositor's log are left when something goes wrong. A film
# made somewhere nobody can watch it — CI — is otherwise impossible to argue with.
keep=${KEEP_FRAMES:-}
# The compositor started here is the only one spoken to: an inherited SWAYSOCK would
# address the one the writer is sitting in, and telling that to exit logs them out.
cleanup() {
    [ -n "$sway" ] && kill "$sway" 2>/dev/null
    wait 2>/dev/null || true
    if [ -n "$keep" ]; then
        mkdir -p "$keep"
        cp "$work"/frame-*.png "$work"/sway.log "$keep/" 2>/dev/null || true
    fi
    rm -rf "$work"
}
trap cleanup EXIT

# The writer's own settings stay out of the picture, and so does the cursor the editor
# remembers for this file: a film shot here must not depend on where it was last left.
# The theme is markatui's own dark one, not whatever the terminal happened to be wearing.
mkdir -p "$work/config/markatui"
printf 'theme = "dark"\n' > "$work/config/markatui/config.toml"
# The editor's own name is a word here, as it would be for anyone who writes about it.
printf 'markatui\n' > "$work/config/markatui/dictionary"
# The film opens on the heading. Left to itself the editor opens a file where it was
# last left, which here is nowhere, and that is its end.
mkdir -p "$work/state/markatui"
printf '0\t%s\n' "$PWD/sample/post.md" > "$work/state/markatui/cursors"

cat > "$work/sway.conf" <<CONF
output HEADLESS-1 resolution 1920x1440
default_border none
for_window [app_id="markatui-shot"] floating enable
exec foot --app-id=markatui-shot --font="monospace:size=11" --window-size-chars=$size \
    $PWD/target/release/markatui sample/post.md
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
if [ -z "${geometry:-}" ]; then
    echo "the terminal never opened" >&2
    cat "$work/sway.log" >&2
    exit 1
fi

# A frame is taken once the editor has finished answering the key rather than on a clock:
# a picture is read off the event loop and drawn a frame or two behind the text it sits
# under, and a frame grabbed in between belongs to neither. Two identical grabs in a row
# are the screen holding still.
still() {
    local shot=$1 previous=$work/previous.png
    rm -f "$previous"
    for _ in $(seq 40); do
        sleep 0.15
        grim -g "$geometry" "$shot"
        cmp -s "$shot" "$previous" && return
        cp "$shot" "$previous"
    done
}

# Send a key and take the frame it draws, or answer that the screen never moved. A
# virtual keyboard's press can be delivered before the compositor has given it the focus,
# and a press nobody received is a step the film skips without saying so — which is the
# whole of the difference between one run and the next — so a key that moves nothing is
# sent again, and a few times over: a press is lost outright rather than delivered late,
# and the one after it usually lands. The key is several words for a chord, and is meant
# to be split into them.
press() {
    local key=$1 shot=$2 before=$3
    for _ in $(seq 8); do
        # A press is sent with a moment on either side of it: a virtual keyboard that is
        # torn down in the same breath as the press can take the press down with it.
        # shellcheck disable=SC2086
        wtype -s 50 $key -s 150
        for _ in $(seq 8); do
            sleep 0.15
            grim -g "$geometry" "$shot"
            cmp -s "$shot" "$before" || { still "$shot"; return 0; }
        done
    done
    return 1
}

# The opening shot waits for the editor to be there at all: an empty terminal holds as
# still as a finished one.
sleep 2
previous=$work/frame-000.png
still "$previous"
step=0
while [ "$step" -lt "$cap" ]; do
    shot=$(printf '%s/frame-%03d.png' "$work" "$((step + 1))")
    # A Down that moves nothing is the end of the document, and the end of the walk.
    press "-k Down" "$shot" "$previous" || break
    previous=$shot
    step=$((step + 1))
done

# The walk ends where the document does — but the last Down may have had nothing left to
# move, the cursor already sitting at the end of the last block, and how far a cursor gets
# on the last line of a document is the terminal's business as much as the editor's. So
# the end is asked for outright, and kept as a frame only where it turns out to be
# somewhere the walk had not already reached.
end=$(printf '%s/frame-%03d.png' "$work" "$((step + 1))")
if press "-M ctrl -k End -m ctrl" "$end" "$previous"; then
    previous=$end
    step=$((step + 1))
else
    rm -f "$end"
fi

# Ctrl+Home carries the cursor back to the top, which is where the film began: the loop
# closes on the first frame instead of cutting to it.
last=$(printf '%s/frame-%03d.png' "$work" "$((step + 1))")
press "-M ctrl -k Home -m ctrl" "$last" "$previous" || {
    echo "the cursor would not come back to the top" >&2
    cat "$work/sway.log" >&2
    exit 1
}
# The last frame is held a beat, so the heading is read before the walk starts again.
cp "$last" "$(printf '%s/frame-%03d.png' "$work" "$((step + 2))")"

# One palette for the whole film, so the picture in the document does not shift colour
# from frame to frame.
ffmpeg -loglevel error -y -framerate "100/$delay" \
    -i "$work/frame-%03d.png" \
    -filter_complex '[0:v] split [a][b];[a] palettegen [p];[b][p] paletteuse' \
    -loop 0 "$out"
echo "$out"
