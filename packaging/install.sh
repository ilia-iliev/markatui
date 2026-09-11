#!/bin/sh
# Build markatui and install it for the current Linux user. Use a statically linked
# musl binary when the matching Rust target is available; otherwise use the native target.
set -eu

assume_yes=false
case ${1-} in
    -y) assume_yes=true; shift ;;
    '') ;;
    *) echo "Usage: $0 [-y]" >&2; exit 2 ;;
esac
if [ "$#" -ne 0 ]; then
    echo "Usage: $0 [-y]" >&2
    exit 2
fi

cd "$(dirname "$0")/.."

if [ "$(uname -s)" != Linux ]; then
    echo "markatui: this installer supports Linux only" >&2
    exit 1
fi

prefix=${PREFIX:-$HOME/.local}
destination="$prefix/bin/markatui"
target="$(uname -m)-unknown-linux-musl"

confirm() {
    if [ "$assume_yes" = true ]; then
        return 0
    fi
    printf '%s [y/N] ' "$1"
    read -r answer || return 1
    case "$answer" in
        y|Y|yes|YES|Yes) return 0 ;;
        *) return 1 ;;
    esac
}

if [ -e "$destination" ] || [ -L "$destination" ]; then
    question="Replace $destination?"
else
    question="Install markatui in $prefix/bin?"
fi
if ! confirm "$question"; then
    echo "markatui: installation cancelled"
    exit 1
fi

if rustup target list --installed 2>/dev/null | grep -qx "$target"; then
    cargo build --release --target "$target"
    binary="target/$target/release/markatui"
else
    cargo build --release
    binary="target/release/markatui"
fi

install -D -m 755 "$binary" "$destination"
echo "markatui: installed $destination"

# Cargo keeps every artifact it has ever built, so a repeatedly rebuilt target
# directory grows without bound. Hold it to a budget, discarding least recent first.
# A budget rather than an age: artifacts of daily builds are never old enough to expire.
if command -v cargo-sweep >/dev/null 2>&1; then
    cargo sweep --maxsize "${CARGO_SWEEP_MAXSIZE:-4000}"
else
    echo "markatui: cargo-sweep is unavailable; stale build artifacts were not pruned" >&2
fi

# Register the application independently of whether the user makes it their default.
# An Exec argument has its own quoting rules: quote the path, escape its reserved
# characters, and double percent signs so they are not interpreted as field codes.
desktop_exec=$(printf '%s' "$destination" | sed \
    -e 's/\\/\\\\\\\\/g' \
    -e 's/"/\\"/g' \
    -e 's/`/\\`/g' \
    -e 's/\$/\\$/g' \
    -e 's/%/%%/g')
data_home=${XDG_DATA_HOME:-$HOME/.local/share}
desktop="$data_home/applications/markatui.desktop"
# A symlink here belongs to whatever put it there, usually a dotfiles repository.
# Writing to the path would follow it and edit that repository's file instead.
if [ -L "$desktop" ]; then
    echo "markatui: $desktop is a symlink; left it for its owner to maintain"
else
    mkdir -p "$(dirname "$desktop")"
    cat > "$desktop" <<EOF
[Desktop Entry]
Type=Application
Name=markatui
Exec="$desktop_exec" %f
TryExec=$destination
Terminal=true
MimeType=text/markdown;
EOF
    chmod 644 "$desktop"
    echo "markatui: installed $desktop"
fi

if command -v xdg-mime >/dev/null 2>&1; then
    markdown_default=$(xdg-mime query default text/markdown) || markdown_default=
    if [ "$markdown_default" != markatui.desktop ] &&
        confirm "Use markatui as the default application for .md files?"; then
        xdg-mime default markatui.desktop text/markdown
        echo "markatui: set as the default application for .md files"
    fi
else
    echo "markatui: xdg-mime is unavailable; the Markdown default was not changed" >&2
fi

# What is installed above is a copy, and a copy is what an install should be: it owes
# nothing to the checkout it came from. That leaves it as it is until this script runs
# again, which is a poor fit for the machine markatui is written on, where the editor in
# use should be the last one built. A link instead of the copy gives that, and gives up
# an install that survives the build directory being cleaned or the checkout moving.
#
# The link is to the plain release target rather than the static one installed above:
# that is what `cargo build --release` writes, so an ordinary rebuild is live at once.
if confirm "Link $destination to the build, so rebuilds are picked up?"; then
    cargo build --release
    ln -sfn "$PWD/target/release/markatui" "$destination"
    echo "markatui: linked $destination to $PWD/target/release/markatui"
fi

case ":$PATH:" in
    *":$prefix/bin:"*) ;;
    *) echo "markatui: $prefix/bin is not on your PATH." ;;
esac
