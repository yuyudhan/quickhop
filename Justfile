set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["bash", "-euo", "pipefail", "-c"]

# Targets clippy runs for; cross targets need `rustup target add` and zig (no linker).
targets := "x86_64-unknown-linux-gnu x86_64-pc-windows-gnu aarch64-apple-darwin"

default:
    @just --list

# Format Rust and TOML in place.
fmt:
    cargo fmt --all
    taplo fmt

# Check formatting without changing files.
fmt-check:
    cargo fmt --all -- --check
    taplo fmt --check

# Line-count (3000) and unsafe guards.
guards:
    scripts/check-line-count.sh
    scripts/check-unsafe.sh

# Spelling.
typos:
    typos

# Clippy with -D warnings for the host only.
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Clippy with -D warnings for every target triple.
clippy-all:
    scripts/clippy-all.sh {{targets}}

# The everyday gate (pre-commit, CI): format, guards, spelling, host clippy, tests.
check: fmt-check guards typos clippy test

# Unit, integration, property, snapshot and CLI end-to-end tests.
test:
    cargo test --workspace --all-features

# Licences, advisories, duplicate dependencies, sources.
deny:
    cargo deny check

# The full local gate (pre-push): check, clippy for every target (needs zig), deny.
verify: check clippy-all deny

# Linux .deb and tarball in target/bundle/ (needs dpkg-dev).
bundle-linux:
    scripts/bundle-linux.sh

# Install the .deb in a clean Debian container and run it (needs docker).
bundle-linux-test: bundle-linux
    scripts/bundle-linux-test.sh

# Use the repo's git hooks.
install-hooks:
    git config core.hooksPath .githooks
