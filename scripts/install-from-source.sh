#!/usr/bin/env bash
# Builds and installs quickhop from source with `cargo install`, after checking everything the
# build needs. A missing piece stops the script before cargo starts, with the command that fixes
# it, instead of a compiler or linker error minutes into the build.
#
# Usage: scripts/install-from-source.sh [--branch NAME | --tag NAME] [--force] [--check]
#   --branch NAME  build that branch of the repository (default: its default branch)
#   --tag NAME     build that tag
#   --force        replace an installed quickhop (`cargo install --force`)
#   --check        run the checks only
#
# It checks, in order: the OS and CPU; Rust (installed, and at least MIN_RUST below, with the
# rustup command or install line when not; a just-installed rustup is picked up from
# ~/.cargo/env); on macOS the Command Line Tools; on Linux a C compiler (for the bundled
# SQLite); free disk space; that the repository can be reached. Then it runs
# `cargo install --locked`, which builds the dependency versions in Cargo.lock.
#
# Environment:
#   QUICKHOP_REPO         git URL to build from (default https://github.com/yuyudhan/quickhop)
#   QUICKHOP_SOURCE       a local checkout to build instead (`cargo install --path`); no network
#                         check
#   QUICKHOP_MIN_DISK_GB  free space needed where cargo builds, in GB (default 3)
#   QUICKHOP_OS_RELEASE   the os-release file naming the Linux distribution (default
#                         /etc/os-release)
set -euo pipefail

# The oldest Rust that builds quickhop: the workspace `rust-version` in Cargo.toml.
# scripts/test-install-from-source.sh fails when the two differ.
MIN_RUST="1.98"

repo="${QUICKHOP_REPO:-https://github.com/yuyudhan/quickhop}"
source_dir="${QUICKHOP_SOURCE:-}"
min_disk_gb="${QUICKHOP_MIN_DISK_GB:-3}"
os_release="${QUICKHOP_OS_RELEASE:-/etc/os-release}"
rustup_line="curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"

ref=()
force=()
check_only=false
while (($#)); do
    case "$1" in
        --branch | --tag)
            [ "$#" -ge 2 ] || {
                echo "install-from-source: $1 needs a name" >&2
                exit 2
            }
            ref=("$1" "$2")
            shift 2
            ;;
        --force)
            force=(--force)
            shift
            ;;
        --check)
            check_only=true
            shift
            ;;
        -h | --help)
            awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
            exit 0
            ;;
        *)
            echo "install-from-source: unknown argument $1 (see --help)" >&2
            exit 2
            ;;
    esac
done

say() {
    echo "install-from-source: $*"
}

# Stops before cargo runs: the problem, then each fix line indented.
stop() {
    local problem="$1"
    shift
    echo "install-from-source: $problem" >&2
    local line
    for line in "$@"; do
        echo "    $line" >&2
    done
    exit 1
}

# Whether version $1 (such as 1.96.1) is at least $2 (such as 1.98).
at_least() {
    local IFS=.
    local -a have want
    read -r -a have <<<"$1"
    read -r -a want <<<"$2"
    local i
    for i in 0 1 2; do
        local h="${have[i]:-0}" w="${want[i]:-0}"
        if ((10#$h > 10#$w)); then
            return 0
        fi
        if ((10#$h < 10#$w)); then
            return 1
        fi
    done
    return 0
}

# The distribution's install command for package names per family: apt, dnf, pacman, zypper.
distro_install() {
    local apt="$1" dnf="$2" pacman="$3" zypper="$4" ids=""
    if [ -r "$os_release" ]; then
        ids="$(sed -n -e 's/^ID=//p' -e 's/^ID_LIKE=//p' "$os_release" | tr -d '"' | tr '\n' ' ')"
    fi
    case " $ids " in
        *" debian "* | *" ubuntu "*) echo "sudo apt install $apt" ;;
        *" fedora "* | *" rhel "* | *" centos "*) echo "sudo dnf install $dnf" ;;
        *" arch "*) echo "sudo pacman -S $pacman" ;;
        *" suse "* | *" opensuse "*) echo "sudo zypper install $zypper" ;;
        *) echo "install $apt (Debian/Ubuntu), $dnf (Fedora), $pacman (Arch) or your distribution's equivalent" ;;
    esac
}

# 1. OS and CPU.
os="$(uname -s)"
cpu="$(uname -m)"
case "$os/$cpu" in
    Darwin/arm64 | Darwin/x86_64 | Linux/x86_64 | Linux/aarch64 | Linux/arm64) ;;
    *) stop "quickhop builds on macOS (Apple silicon, Intel) and Linux (x86_64, aarch64); this is $os $cpu" ;;
esac
say "system: $os $cpu"

# 2. Rust. rustup's installer puts cargo on PATH only for new shells; pick it up here.
if ! command -v cargo >/dev/null && [ -r "${CARGO_HOME:-$HOME/.cargo}/env" ]; then
    # shellcheck disable=SC1091 # written by rustup's installer
    . "${CARGO_HOME:-$HOME/.cargo}/env"
fi
if ! command -v rustc >/dev/null || ! command -v cargo >/dev/null; then
    stop "Rust is not installed (no rustc and cargo on PATH). Install it with rustup, then run this again:" \
        "$rustup_line" \
        ". \"\$HOME/.cargo/env\"    # or open a new terminal"
fi
rust_version="$(rustc --version | sed -n 's/^rustc \([0-9][0-9.]*\).*/\1/p')"
[ -n "$rust_version" ] || stop "cannot read the Rust version from: $(rustc --version)"
if ! at_least "$rust_version" "$MIN_RUST"; then
    if command -v rustup >/dev/null; then
        stop "quickhop needs Rust $MIN_RUST or newer; this is $rust_version. Update it, then run this again:" \
            "rustup update stable" \
            "(a rust-toolchain.toml or \`rustup override\` in this folder wins over stable: \`rustup show\` says which toolchain is used)"
    fi
    stop "quickhop needs Rust $MIN_RUST or newer; this is $rust_version from $(command -v rustc), not managed by rustup. Distribution Rust packages are usually too old; install rustup, then run this again:" \
        "$rustup_line" \
        ". \"\$HOME/.cargo/env\"    # or open a new terminal"
fi
say "rust: $rust_version (needs $MIN_RUST or newer)"

# 3. The C toolchain (the bundled SQLite is C; on macOS the window toolkit needs the SDK).
if [ "$os" = Darwin ]; then
    if ! xcode-select -p >/dev/null 2>&1 || ! xcrun --show-sdk-path >/dev/null 2>&1; then
        stop "the Xcode Command Line Tools are missing or still installing. Install them, wait until the installer finishes, then run this again:" \
            "xcode-select --install" \
            "xcode-select -p    # prints a path once they are installed"
    fi
    say "command line tools: $(xcode-select -p)"
else
    if ! command -v cc >/dev/null && ! command -v gcc >/dev/null && ! command -v clang >/dev/null; then
        stop "no C compiler (cc, gcc or clang); the bundled SQLite is compiled from C. Install one, then run this again:" \
            "$(distro_install build-essential gcc base-devel gcc)"
    fi
    say "c compiler: found"
fi

# 4. Free disk where cargo downloads and builds (the window toolkit's git checkout and a
# release build of it).
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
probe="$cargo_home"
while [ ! -d "$probe" ]; do
    probe="$(dirname "$probe")"
done
free_kb="$(df -Pk "$probe" | sed -n '2p' | tr -s ' ' | cut -d ' ' -f 4)"
if [ -n "$free_kb" ] && ((free_kb < min_disk_gb * 1024 * 1024)); then
    stop "only $((free_kb / 1024 / 1024)) GB free under $probe; building quickhop needs about $min_disk_gb GB. Free some space, then run this again (cargo clean in old Rust projects frees the most)."
fi
say "disk: $((free_kb / 1024 / 1024)) GB free under $probe"

# 5. The source.
if [ -n "$source_dir" ]; then
    [ -f "$source_dir/crates/quickhop/Cargo.toml" ] ||
        stop "QUICKHOP_SOURCE=$source_dir is not a quickhop checkout (no crates/quickhop/Cargo.toml)"
    install=(cargo install --locked ${force[@]+"${force[@]}"} --path "$source_dir/crates/quickhop")
    say "source: $source_dir"
else
    if ! command -v git >/dev/null; then
        if [ "$os" = Darwin ]; then
            git_fix="xcode-select --install"
        else
            git_fix="$(distro_install git git git git)"
        fi
        stop "git is needed to fetch the source. Install it, then run this again:" "$git_fix"
    fi
    if ! GIT_TERMINAL_PROMPT=0 git ls-remote "$repo" HEAD >/dev/null 2>&1; then
        stop "cannot reach $repo. Check the network or proxy. While the repository is private it needs a GitHub account with access: with the GitHub CLI, \`gh auth login\` then \`gh auth setup-git\`, and run this again."
    fi
    # cargo's own git client ignores git's credential helpers and some proxies; git's does not.
    export CARGO_NET_GIT_FETCH_WITH_CLI="${CARGO_NET_GIT_FETCH_WITH_CLI:-true}"
    install=(cargo install --locked ${force[@]+"${force[@]}"} --git "$repo" ${ref[@]+"${ref[@]}"} quickhop)
    say "source: $repo ${ref[*]+${ref[*]}}"
fi

if $check_only; then
    say "every check passed; would run: ${install[*]}"
    exit 0
fi
say "building (several minutes the first time): ${install[*]}"
"${install[@]}"

bin_dir="$cargo_home/bin"
case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) say "add $bin_dir to PATH (rustup's ~/.cargo/env does it), or open a new terminal" ;;
esac
say "installed. Next: quickhop doctor, then quickhop autostart on"
