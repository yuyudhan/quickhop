#!/usr/bin/env bash
# Installs the development configuration (config/dev/: config.toml, snippets.toml and the
# sample script in scripts-extra/) into the directory development builds read:
# $XDG_CONFIG_HOME/quickhop-dev, by default ~/.config/quickhop-dev. A release's
# ~/.config/quickhop is never touched.
#
# Files already there are kept, unless --force: then each is first copied to
# <file>.bak-<UTC timestamp> and replaced. Afterwards the installed config is checked with
# the development binary when one is built (target/debug/quickhop).
#
# Usage: scripts/dev-config.sh [--force]
set -euo pipefail

usage() {
    echo "usage: $0 [--force]" >&2
    exit 2
}

force=false
case "$#:${1:-}" in
0:) ;;
1:--force) force=true ;;
*) usage ;;
esac

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source_dir="$repo/config/dev"
config_home=${XDG_CONFIG_HOME:-}
if [[ -z "$config_home" ]]; then
    if [[ -z "${HOME:-}" ]]; then
        echo "dev-config: neither XDG_CONFIG_HOME nor HOME is set" >&2
        exit 1
    fi
    config_home="$HOME/.config"
fi
target="$config_home/quickhop-dev"
stamp=$(date -u +%Y%m%dT%H%M%SZ)

# Installs $source_dir/$1 as $target/$1 (mode $2).
install_one() {
    local name=$1 mode=$2
    local from="$source_dir/$name" to="$target/$name"
    mkdir -p "$(dirname "$to")"
    if [[ -e "$to" ]]; then
        if ! $force; then
            echo "kept $to (--force replaces it, keeping a backup)"
            return
        fi
        cp -p "$to" "$to.bak-$stamp"
        echo "backed up $to to $to.bak-$stamp"
    fi
    install -m "$mode" "$from" "$to"
    echo "installed $to"
}

install_one config.toml 644
install_one snippets.toml 644
install_one scripts-extra/dev-hello.sh 755
mkdir -p "$target/scripts"

binary="$repo/target/debug/quickhop"
if [[ -x "$binary" ]]; then
    QUICKHOP_CONFIG="$target/config.toml" "$binary" config check
else
    echo "check it once built: cargo build -p quickhop && target/debug/quickhop config check"
fi
