#!/bin/sh
# Build markatui and put it on the path. Static against musl where that target is
# installed, so the binary is one file that runs on any Linux of the same architecture;
# against this system's libc where it is not.
set -eu

cd "$(dirname "$0")/.."

prefix=${PREFIX:-$HOME/.local}
destination="$prefix/bin/markatui"
target="$(uname -m)-unknown-linux-musl"

confirm() {
    printf '%s [y/N] ' "$1"
    read -r answer || return 1
    case "$answer" in
        y|Y|yes|YES|Yes) return 0 ;;
        *) return 1 ;;
    esac
}

if [ ! -e "$destination" ] && ! confirm "Install markatui in $prefix/bin?"; then
    echo "markatui: installation cancelled"
    exit 1
fi

if rustup target list --installed 2>/dev/null | grep -qx "$target"; then
    cargo build --release --target "$target"
    binary="target/$target/release/markatui"
else
    echo "markatui: $target is not installed, so this build needs the libc it was built"
    echo "markatui: against. \`rustup target add $target\` for one that does not."
    cargo build --release
    binary="target/release/markatui"
fi

install -D -m 755 "$binary" "$destination"
echo "markatui: installed $destination"

config_home=${XDG_CONFIG_HOME:-$HOME/.config}
keymap="$config_home/markatui/keymap.toml"
if [ ! -e "$keymap" ]; then
    install -D -m 644 assets/keymap.toml "$keymap"
    echo "markatui: installed $keymap"
fi

markdown_default=
if command -v xdg-mime >/dev/null 2>&1; then
    markdown_default=$(xdg-mime query default text/markdown) || markdown_default=
fi

if [ "$markdown_default" != markatui.desktop ] &&
    confirm "Use markatui as the default application for .md files?"; then
    if command -v xdg-mime >/dev/null 2>&1; then
        data_home=${XDG_DATA_HOME:-$HOME/.local/share}
        desktop="$data_home/applications/markatui.desktop"
        mkdir -p "$(dirname "$desktop")"
        cat > "$desktop" <<EOF
[Desktop Entry]
Type=Application
Name=markatui
Exec=$destination %f
Terminal=true
MimeType=text/markdown;
EOF
        chmod 644 "$desktop"
        xdg-mime default markatui.desktop text/markdown
        echo "markatui: set as the default application for .md files"
    else
        echo "markatui: xdg-mime is unavailable; the default was not changed" >&2
    fi
fi

case ":$PATH:" in
    *":$prefix/bin:"*) ;;
    *) echo "markatui: $prefix/bin is not on your PATH." ;;
esac
