#!/usr/bin/env bash
# Fails when any Rust source file (outside target/) exceeds the line limit (default 3000).
set -euo pipefail
limit="${1:-3000}"
# Runs on the tree it lives in (the repo, or the hooks' snapshot of it), without git.
cd "$(dirname "$0")/.."
status=0
while IFS= read -r -d '' file; do
    lines=$(wc -l <"$file")
    if ((lines > limit)); then
        echo "line-count: $file has $lines lines (limit $limit)" >&2
        status=1
    fi
done < <(find . -path ./target -prune -o -path ./.git -prune -o -name '*.rs' -type f -print0 | sort -z)
if ((status == 0)); then
    echo "line-count: ok (limit $limit)"
fi
exit "$status"
