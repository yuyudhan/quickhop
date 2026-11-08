#!/usr/bin/env bash
# The hooks check the working tree, so it must equal the tree being committed or pushed:
# fails on unstaged changes to tracked files and on untracked, unignored files.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
mode="${1:-commit}"
status=0
if ! git diff --quiet; then
    echo "hook: unstaged changes would not be part of this $mode; stage or drop them:" >&2
    git diff --name-only >&2
    status=1
fi
if [[ "$mode" == push ]] && ! git diff --cached --quiet; then
    echo "hook: staged but uncommitted changes; commit them before pushing:" >&2
    git diff --cached --name-only >&2
    status=1
fi
untracked=$(git ls-files --others --exclude-standard)
if [[ -n "$untracked" ]]; then
    echo "hook: untracked files would be checked but not included; add or ignore them:" >&2
    echo "$untracked" >&2
    status=1
fi
exit "$status"
