#!/usr/bin/env bash
# End-to-end test of scripts/dev-config.sh in a throwaway HOME: installs into
# ~/.config/quickhop-dev (and $XDG_CONFIG_HOME/quickhop-dev), keeps edited files without
# --force, backs them up and replaces them with it, never writes ~/.config/quickhop, and
# refuses unknown arguments.
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
script="$repo/scripts/dev-config.sh"
sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT

fail() {
    echo "test-dev-config: $*" >&2
    exit 1
}

run() {
    env -u XDG_CONFIG_HOME HOME="$sandbox/home" "$script" "$@"
}

mkdir -p "$sandbox/home"
target="$sandbox/home/.config/quickhop-dev"

run >"$sandbox/first.log"
for file in config.toml snippets.toml scripts-extra/dev-hello.sh; do
    cmp -s "$repo/config/dev/$file" "$target/$file" || fail "$file was not installed"
done
[[ -x "$target/scripts-extra/dev-hello.sh" ]] || fail "the sample script is not executable"
[[ -d "$target/scripts" ]] || fail "scripts/ was not created"
[[ ! -e "$sandbox/home/.config/quickhop" ]] || fail "the release directory was written"

echo "# mine" >>"$target/config.toml"
mine=$(cat "$target/config.toml")
run >"$sandbox/second.log"
[[ "$(cat "$target/config.toml")" == "$mine" ]] || fail "an existing config.toml was overwritten"
grep -q "^kept $target/config.toml" "$sandbox/second.log" || fail "no 'kept' line: $(cat "$sandbox/second.log")"
if compgen -G "$target/config.toml.bak-*" >/dev/null; then
    fail "a backup was made without --force"
fi

run --force >"$sandbox/forced.log"
cmp -s "$repo/config/dev/config.toml" "$target/config.toml" || fail "--force did not replace config.toml"
backups=("$target"/config.toml.bak-*)
[[ ${#backups[@]} -eq 1 && -f "${backups[0]}" ]] || fail "expected one backup, got: ${backups[*]}"
[[ "$(cat "${backups[0]}")" == "$mine" ]] || fail "the backup does not hold the replaced file"

XDG_CONFIG_HOME="$sandbox/xdg" HOME="$sandbox/home" "$script" >/dev/null
[[ -f "$sandbox/xdg/quickhop-dev/config.toml" ]] || fail "XDG_CONFIG_HOME was not followed"

if run --frobnicate 2>/dev/null; then
    fail "an unknown argument was accepted"
fi
echo "test-dev-config: ok"
