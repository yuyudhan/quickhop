#!/usr/bin/env bash
# Clippy with -D warnings for every target triple given (default: the Justfile's list).
#
# Each target is either checked on this machine or deferred, by name, to a machine of its own
# OS, and the run ends with a line saying which is which:
#   - the host and other triples of the host's OS: checked here;
#   - Windows: cross-checked from macOS or Linux with zig (the bundled SQLite is C and its build
#     script runs even for clippy; see scripts/zig-tool.sh) and mingw-w64's windres (gpui embeds
#     its Windows manifest);
#   - Linux from another OS: deferred. gpui's font libraries (fontconfig, freetype) need a Linux
#     sysroot, which macOS does not have;
#   - macOS from another OS: deferred. gpui's Metal shaders are built with the macOS SDK.
# `just verify` on a Mac and on Linux together check every target before a push (GitHub
# Actions is disabled, so nothing lints them after it). A target this script knows no way to
# check, or a missing tool for one it can, fails the run: nothing is skipped without saying so.
set -euo pipefail
cd "$(dirname "$0")/.."

os_of() {
    case "$1" in
        *-apple-darwin) echo macos ;;
        *-linux-*) echo linux ;;
        *-windows-*) echo windows ;;
        *) echo unknown ;;
    esac
}

zig_target_of() {
    case "$1" in
        x86_64-unknown-linux-gnu) echo x86_64-linux-gnu ;;
        aarch64-unknown-linux-gnu) echo aarch64-linux-gnu ;;
        x86_64-pc-windows-gnu) echo x86_64-windows-gnu ;;
        aarch64-apple-darwin) echo aarch64-macos ;;
        x86_64-apple-darwin) echo x86_64-macos ;;
        *) return 1 ;;
    esac
}

need() {
    command -v "$1" >/dev/null || {
        echo "clippy-all: $1 is needed to check $2 ($3)" >&2
        exit 2
    }
}

host=$(rustc -vV | sed -n 's/^host: //p')
host_os=$(os_of "$host")
checked=()
deferred=()
for target in "$@"; do
    target_os=$(os_of "$target")
    # rust-toolchain.toml installs only the host target; add the ones this run checks.
    if [[ "$target_os" == "$host_os" || "$target_os" == windows ]] &&
        ! rustup target list --installed | grep -x "$target" >/dev/null; then
        rustup target add "$target"
    fi
    if [[ "$target" == "$host" ]]; then
        cargo clippy --workspace --all-targets --all-features --target "$target" -- -D warnings
        checked+=("$target")
        continue
    fi
    if [[ "$target_os" != "$host_os" && "$target_os" != windows ]]; then
        case "$target_os" in
            linux) reason="gpui's font libraries need a Linux sysroot" ;;
            macos) reason="gpui's Metal shaders need the macOS SDK" ;;
            *)
                echo "clippy-all: no way known to check $target from $host" >&2
                exit 2
                ;;
        esac
        echo "clippy-all: $target is checked on $target_os, not here: $reason" >&2
        deferred+=("$target")
        continue
    fi
    zig_target=$(zig_target_of "$target") || {
        echo "clippy-all: no zig target known for $target" >&2
        exit 2
    }
    need zig "$target" "brew install zig, or ziglang.org/download"
    if [[ "$target_os" == windows ]]; then
        need x86_64-w64-mingw32-windres "$target" "brew install mingw-w64, or apt install mingw-w64"
    fi
    triple=${target//-/_}
    # No rustc wrapper for cross builds: cargo hands RUSTC_WRAPPER (sccache) to cc-rs, which
    # then runs sccache around CC, and sccache probes the compiler without zig-tool.sh's
    # arguments, so the C build fails before it starts.
    env RUSTC_WRAPPER="" \
        "CC_$triple=$PWD/scripts/zig-tool.sh cc $zig_target" \
        "AR_$triple=$PWD/scripts/zig-tool.sh ar $zig_target" \
        cargo clippy --workspace --all-targets --all-features --target "$target" -- -D warnings
    checked+=("$target")
done
echo "clippy-all: checked here (${host_os}): ${checked[*]:-none}" >&2
if ((${#deferred[@]} > 0)); then
    echo "clippy-all: deferred to their own OS (run just verify there): ${deferred[*]}" >&2
fi
