#!/usr/bin/env bash
# Tests scripts/install-from-source.sh against stand-in tools: each case builds a PATH of stubs
# (uname, rustc, cargo, rustup, xcode-select, xcrun, cc, git, df) plus the few real utilities
# the script uses, runs it in an empty environment (env -i) with a throwaway HOME, and checks
# its exit status, its message and whether cargo was started. Also checks that the script's
# MIN_RUST matches the workspace rust-version.
#
# Usage: scripts/test-install-from-source.sh
# The stubs are shell source written literally: their $1 and ${FAKE_*} expand when they run.
# shellcheck disable=SC2016
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
script="$root/scripts/install-from-source.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0
pass() {
    echo "ok   - $*"
}
fail() {
    echo "FAIL - $*"
    failures=$((failures + 1))
}

# The real utilities the script needs, and nothing else from the system.
base="$work/base"
mkdir -p "$base"
for tool in bash sh env sed tr cut dirname awk cat; do
    ln -s "$(command -v "$tool")" "$base/$tool"
done

# stub NAME BODY: writes an executable stub into the current case's stub directory.
stub() {
    printf '#!/bin/sh\n%s\n' "$2" >"$stubs/$1"
    chmod +x "$stubs/$1"
}

# A case starts with every tool present and working: macOS arm64, Rust 1.99.1 from rustup,
# the Command Line Tools, plenty of disk, a reachable repository.
new_case() {
    case_dir="$work/case$((++cases))"
    stubs="$case_dir/bin"
    home="$case_dir/home"
    mkdir -p "$stubs" "$home"
    stub uname 'case "$1" in -s) echo "${FAKE_OS:-Darwin}" ;; -m) echo "${FAKE_CPU:-arm64}" ;; esac'
    stub rustc 'echo "rustc ${FAKE_RUST:-1.99.1} (58cf8f9c3 2026-10-13)"'
    stub cargo 'echo "$*" >>"$HOME/cargo.log"'
    stub rustup 'exit 0'
    stub xcode-select '[ "$1" = -p ] && echo /Library/Developer/CommandLineTools'
    stub xcrun 'echo /Library/Developer/CommandLineTools/SDKs/MacOSX.sdk'
    stub cc 'exit 0'
    stub git '[ "$1" = ls-remote ] && exit "${FAKE_GIT:-0}"; exit 0'
    stub df 'echo "Filesystem 1024-blocks Used Available Capacity Mounted"; echo "/dev/disk3 999 1 ${FAKE_FREE_KB:-104857600} 1% /"'
}

# run [VAR=value ...] -- [ARGS ...]: the script in an empty environment; sets $out and $status.
run() {
    local vars=()
    while (($#)) && [ "$1" != -- ]; do
        vars+=("$1")
        shift
    done
    shift
    status=0
    out="$(env -i HOME="$home" PATH="$stubs:$base" TERM=dumb ${vars[@]+"${vars[@]}"} \
        "$base/bash" "$script" "$@" 2>&1)" || status=$?
}

cargo_ran() {
    [ -s "$home/cargo.log" ]
}

# expect_stop NAME TEXT: the run stopped (exit 1) before cargo, saying TEXT.
expect_stop() {
    if [ "$status" -eq 1 ] && [[ "$out" == *"$2"* ]] && ! cargo_ran; then
        pass "$1"
    else
        fail "$1 (status $status, cargo $(cargo_ran && echo ran || echo idle)): $out"
    fi
}

cases=0

min_rust="$(sed -n 's/^MIN_RUST="\(.*\)"$/\1/p' "$script")"
declared="$(sed -n 's/^rust-version = "\(.*\)"$/\1/p' "$root/Cargo.toml")"
if [ -n "$min_rust" ] && [ "$min_rust" = "$declared" ]; then
    pass "MIN_RUST ($min_rust) matches the workspace rust-version"
else
    fail "MIN_RUST ($min_rust) differs from the workspace rust-version ($declared)"
fi

new_case
run -- --branch nightly
if [ "$status" -eq 0 ] && [ "$(cat "$home/cargo.log")" = "install --locked --git https://github.com/yuyudhan/quickhop --branch nightly quickhop" ]; then
    pass "everything present: cargo install --locked --git with the branch"
else
    fail "everything present (status $status): $out / cargo: $(cat "$home/cargo.log" 2>/dev/null)"
fi

new_case
run -- --check
if [ "$status" -eq 0 ] && [[ "$out" == *"every check passed"* ]] && ! cargo_ran; then
    pass "--check runs the checks only"
else
    fail "--check (status $status): $out"
fi

new_case
run FAKE_RUST=1.96.1 --
expect_stop "Rust 1.96.1 from rustup: says rustup update stable" "rustup update stable"
if [[ "$out" == *"needs Rust $min_rust or newer; this is 1.96.1"* ]]; then
    pass "the old-Rust message names both versions"
else
    fail "the old-Rust message: $out"
fi

new_case
rm "$stubs/rustup"
run FAKE_RUST=1.75.0 --
expect_stop "distribution Rust 1.75: points at rustup" "not managed by rustup"
if [[ "$out" == *"sh.rustup.rs"* ]]; then
    pass "the distribution-Rust message gives the rustup line"
else
    fail "the distribution-Rust message: $out"
fi

new_case
rm "$stubs/rustc" "$stubs/cargo"
run --
expect_stop "no Rust: gives the rustup install line" "Rust is not installed"

new_case
mkdir -p "$home/.cargo/bin"
mv "$stubs/rustc" "$stubs/cargo" "$home/.cargo/bin/"
printf 'export PATH="$HOME/.cargo/bin:$PATH"\n' >"$home/.cargo/env"
run --
if [ "$status" -eq 0 ] && [ -s "$home/cargo.log" ]; then
    pass "a just-installed rustup is picked up from ~/.cargo/env"
else
    fail "\$HOME/.cargo/env (status $status): $out"
fi

new_case
stub xcode-select 'exit 2'
run --
expect_stop "macOS without the Command Line Tools: xcode-select --install" "xcode-select --install"

new_case
stub xcrun 'exit 1'
run --
expect_stop "macOS with the tools still installing (no SDK): stops" "still installing"

new_case
rm "$stubs/cc"
printf 'ID=fedora\n' >"$case_dir/os-release"
run FAKE_OS=Linux FAKE_CPU=x86_64 QUICKHOP_OS_RELEASE="$case_dir/os-release" --
expect_stop "Fedora without a C compiler: sudo dnf install gcc" "sudo dnf install gcc"

new_case
rm "$stubs/cc"
printf 'ID=ubuntu\nID_LIKE=debian\n' >"$case_dir/os-release"
run FAKE_OS=Linux FAKE_CPU=aarch64 QUICKHOP_OS_RELEASE="$case_dir/os-release" --
expect_stop "Ubuntu without a C compiler: sudo apt install build-essential" "sudo apt install build-essential"

new_case
run FAKE_OS=FreeBSD FAKE_CPU=amd64 --
expect_stop "an unsupported OS is named" "this is FreeBSD amd64"

new_case
run FAKE_FREE_KB=1048576 --
expect_stop "1 GB free: says how much is needed" "needs about 3 GB"

new_case
run FAKE_GIT=128 --
expect_stop "unreachable repository: explains access" "cannot reach https://github.com/yuyudhan/quickhop"

new_case
run QUICKHOP_SOURCE="$root" -- --force
if [ "$status" -eq 0 ] && [ "$(cat "$home/cargo.log")" = "install --locked --force --path $root/crates/quickhop" ]; then
    pass "QUICKHOP_SOURCE builds the checkout with --path, --force passed on"
else
    fail "QUICKHOP_SOURCE (status $status): $out / cargo: $(cat "$home/cargo.log" 2>/dev/null)"
fi

new_case
run -- --bogus
if [ "$status" -eq 2 ] && ! cargo_ran; then
    pass "an unknown argument exits 2"
else
    fail "unknown argument (status $status)"
fi

echo "--- $failures failed"
((failures == 0))
