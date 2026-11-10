#!/usr/bin/env bash
# Builds the macOS app bundle: the quickhop binary as its executable (the command line and the
# app are one program; opened without arguments it is the app, see crates/quickhop/src/app.rs),
# packaging/macos/Info.plist.in with the binary's version, and AppIcon.icns rendered from
# packaging/macos/quickhop.svg. The bundle is signed ad hoc (no Developer ID yet): Apple
# silicon runs only signed code, and the signature seals the Info.plist and the icon to the
# executable. Prints the bundle's path.
#
# The binary decides which app it becomes (crates/quickhop-core/src/flavour.rs): a release
# (`quickhop --version` prints "quickhop X.Y.Z") is quickhop.app, io.github.yuyudhan.quickhop;
# a development build ("quickhop X.Y.Z (dev)": a debug build, or one with the `dev` feature)
# is "quickhop Dev.app", io.github.yuyudhan.quickhop.dev, so macOS keeps its Accessibility
# grant apart from the release's and rebuilding it never invalidates the release's.
#
# Usage: scripts/bundle-macos.sh [--dev | --bin PATH] [--out DIR]
#   --dev  build the development flavour (release profile, `--features dev`) and bundle it
#   --bin  the binary to bundle (default: the release build, built first with cargo)
#   --out  the directory the app goes to, replacing one of the same name there
#          (default: target/bundle)
# Needs macOS 14 or later (AppKit renders the SVG icon) and the Command Line Tools (swift,
# iconutil, codesign).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin="" out="${root}/target/bundle" dev=false
while [ "$#" -gt 0 ]; do
    case "$1" in
    --dev)
        dev=true
        shift
        ;;
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
[ "$dev" = false ] || [ -z "$bin" ] || {
    echo "bundle-macos: --dev builds the binary itself; with --bin the binary decides" >&2
    exit 2
}
if [ -z "$bin" ]; then
    if [ "$dev" = true ]; then
        (cd "$root" && cargo build --release --locked -p quickhop --features dev)
    else
        (cd "$root" && cargo build --release --locked -p quickhop)
    fi
    bin="${root}/target/release/quickhop"
fi
[ -f "$bin" ] || {
    echo "bundle-macos: --bin ${bin}: no such file" >&2
    exit 2
}

# `quickhop --version` prints "quickhop X.Y.Z", and " (dev)" after it for a development build.
version="$("$bin" --version)"
version="${version#quickhop }"
flavour=release name=quickhop bundle_id=io.github.yuyudhan.quickhop
if [[ "$version" == *" (dev)" ]]; then
    version="${version% (dev)}"
    flavour=dev name="quickhop Dev" bundle_id=io.github.yuyudhan.quickhop.dev
fi
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
    echo "bundle-macos: ${bin} --version printed no X.Y.Z version: ${version}" >&2
    exit 2
}

mkdir -p "$out"
out="$(cd "$out" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="${work}/${name}.app"
mkdir -p "${stage}/Contents/MacOS" "${stage}/Contents/Resources"
install -m 755 "$bin" "${stage}/Contents/MacOS/quickhop"
plist="${stage}/Contents/Info.plist"
sed -e "s/@VERSION@/${version}/g" "${root}/packaging/macos/Info.plist.in" >"$plist"
if [ "$flavour" = dev ]; then
    for key in CFBundleName CFBundleDisplayName; do
        plutil -replace "$key" -string "$name" "$plist"
    done
    plutil -replace CFBundleIdentifier -string "$bundle_id" "$plist"
fi
plutil -lint -s "$plist"
swift "${root}/scripts/render-icon.swift" "${root}/packaging/macos/quickhop.svg" "${work}/AppIcon.iconset"
iconutil -c icns -o "${stage}/Contents/Resources/AppIcon.icns" "${work}/AppIcon.iconset"
[ "$(plutil -extract CFBundleIdentifier raw -o - "$plist")" = "$bundle_id" ] || {
    echo "bundle-macos: ${plist} does not hold the ${flavour} identifier ${bundle_id}" >&2
    exit 1
}
codesign --force --sign - --identifier "$bundle_id" "$stage"
codesign --verify --deep --strict "$stage"

app="${out}/${name}.app"
rm -rf "$app"
mv "$stage" "$app"
echo "$app"
