#!/usr/bin/env bash
# Builds the release quickhop binary for one target triple, checks what systems it runs on, and
# prints its path. The release (scripts/release-local.sh) calls it once per target.
#
# Usage: scripts/release-build.sh TARGET
#   TARGET is x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, aarch64-apple-darwin or
#   x86_64-apple-darwin.
#
# Linux: built natively on a machine of that architecture (gpui links the system's fontconfig,
# wayland and xkbcommon, which a cross linker cannot find); the release builds it inside a
# debian:bullseye container, so the binary needs glibc 2.31 or later. The script refuses a binary
# that asks for a newer glibc symbol than QUICKHOP_GLIBC_FLOOR (default 2.31), because install.sh
# and the docs promise that floor.
#
# macOS: built for the LSMinimumSystemVersion in packaging/macos/Info.plist.in
# (MACOSX_DEPLOYMENT_TARGET), and the binary's LC_BUILD_VERSION minos is checked against it.
set -euo pipefail

target="${1:-}"
if [ "$#" -ne 1 ] || [ -z "$target" ]; then
    echo "usage: $0 TARGET" >&2
    exit 2
fi
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
glibc_floor="${QUICKHOP_GLIBC_FLOOR:-2.31}"

case "$target" in
    x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu)
        host="$(rustc -vV | sed -n 's/^host: //p')"
        if [ "$target" != "$host" ]; then
            echo "release-build: ${target} links gpui's system libraries: build it on a ${target%%-*} Linux machine (this is ${host})" >&2
            exit 1
        fi
        ;;
    aarch64-apple-darwin | x86_64-apple-darwin)
        [ "$(uname -s)" = Darwin ] || {
            echo "release-build: ${target} builds on macOS" >&2
            exit 1
        }
        minimum="$(plutil -extract LSMinimumSystemVersion raw -o - packaging/macos/Info.plist.in)"
        export MACOSX_DEPLOYMENT_TARGET="$minimum"
        echo "release-build: MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET}" >&2
        ;;
    *)
        echo "release-build: unsupported target '${target}'" >&2
        exit 2
        ;;
esac

rustup target add "$target" >&2
cargo build --release --locked --package quickhop --target "$target" >&2

binary="${CARGO_TARGET_DIR:-${root}/target}/${target}/release/quickhop"
[ -f "$binary" ] || {
    echo "release-build: ${binary} was not produced" >&2
    exit 1
}

# True when version $1 (MAJOR.MINOR[.PATCH]) is at most $2.
version_at_most() {
    [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1)" = "$2" ]
}

case "$target" in
    *-linux-gnu)
        needed="$(objdump -T "$binary" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/^GLIBC_//' |
            sort -uV | tail -n 1)"
        [ -n "$needed" ] || {
            echo "release-build: objdump found no GLIBC_ symbol versions in ${binary}" >&2
            exit 1
        }
        if ! version_at_most "$needed" "$glibc_floor"; then
            echo "release-build: ${binary} needs glibc ${needed}, newer than the promised floor ${glibc_floor}; build it on an older base (debian:bullseye)" >&2
            exit 1
        fi
        echo "release-build: ${binary} needs glibc ${needed} (floor ${glibc_floor})" >&2
        ;;
    *-apple-darwin)
        minos="$(otool -l "$binary" |
            awk '$1 == "cmd" { build = ($2 == "LC_BUILD_VERSION") } build && $1 == "minos" { print $2; exit }')"
        if [ "$minos" != "$MACOSX_DEPLOYMENT_TARGET" ]; then
            echo "release-build: ${binary} has minos '${minos}', expected ${MACOSX_DEPLOYMENT_TARGET}" >&2
            exit 1
        fi
        echo "release-build: ${binary} runs on macOS ${minos} and later" >&2
        ;;
esac
echo "$binary"
