#!/usr/bin/env bash
# Builds quickhop.app, the macOS app bundle: the quickhop binary as its executable (the
# command line and the app are one program; opened without arguments it is the app, see
# crates/quickhop/src/app.rs), packaging/macos/Info.plist.in with the binary's version, and
# AppIcon.icns rendered from packaging/macos/quickhop.svg. The bundle is signed ad hoc (no
# Developer ID yet): Apple silicon runs only signed code, and the signature seals the
# Info.plist and the icon to the executable. Prints the bundle's path.
#
# Usage: scripts/bundle-macos.sh [--bin PATH] [--out DIR]
#   --bin  the binary to bundle (default: the release build, built first with cargo)
#   --out  the directory quickhop.app goes to, replacing one there (default: target/bundle)
# Needs macOS 14 or later (AppKit renders the SVG icon) and the Command Line Tools (swift,
# iconutil, codesign).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin="" out="${root}/target/bundle"
while [ "$#" -gt 0 ]; do
    case "$1" in
    --bin | --out)
        [ "$#" -ge 2 ] || {
            echo "bundle-macos: $1 needs a value" >&2
            exit 2
        }
        case "$1" in
        --bin) bin="$2" ;;
        --out) out="$2" ;;
        esac
        shift 2
        ;;
    *)
        echo "bundle-macos: unknown option $1" >&2
        exit 2
        ;;
    esac
done
[ "$(uname -s)" = Darwin ] || {
    echo "bundle-macos: builds a macOS app, run it on macOS" >&2
    exit 2
}
if [ -z "$bin" ]; then
    (cd "$root" && cargo build --release --locked -p quickhop)
    bin="${root}/target/release/quickhop"
fi
[ -f "$bin" ] || {
    echo "bundle-macos: --bin ${bin}: no such file" >&2
    exit 2
}

# `quickhop --version` prints "quickhop X.Y.Z".
version="$("$bin" --version)"
version="${version#quickhop }"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
    echo "bundle-macos: ${bin} --version printed no X.Y.Z version: ${version}" >&2
    exit 2
}

mkdir -p "$out"
out="$(cd "$out" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="${work}/quickhop.app"
mkdir -p "${stage}/Contents/MacOS" "${stage}/Contents/Resources"
install -m 755 "$bin" "${stage}/Contents/MacOS/quickhop"
plist="${stage}/Contents/Info.plist"
sed -e "s/@VERSION@/${version}/g" "${root}/packaging/macos/Info.plist.in" >"$plist"
plutil -lint -s "$plist"
swift "${root}/scripts/render-icon.swift" "${root}/packaging/macos/quickhop.svg" "${work}/AppIcon.iconset"
iconutil -c icns -o "${stage}/Contents/Resources/AppIcon.icns" "${work}/AppIcon.iconset"
bundle_id="$(plutil -extract CFBundleIdentifier raw -o - "$plist")"
codesign --force --sign - --identifier "$bundle_id" "$stage"
codesign --verify --deep --strict "$stage"

app="${out}/quickhop.app"
rm -rf "$app"
mv "$stage" "$app"
echo "$app"
