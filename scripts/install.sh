#!/usr/bin/env bash
# Installs, updates or uninstalls quickhop on Linux and macOS, without sudo. Published with every
# GitHub release as releases/latest/download/install.sh; docs/install.md has the one-line curl
# command that pipes it into bash.
#
# Piping is safe: all work happens in main, called on the last line, so bash has read the whole
# script before anything runs. Options go after `bash -s --`.
#
# It fetches the release archive for this OS and CPU (scripts/dist.sh builds it), checks it
# against the release's .sha256, puts the `quickhop` binary in ~/.local/bin, and adds what the
# archive carries for the desktop: on Linux the desktop entry and icon under ~/.local/share, on
# macOS an app bundle in /Applications (~/Applications when /Applications is not writable).
#
# Options:
#   --version X.Y.Z    Install that release instead of the latest.
#   --from DIR         Install from a local directory holding the release assets (no network).
#   --uninstall        Stop the daemon, turn autostart off, remove what install put in place;
#                      keep the config and data.
#   --purge            With --uninstall, also delete the quickhop config and data directories.
#   -h, --help         Show this help.
#
# Environment:
#   QUICKHOP_INSTALL_DIR    Where the binary goes (default ~/.local/bin).
#   QUICKHOP_APP_DIR        Where the macOS app bundle goes (default /Applications).
#   QUICKHOP_INSTALL_BASE   URL of the directory holding the release assets, overriding GitHub
#                           (for mirrors and for testing a locally built release).
set -euo pipefail

repo="yuyudhan/quickhop"
# Every request: retry transient failures, give up on a connection that never opens, and abort
# a transfer that stalls (under 1 KB/s for 30 s) instead of hanging.
net=(--fail --silent --show-error --location --connect-timeout 15 --retry 3 --retry-delay 2
    --speed-limit 1024 --speed-time 30)

say() {
    printf 'quickhop: %s\n' "$*"
}

fail() {
    printf 'quickhop: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Install, update or uninstall quickhop (Linux and macOS, no sudo).

Options (after "bash -s --" when piped):
  --version X.Y.Z    install that release instead of the latest
  --from DIR         install from a local directory of release assets (no network)
  --uninstall        stop the daemon, remove quickhop, keep the config and data
  --purge            with --uninstall, also delete the config and data directories

Environment: QUICKHOP_INSTALL_DIR (default ~/.local/bin), QUICKHOP_APP_DIR (default
/Applications), QUICKHOP_INSTALL_BASE (asset URL base).
EOF
}

# The release asset name for this machine.
asset_target() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux)
            case "$arch" in
                x86_64 | amd64) echo "x86_64-unknown-linux-gnu" ;;
                aarch64 | arm64) echo "aarch64-unknown-linux-gnu" ;;
                *) fail "no Linux build for ${arch}; supported: x86_64, aarch64" ;;
            esac
            ;;
        Darwin) echo "universal-apple-darwin" ;;
        MINGW* | MSYS* | CYGWIN*)
            fail "quickhop does not run on Windows yet; see docs/platform-support.md"
            ;;
        *) fail "unsupported OS ${os}; quickhop runs on Linux and macOS" ;;
    esac
}

# The same rules as quickhop's own config and data directories on Linux and macOS.
config_dir() {
    echo "${XDG_CONFIG_HOME:-${HOME}/.config}/quickhop"
}

data_dir() {
    echo "${XDG_DATA_HOME:-${HOME}/.local/share}/quickhop"
}

share_dir() {
    echo "${XDG_DATA_HOME:-${HOME}/.local/share}"
}

# /Applications when writable, else ~/Applications; QUICKHOP_APP_DIR overrides both.
app_dir() {
    if [ -n "${QUICKHOP_APP_DIR:-}" ]; then
        echo "$QUICKHOP_APP_DIR"
    elif [ -w /Applications ]; then
        echo /Applications
    else
        echo "${HOME}/Applications"
    fi
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        fail "need sha256sum or shasum to verify the download"
    fi
}

# Copies or downloads one release asset from `base` (a URL, or a directory with --from).
fetch() {
    local from="$1" base="$2" name="$3" to="$4"
    if [ -n "$from" ]; then
        [ -f "${base}/${name}" ] || fail "no ${name} in ${base}"
        cp "${base}/${name}" "$to"
    else
        curl "${net[@]}" --output "$to" "${base}/${name}" || fail "download failed: ${base}/${name}"
    fi
}

# Puts `file` at `dest` with `mode`: copied beside it, then renamed over it, so a running daemon
# keeps its old inode and a failed copy never leaves a half-written file behind.
place() {
    local file="$1" dest="$2" mode="$3" tmp
    mkdir -p "$(dirname "$dest")"
    tmp="$(dirname "$dest")/.$(basename "$dest").new"
    cp "$file" "$tmp"
    chmod "$mode" "$tmp"
    mv -f "$tmp" "$dest"
}

install_release() {
    local version="$1" from="$2" bin_dir="$3" target base archive tmp expected actual pkg app
    target="$(asset_target)"
    if [ -n "$from" ]; then
        [ -d "$from" ] || fail "--from ${from}: not a directory"
        base="$(cd "$from" && pwd)"
    elif [ -n "${QUICKHOP_INSTALL_BASE:-}" ]; then
        base="${QUICKHOP_INSTALL_BASE%/}"
    elif [ -n "$version" ]; then
        base="https://github.com/${repo}/releases/download/v${version}"
    else
        base="https://github.com/${repo}/releases/latest/download"
    fi
    archive="quickhop-${target}.tar.gz"

    [ -n "$from" ] || command -v curl >/dev/null 2>&1 || fail "curl is required"
    command -v tar >/dev/null 2>&1 || fail "tar is required"
    tmp="$(mktemp -d)"
    # shellcheck disable=SC2064 # expand now: tmp is local and gone when the trap fires
    trap "rm -rf '${tmp}'" EXIT

    say "fetching ${archive} from ${base}"
    fetch "$from" "$base" "$archive" "${tmp}/${archive}"
    fetch "$from" "$base" "${archive}.sha256" "${tmp}/${archive}.sha256"
    expected="$(cut -d ' ' -f 1 <"${tmp}/${archive}.sha256")"
    actual="$(sha256_of "${tmp}/${archive}")"
    if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
        fail "checksum mismatch for ${archive} (expected ${expected}, got ${actual})"
    fi
    say "checksum ok (${actual})"

    tar -xzf "${tmp}/${archive}" -C "$tmp"
    pkg="${tmp}/quickhop-${target}"
    [ -f "${pkg}/quickhop" ] || fail "archive has no quickhop-${target}/quickhop"
    if [ -n "$version" ] && ! "${pkg}/quickhop" --version | grep -qx "quickhop ${version}"; then
        fail "the archive holds $("${pkg}/quickhop" --version), not quickhop ${version}"
    fi

    place "${pkg}/quickhop" "${bin_dir}/quickhop" 755
    if [ "$(uname -s)" = "Darwin" ]; then
        xattr -d com.apple.quarantine "${bin_dir}/quickhop" 2>/dev/null || true
    fi
    say "installed $("${bin_dir}/quickhop" --version) to ${bin_dir}/quickhop"

    if [ -d "${pkg}/quickhop.app" ]; then
        app="$(app_dir)/quickhop.app"
        mkdir -p "$(dirname "$app")"
        rm -rf "${app}.new"
        cp -R "${pkg}/quickhop.app" "${app}.new"
        rm -rf "$app"
        mv "${app}.new" "$app"
        xattr -dr com.apple.quarantine "$app" 2>/dev/null || true
        say "installed ${app}"
    fi
    if [ -f "${pkg}/share/applications/quickhop.desktop" ]; then
        place "${pkg}/share/applications/quickhop.desktop" \
            "$(share_dir)/applications/quickhop.desktop" 644
        place "${pkg}/share/icons/hicolor/scalable/apps/quickhop.svg" \
            "$(share_dir)/icons/hicolor/scalable/apps/quickhop.svg" 644
        say "installed the desktop entry and icon in $(share_dir)"
    fi

    case ":${PATH}:" in
        *":${bin_dir}:"*) ;;
        *) say "add ${bin_dir} to PATH, e.g. in ~/.profile: export PATH=\"${bin_dir}:\$PATH\"" ;;
    esac
    say "next: quickhop doctor, then quickhop autostart on"
}

# Removes `path` and says so; absent paths are skipped quietly.
remove() {
    if [ -e "$1" ] || [ -L "$1" ]; then
        rm -rf "$1"
        say "removed $1"
    fi
}

uninstall() {
    local bin_dir="$1" purge="$2" bin="${1}/quickhop" dir
    if [ -x "$bin" ]; then
        # Best effort: a daemon that is not running and autostart that is off are both fine.
        "$bin" daemon stop >/dev/null 2>&1 || true
        "$bin" autostart off >/dev/null 2>&1 || true
        remove "$bin"
    else
        say "quickhop is not installed in ${bin_dir}"
    fi
    remove "$(app_dir)/quickhop.app"
    remove "$(share_dir)/applications/quickhop.desktop"
    remove "$(share_dir)/icons/hicolor/scalable/apps/quickhop.svg"
    for dir in "$(config_dir)" "$(data_dir)"; do
        if [ "$purge" = true ]; then
            if [ -e "$dir" ]; then
                say "--purge deletes: ${dir}"
                remove "$dir"
            fi
        elif [ -d "$dir" ]; then
            say "kept ${dir} (add --purge to delete it)"
        fi
    done
}

main() {
    local version="" from="" action=install purge=false bin_dir
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --version)
                [ "$#" -ge 2 ] || fail "--version needs a value"
                version="${2#v}"
                shift 2
                ;;
            --from)
                [ "$#" -ge 2 ] || fail "--from needs a directory"
                from="$2"
                shift 2
                ;;
            --uninstall)
                action=uninstall
                shift
                ;;
            --purge)
                purge=true
                shift
                ;;
            -h | --help)
                usage
                return 0
                ;;
            *) fail "unknown option $1 (see --help)" ;;
        esac
    done
    if [ -n "$version" ] && ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        fail "--version wants MAJOR.MINOR.PATCH, got ${version}"
    fi
    if [ "$purge" = true ] && [ "$action" != uninstall ]; then
        fail "--purge only goes with --uninstall"
    fi
    if [ "$action" = uninstall ] && { [ -n "$version" ] || [ -n "$from" ]; }; then
        fail "--uninstall takes no --version or --from"
    fi
    [ -n "${HOME:-}" ] || fail "HOME is not set"
    bin_dir="${QUICKHOP_INSTALL_DIR:-${HOME}/.local/bin}"

    if [ "$action" = uninstall ]; then
        uninstall "$bin_dir" "$purge"
    else
        install_release "$version" "$from" "$bin_dir"
    fi
}

main "$@"
