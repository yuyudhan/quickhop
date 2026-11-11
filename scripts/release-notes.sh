#!/usr/bin/env bash
# Prints the GitHub release notes for version X.Y.Z: the one-line install from docs/install.md
# ("The installer script", so the notes and the docs never differ), links to the install docs
# at the tag, and that version's CHANGELOG.md section. Fails when either is missing, so a
# release cannot go out without them.
#
# Usage: scripts/release-notes.sh X.Y.Z
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
repo="yuyudhan/quickhop"

fail() {
    printf 'release-notes: %s\n' "$*" >&2
    exit 1
}

[ "$#" -eq 1 ] || fail "usage: scripts/release-notes.sh X.Y.Z"
version="$1"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "wants X.Y.Z, got ${version}"

# The indented command line under "## The installer script".
install="$(awk '
    /^## The installer script$/ { section = 1; next }
    section && /^## / { exit }
    section && /^    \(d="\$\(mktemp -d\)"/ { sub(/^    /, ""); print; exit }
' "${root}/docs/install.md")"
[[ "$install" == *"releases/latest/download/install.sh"* ]] ||
    fail "no one-line install under \"## The installer script\" in docs/install.md"

# The body of "## [X.Y.Z] - date", up to the next version heading or the link definitions.
changes="$(awk -v heading="## [${version}]" '
    index($0, heading) == 1 { section = 1; next }
    section && (/^## / || /^\[[^]]+\]: /) { exit }
    section { print }
' "${root}/CHANGELOG.md")"
[ -n "$(printf '%s' "$changes" | tr -d '[:space:]')" ] ||
    fail "CHANGELOG.md has no \"## [${version}]\" section"

docs="https://github.com/${repo}/blob/v${version}/docs"
fence='```'
printf '%s\n' \
    '## Install' \
    '' \
    'macOS (Apple silicon and Intel) and Linux (x86_64 and aarch64, glibc 2.31 or later), in bash or zsh:' \
    '' \
    "${fence}sh" \
    "$install" \
    "$fence" \
    '' \
    'It downloads install.sh into a new temporary folder, runs it from there and removes the folder when it ends. Every asset below has a .sha256, and SHA256SUMS lists them all.' \
    "Other ways to install (the .deb, offline, from source): [docs/install.md](${docs}/install.md); problems: [docs/troubleshooting.md](${docs}/troubleshooting.md)." \
    '' \
    '## Changes' \
    "$changes"
