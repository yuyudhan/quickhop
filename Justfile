set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["bash", "-euo", "pipefail", "-c"]

# Targets clippy runs for; each machine checks those it can and names the rest (scripts/clippy-all.sh).
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

# Clippy with -D warnings for every target triple this OS can check; prints what it deferred.
clippy-all:
    scripts/clippy-all.sh {{targets}}

# Shell scripts: shellcheck, then the installer's end-to-end test (skipped on Windows).
shell:
    #!/usr/bin/env bash
    set -euo pipefail
    case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) echo "shell: skipped on Windows"; exit 0 ;; esac
    shellcheck scripts/*.sh .githooks/*
    scripts/test-install.sh

# The everyday gate (pre-commit, CI): format, guards, spelling, shell, host clippy, tests.
check: fmt-check guards typos shell clippy test

# Unit, integration, property, snapshot and CLI end-to-end tests.
test:
    cargo test --workspace --all-features

# Licences, advisories, duplicate dependencies, sources.
deny:
    cargo deny check

# The full local gate (pre-push): check, clippy-all (needs zig and mingw-w64), deny.
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
