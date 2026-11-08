#!/usr/bin/env bash
# Clippy with -D warnings for every target triple given (default: the Justfile's list).
# Cross targets need `rustup target add <triple>` and `zig` (see scripts/zig-tool.sh): the
# bundled SQLite is C and its build script runs even for clippy.
set -euo pipefail
cd "$(dirname "$0")/.."
host=$(rustc -vV | sed -n 's/^host: //p')
for target in "$@"; do
    if [[ "$target" != "$host" ]]; then
        case "$target" in
            x86_64-unknown-linux-gnu) zig_target=x86_64-linux-gnu ;;
            x86_64-pc-windows-gnu) zig_target=x86_64-windows-gnu ;;
            aarch64-apple-darwin) zig_target=aarch64-macos ;;
            x86_64-apple-darwin) zig_target=x86_64-macos ;;
            *)
                echo "clippy-all: no zig target known for $target" >&2
                exit 2
                ;;
        esac
        command -v zig >/dev/null || { echo "clippy-all: zig is needed to cross-check $target" >&2; exit 2; }
        triple=${target//-/_}
        export "CC_$triple=$PWD/scripts/zig-tool.sh cc $zig_target"
        export "AR_$triple=$PWD/scripts/zig-tool.sh ar $zig_target"
    fi
    cargo clippy --workspace --all-targets --all-features --target "$target" -- -D warnings
done
