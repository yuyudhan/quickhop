#!/usr/bin/env bash
# Completes a release asset directory: checks that every archive the installer and Homebrew
# point at is there (universal macOS, Linux x86_64 and aarch64, the two .deb packages), adds
# install.sh, renders the Homebrew formula and cask (scripts/homebrew.sh), gives every asset a
# .sha256, writes SHA256SUMS over them and checks it.
#
# Usage: scripts/release-finish.sh VERSION DIST
set -euo pipefail

[ "$#" -eq 2 ] || {
    echo "usage: $0 VERSION DIST" >&2
    exit 2
}
version="$1"
dist="$2"
root="$(cd "$(dirname "$0")/.." && pwd)"

missing=0
for asset in quickhop-universal-apple-darwin.tar.gz quickhop-x86_64-unknown-linux-gnu.tar.gz \
    quickhop-aarch64-unknown-linux-gnu.tar.gz "quickhop_${version}_amd64.deb" \
    "quickhop_${version}_arm64.deb"; do
    if [ ! -s "${dist}/${asset}" ]; then
        echo "release-finish: ${dist}/${asset} is missing or empty" >&2
        missing=1
    fi
done
[ "$missing" -eq 0 ] || exit 1

install -m 755 "${root}/scripts/install.sh" "${dist}/install.sh"
"${root}/scripts/homebrew.sh" "$version" "$dist" >/dev/null

(
    cd "$dist"
    sum() {
        if command -v sha256sum >/dev/null 2>&1; then sha256sum "$@"; else shasum -a 256 "$@"; fi
    }
    for file in *; do
        case "$file" in
            *.sha256 | SHA256SUMS) ;;
            # dist.sh wrote the archives' own; a mismatch means a corrupted transfer.
            *) [ -f "${file}.sha256" ] || sum "$file" >"${file}.sha256" ;;
        esac
    done
    cat ./*.sha256 >SHA256SUMS
    sum --check --quiet SHA256SUMS
)
echo "release-finish: ${dist}/SHA256SUMS ($(wc -l <"${dist}/SHA256SUMS" | tr -d ' ') assets)" >&2
