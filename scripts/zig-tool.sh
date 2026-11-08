#!/usr/bin/env bash
# Cross C toolchain for `just clippy-all`: rusqlite's bundled SQLite is C, so clippy for
# another target triple must compile it. `zig cc` ships the libc headers for every target.
#
# Usage (set by the Justfile): CC_<triple>="scripts/zig-tool.sh cc <zig-target>",
# AR_<triple>="scripts/zig-tool.sh ar <zig-target>".
# cc-rs adds clang's `--target=<rust triple>`, which zig does not understand; it is dropped
# because <zig-target> already says the same.
set -euo pipefail
tool=$1
target=$2
shift 2
case "$tool" in
    cc)
        args=()
        for arg in "$@"; do
            [[ "$arg" == --target=* ]] || args+=("$arg")
        done
        exec zig cc -target "$target" "${args[@]}"
        ;;
    ar) exec zig ar "$@" ;;
    *)
        echo "zig-tool: unknown tool $tool" >&2
        exit 2
        ;;
esac
