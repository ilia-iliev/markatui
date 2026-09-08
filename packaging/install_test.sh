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
    set +e
    printf '%b' "$input" | env \
        HOME="$tmp/home" \
        PREFIX="$tmp/prefix" \
        XDG_CONFIG_HOME="$tmp/config" \
        XDG_DATA_HOME="$tmp/data" \
        XDG_MIME_LOG="$tmp/xdg-mime.log" \
        PATH="$tmp/bin:/usr/bin:/bin" \
        "$repo/packaging/install.sh" > "$output" 2>&1
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

# Accepting the destination installs it and asks about the Markdown file association.
run_installer 'y\nn\n' "$tmp/accepted.out"
[ "$status" -eq 0 ] || fail "accepted installation failed"
[ -x "$tmp/prefix/bin/markatui" ] || fail "binary was not installed"
assert_contains "$tmp/accepted.out" "Use markatui as the default application for .md files?"
if [ -e "$tmp/xdg-mime.log" ] && grep -F "default markatui.desktop text/markdown" "$tmp/xdg-mime.log" >/dev/null; then
    fail "declining changed the Markdown default"
fi

# An update does not ask for the already confirmed destination. It can set the default.
: > "$tmp/xdg-mime.log"
run_installer 'y\n' "$tmp/update.out"
[ "$status" -eq 0 ] || fail "update failed"
if grep -F "Install markatui in" "$tmp/update.out" >/dev/null; then
    fail "update asked to confirm its destination"
fi
assert_contains "$tmp/update.out" "Use markatui as the default application for .md files?"
grep -Fx "default markatui.desktop text/markdown" "$tmp/xdg-mime.log" >/dev/null ||
    fail "Markdown default was not set"
desktop="$tmp/data/applications/markatui.desktop"
[ -f "$desktop" ] || fail "desktop entry was not installed"
assert_contains "$desktop" "Exec=$tmp/prefix/bin/markatui %f"
assert_contains "$desktop" "MimeType=text/markdown;"

# Do not ask when markatui is already the Markdown default.
: > "$tmp/xdg-mime.log"
export CURRENT_MARKDOWN_DEFAULT=markatui.desktop
run_installer '' "$tmp/already-default.out"
[ "$status" -eq 0 ] || fail "update with existing Markdown default failed"
if grep -F "Use markatui as the default" "$tmp/already-default.out" >/dev/null; then
    fail "asked to replace an existing markatui Markdown default"
fi
grep -Fx "query default text/markdown" "$tmp/xdg-mime.log" >/dev/null ||
    fail "existing Markdown default was not checked"

printf 'install tests passed\n'
