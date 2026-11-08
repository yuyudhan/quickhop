#!/usr/bin/env bash
# Fails when `unsafe` code appears in any crate, or when a crate root lacks
# `#![forbid(unsafe_code)]`. A future sys crate gets a narrow, listed exception here.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
allowed='^$'
status=0
while IFS= read -r -d '' file; do
    [[ "$file" =~ $allowed ]] && continue
    if matches=$(grep -nE '(^|[^[:alnum:]_])unsafe[[:space:]]*(\{|fn|impl|extern|trait)' "$file"); then
        echo "unsafe-guard: unsafe code in $file:" >&2
        echo "$matches" >&2
        status=1
    fi
done < <(git ls-files -z --cached --others --exclude-standard -- '*.rs')
for root in crates/*/src/lib.rs crates/*/src/main.rs; do
    [[ -f "$root" ]] || continue
    if ! grep -q '^#!\[forbid(unsafe_code)\]' "$root"; then
        echo "unsafe-guard: $root lacks #![forbid(unsafe_code)]" >&2
        status=1
    fi
done
if ((status == 0)); then
    echo "unsafe-guard: ok"
fi
exit "$status"
