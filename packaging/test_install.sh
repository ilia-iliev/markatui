#!/bin/sh
set -eu

source_repo=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

repo="$tmp/repo"
mkdir -p "$repo/packaging" "$repo/assets" "$tmp/bin"
cp "$source_repo/packaging/install.sh" "$repo/packaging/install.sh"
cp "$source_repo/assets/keymap.toml" "$repo/assets/keymap.toml"
cat > "$tmp/bin/rustup" <<'EOF'
#!/bin/sh
printf '%s-unknown-linux-musl\n' "$(uname -m)"
EOF
cat > "$tmp/bin/cargo" <<'EOF'
#!/bin/sh
case " $* " in
    *" --target "*) output="target/$(uname -m)-unknown-linux-musl/release/markatui" ;;
    *) output="target/release/markatui" ;;
esac
mkdir -p "$(dirname "$output")"
cp /bin/true "$output"
EOF
cat > "$tmp/bin/xdg-mime" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >> "$XDG_MIME_LOG"
if [ "$1 $2" = "query default" ]; then
    printf '%s\n' "${CURRENT_MARKDOWN_DEFAULT:-other.desktop}"
fi
EOF
chmod +x "$tmp/bin/rustup" "$tmp/bin/cargo" "$tmp/bin/xdg-mime"

run_installer() {
    input=$1
    output=$2
    shift 2
    set +e
    printf '%b' "$input" | env \
        HOME="$tmp/home" \
        PREFIX="${INSTALL_PREFIX:-$tmp/prefix}" \
        XDG_CONFIG_HOME="$tmp/config" \
        XDG_DATA_HOME="$tmp/data" \
        XDG_MIME_LOG="$tmp/xdg-mime.log" \
        PATH="$tmp/bin:/usr/bin:/bin" \
        "$repo/packaging/install.sh" "$@" > "$output" 2>&1
    status=$?
    set -e
}

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    exit 1
}

assert_contains() {
    grep -F "$2" "$1" >/dev/null || fail "expected '$2' in $1"
}

# A first installation confirms its destination before building or copying anything.
run_installer 'n\n' "$tmp/refused.out"
[ "$status" -ne 0 ] || fail "refusing a first installation should stop"
assert_contains "$tmp/refused.out" "Install markatui in $tmp/prefix/bin?"
[ ! -e "$tmp/prefix/bin/markatui" ] || fail "binary was installed after refusal"

# Accepting the destination installs it, registers the application, and asks before
# changing the Markdown default.
run_installer 'y\nn\n' "$tmp/accepted.out"
[ "$status" -eq 0 ] || fail "accepted installation failed"
[ -x "$tmp/prefix/bin/markatui" ] || fail "binary was not installed"
[ "$(readlink "$tmp/prefix/bin/mrk")" = markatui ] || fail "mrk was not linked to markatui"
[ -x "$tmp/prefix/bin/mrk" ] || fail "mrk does not resolve to the installed binary"
assert_contains "$tmp/accepted.out" "Use markatui as the default application for .md files?"
if [ -e "$tmp/xdg-mime.log" ] && grep -F "default markatui.desktop text/markdown" "$tmp/xdg-mime.log" >/dev/null; then
    fail "declining changed the Markdown default"
fi
desktop="$tmp/data/applications/markatui.desktop"
[ -f "$desktop" ] || fail "desktop entry was not installed"
assert_contains "$desktop" "Exec=\"$tmp/prefix/bin/markatui\" %f"
assert_contains "$desktop" "MimeType=text/markdown;"
[ ! -e "$tmp/config/markatui/keymap.toml" ] || fail "installer wrote application-owned config"

# A mrk that belongs to something else is somebody's command, not a stale link of ours.
rm "$tmp/prefix/bin/mrk"
cp /bin/true "$tmp/prefix/bin/mrk"
run_installer 'y\nn\n' "$tmp/short-taken.out"
[ "$status" -eq 0 ] || fail "install beside an occupied mrk failed"
[ ! -L "$tmp/prefix/bin/mrk" ] || fail "replaced an unrelated mrk"
assert_contains "$tmp/short-taken.out" "$tmp/prefix/bin/mrk is already something else"
rm "$tmp/prefix/bin/mrk"

# An existing command is never overwritten without confirmation.
cp "$tmp/prefix/bin/markatui" "$tmp/before-refusal"
run_installer 'n\n' "$tmp/update-refused.out"
[ "$status" -ne 0 ] || fail "refusing an update should stop"
cmp "$tmp/before-refusal" "$tmp/prefix/bin/markatui" >/dev/null ||
    fail "existing binary changed after refusal"
assert_contains "$tmp/update-refused.out" "Replace $tmp/prefix/bin/markatui?"

# An accepted update can set the default.
: > "$tmp/xdg-mime.log"
run_installer 'y\ny\n' "$tmp/update.out"
[ "$status" -eq 0 ] || fail "update failed"
assert_contains "$tmp/update.out" "Use markatui as the default application for .md files?"
grep -Fx "default markatui.desktop text/markdown" "$tmp/xdg-mime.log" >/dev/null ||
    fail "Markdown default was not set"

# -y accepts the installation and default-application questions without prompting.
: > "$tmp/xdg-mime.log"
INSTALL_PREFIX="$tmp/unattended prefix"
export INSTALL_PREFIX
run_installer '' "$tmp/unattended.out" -y
[ "$status" -eq 0 ] || fail "unattended installation failed"
[ -x "$INSTALL_PREFIX/bin/markatui" ] || fail "unattended installation missed the binary"
if grep -F "[y/N]" "$tmp/unattended.out" >/dev/null; then
    fail "unattended installation prompted"
fi
grep -Fx "default markatui.desktop text/markdown" "$tmp/xdg-mime.log" >/dev/null ||
    fail "unattended installation did not set the Markdown default"

# Register a changed, safely quoted command even when markatui is already the default.
: > "$tmp/xdg-mime.log"
export CURRENT_MARKDOWN_DEFAULT=markatui.desktop
INSTALL_PREFIX="$tmp/new prefix"
export INSTALL_PREFIX
run_installer 'y\n' "$tmp/already-default.out"
[ "$status" -eq 0 ] || fail "install at changed prefix failed"
if grep -F "Use markatui as the default" "$tmp/already-default.out" >/dev/null; then
    fail "asked to replace an existing markatui Markdown default"
fi
assert_contains "$desktop" "Exec=\"$INSTALL_PREFIX/bin/markatui\" %f"
grep -Fx "query default text/markdown" "$tmp/xdg-mime.log" >/dev/null ||
    fail "existing Markdown default was not checked"

# A stowed entry is a symlink to a dotfiles repository; writing to the path would
# follow it and rewrite that repository's file.
dotfiles="$tmp/dotfiles/markatui.desktop"
mkdir -p "$(dirname "$dotfiles")"
cat > "$dotfiles" <<'ENTRY'
[Desktop Entry]
Type=Application
Name=Markatui
Exec=foot -e markatui %f
MimeType=text/markdown;
ENTRY
before=$(cat "$dotfiles")
rm -f "$desktop"
ln -s "$dotfiles" "$desktop"
run_installer 'y\n' "$tmp/stowed.out"
[ "$status" -eq 0 ] || fail "install over a stowed desktop entry failed"
[ -L "$desktop" ] || fail "replaced the stowed desktop entry symlink"
[ "$(cat "$dotfiles")" = "$before" ] || fail "wrote through the stowed desktop entry"

printf 'install tests passed\n'
