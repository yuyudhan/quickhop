#!/usr/bin/env bash
# The declared rust-version must build the workspace, and must not fall below the toolchain the
# pinned zed revision (gpui) is built with: moving the zed pin can raise the floor
# (CONTRIBUTING.md, "Minimum Rust version"). `just release` runs it before building.
#
# Usage: scripts/msrv.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

msrv="$(sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml)"
[ -n "$msrv" ] || {
    echo "msrv: no rust-version in Cargo.toml" >&2
    exit 1
}
echo "msrv: rust-version ${msrv}" >&2
rustup toolchain install "$msrv" --profile minimal >&2
cargo +"$msrv" check --locked --workspace

rev="$(sed -n 's/^gpui = .*rev = "\([0-9a-f]*\)".*/\1/p' Cargo.toml)"
[ -n "$rev" ] || {
    echo "msrv: no pinned gpui rev in Cargo.toml" >&2
    exit 1
}
checkouts="${CARGO_HOME:-${HOME}/.cargo}/git/checkouts"
file="$(find "$checkouts" -path "*/zed-*/${rev:0:7}/rust-toolchain.toml" -print -quit)"
[ -n "$file" ] || {
    echo "msrv: no rust-toolchain.toml for zed ${rev:0:7} under ${checkouts}" >&2
    exit 1
}
zed="$(sed -n 's/^channel = "\([0-9]*\.[0-9]*\).*"$/\1/p' "$file")"
echo "msrv: rust-version ${msrv}, zed ${rev:0:7} builds with ${zed}" >&2
lowest="$(printf '%s\n' "$msrv" "$zed" | sort -V | head -n 1)"
if [ "$lowest" != "$zed" ]; then
    echo "msrv: rust-version ${msrv} is below ${zed}, the toolchain of the pinned zed revision" >&2
    exit 1
fi
