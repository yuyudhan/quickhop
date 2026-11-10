#!/usr/bin/env bash
# Packages one built quickhop binary as the release assets scripts/install.sh installs:
# OUT/quickhop-TARGET.tar.gz, its .sha256 ("<hash>  <name>", as sha256sum writes it) and a copy
# of install.sh. The archive holds quickhop-TARGET/ with the binary, the licences and the
# README; Linux targets add the desktop entry and icon under share/, and --app adds a macOS app
# bundle as quickhop.app.
#
# Usage: scripts/dist.sh --target TRIPLE --bin PATH [--app PATH] [--out DIR]
#   TRIPLE is x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu or universal-apple-darwin;
#   OUT defaults to target/dist.
#
# universal-apple-darwin is the one archive install.sh gives every Mac, Apple silicon and Intel,
# so its binary (and the app's executable) must be a universal Mach-O holding arm64 and x86_64:
# dist.sh refuses anything else (`lipo -archs`, macOS only), as a single-architecture build
# would fail on the other kind of Mac with "Bad CPU type in executable". The release workflow
# builds both and joins them with `lipo -create`.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
target="" bin="" app="" out="${root}/target/dist"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --target | --bin | --app | --out)
            [ "$#" -ge 2 ] || { echo "dist: $1 needs a value" >&2; exit 2; }
            case "$1" in
                --target) target="$2" ;;
                --bin) bin="$2" ;;
                --app) app="$2" ;;
                --out) out="$2" ;;
            esac
            shift 2
            ;;
        *) echo "dist: unknown option $1" >&2; exit 2 ;;
    esac
done
case "$target" in
    x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu) linux=true ;;
    universal-apple-darwin) linux=false ;;
    *) echo "dist: --target must be a supported triple, got '${target}'" >&2; exit 2 ;;
esac
[ -f "$bin" ] || { echo "dist: --bin ${bin}: no such file" >&2; exit 2; }
if [ -n "$app" ]; then
    [ "$linux" = false ] || { echo "dist: --app is for universal-apple-darwin" >&2; exit 2; }
    [ -d "$app" ] || { echo "dist: --app ${app}: not a directory" >&2; exit 2; }
fi

# Refuses a macOS executable that is not universal (arm64 and x86_64).
require_universal() {
    local file="$1" archs
    command -v lipo >/dev/null 2>&1 || {
        echo "dist: universal-apple-darwin needs lipo (macOS) to check ${file} holds arm64 and x86_64" >&2
        exit 2
    }
    archs=" $(lipo -archs "$file" 2>/dev/null || true) "
    if [[ "$archs" != *" arm64 "* ]] || [[ "$archs" != *" x86_64 "* ]]; then
        echo "dist: ${file} is not a universal binary (lipo -archs:${archs% }); build aarch64-apple-darwin and x86_64-apple-darwin and join them with lipo -create" >&2
        exit 2
    fi
}
if [ "$linux" = false ]; then
    require_universal "$bin"
    [ -z "$app" ] || require_universal "${app}/Contents/MacOS/quickhop"
fi

mkdir -p "$out"
out="$(cd "$out" && pwd)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
pkg="${stage}/quickhop-${target}"
mkdir -p "$pkg"
install -m 755 "$bin" "${pkg}/quickhop"
install -m 644 "${root}/LICENSE-MIT" "${root}/LICENSE-APACHE" "${root}/README.md" "$pkg/"
if [ "$linux" = true ]; then
    mkdir -p "${pkg}/share/applications" "${pkg}/share/icons/hicolor/scalable/apps"
    install -m 644 "${root}/packaging/linux/quickhop.desktop" "${pkg}/share/applications/"
    install -m 644 "${root}/packaging/linux/quickhop.svg" "${pkg}/share/icons/hicolor/scalable/apps/"
fi
[ -z "$app" ] || cp -R "$app" "${pkg}/quickhop.app"

archive="quickhop-${target}.tar.gz"
tar -C "$stage" -czf "${out}/${archive}" "quickhop-${target}"
(
    cd "$out"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$archive" >"${archive}.sha256"
    else
        shasum -a 256 "$archive" >"${archive}.sha256"
    fi
)
install -m 755 "${root}/scripts/install.sh" "${out}/install.sh"
echo "${out}/${archive}"
