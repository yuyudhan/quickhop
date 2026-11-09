#!/usr/bin/env bash
# Unsafe code is allowed only in platform crates, `crates/quickhop-platform-*` (FFI to
# AppKit/Accessibility, Wayland, X11). Every other crate root has `#![forbid(unsafe_code)]`
# and no `unsafe` anywhere; platform crate roots must instead carry
# `#![deny(unsafe_op_in_unsafe_fn)]` and `#![deny(clippy::undocumented_unsafe_blocks)]`, so
# every unsafe operation sits in its own block with a `// SAFETY:` comment.
set -euo pipefail
# Runs on the tree it lives in (the repo, or the hooks' snapshot of it), without git.
cd "$(dirname "$0")/.."
platform='^\./crates/quickhop-platform-[a-z0-9-]+/'
status=0
while IFS= read -r -d '' file; do
    [[ "$file" =~ $platform ]] && continue
    if matches=$(grep -nE '(^|[^[:alnum:]_])unsafe[[:space:]]*(\{|fn|impl|extern|trait)' "$file"); then
        echo "unsafe-guard: unsafe code outside a platform crate in $file:" >&2
        echo "$matches" >&2
        status=1
    fi
done < <(find . -path ./target -prune -o -path ./.git -prune -o -name '*.rs' -type f -print0 | sort -z)
for root in crates/*/src/lib.rs crates/*/src/main.rs; do
    [[ -f "$root" ]] || continue
    if [[ "./$root" =~ $platform ]]; then
        for attr in 'unsafe_op_in_unsafe_fn' 'clippy::undocumented_unsafe_blocks'; do
            if ! grep -qF "#![deny($attr)]" "$root"; then
                echo "unsafe-guard: platform crate root $root lacks #![deny($attr)]" >&2
                status=1
            fi
        done
    elif ! grep -q '^#!\[forbid(unsafe_code)\]' "$root"; then
        echo "unsafe-guard: $root lacks #![forbid(unsafe_code)]" >&2
        status=1
    fi
done
if ((status == 0)); then
    echo "unsafe-guard: ok"
fi
exit "$status"
