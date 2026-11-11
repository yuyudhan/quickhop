#!/usr/bin/env bash
# Installs, updates or uninstalls quickhop on Linux and macOS, without sudo. Published with every
# GitHub release as releases/latest/download/install.sh; docs/install.md has the one-line install
# that downloads it into a fresh temporary folder, runs it from there and removes that folder.
#
# Every download goes into one private temporary folder (mktemp -d), removed when the script
# exits for any reason: success, an error, Ctrl+C (INT) or TERM. Nothing is written to the
# current folder. Piping is safe too: all work happens in main, called on the last line, so bash
# has read the whole script before anything runs. Options go after `bash -s --`.
#
# It fetches the release archive for this OS and CPU (scripts/dist.sh builds it), checks it
# against the release's .sha256 and that its binary starts on this system, then:
#   macOS: puts quickhop.app in /Applications (~/Applications when /Applications is not
#     writable) and makes ~/.local/bin/quickhop a symlink to the app's executable, so the
#     command line and the app are one signed program with one Accessibility grant.
#   Linux (x86_64, aarch64; glibc 2.31 or later): puts the `quickhop` binary in ~/.local/bin,
#     and the desktop entry and icon under ~/.local/share. Systems the build cannot run on (an
#     older glibc, musl) are refused before anything is downloaded.
# A daemon that was running is stopped before anything is replaced and started again the same
# way afterwards (its LaunchAgent, quickhop.app, its systemd user unit, or `quickhop daemon`),
# so an update takes effect without a manual restart.
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
#   QUICKHOP_INSTALL_DIR    Where the command goes (default ~/.local/bin).
#   QUICKHOP_APP_DIR        Where the macOS app bundle goes (default /Applications).
#   QUICKHOP_INSTALL_BASE   URL of the directory holding the release assets, overriding GitHub
#                           (for mirrors and for testing a locally built release).
set -euo pipefail

repo="yuyudhan/quickhop"
# The LaunchAgent label `quickhop autostart on` writes, also quickhop.app's bundle identifier.
launchd_label="io.github.yuyudhan.quickhop"
# The Linux release is built in a debian:bullseye container, so it needs that glibc or later.
min_glibc="2.31"
source_url="https://github.com/${repo}/blob/nightly/docs/install.md#from-source"
# How long a daemon may take to stop or to start, in tenths of a second.
daemon_wait=150
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
            fail "quickhop's installer is for macOS and Linux; on Windows only the command line builds, from source: ${source_url}"
            ;;
        *) fail "unsupported OS ${os}; quickhop runs on macOS and Linux" ;;
    esac
}

# True when version $1 (MAJOR.MINOR) is $2 or later.
version_at_least() {
    local have_major="${1%%.*}" have_minor="${1#*.}" want_major="${2%%.*}" want_minor="${2#*.}"
    have_minor="${have_minor%%.*}"
    [ "$have_major" -gt "$want_major" ] ||
        { [ "$have_major" -eq "$want_major" ] && [ "$have_minor" -ge "$want_minor" ]; }
}

# This system's glibc version (MAJOR.MINOR), or nothing when neither getconf nor ldd names one.
glibc_version() {
    local out
    if out="$(getconf GNU_LIBC_VERSION 2>/dev/null)" && [[ "$out" =~ ^glibc\ ([0-9]+\.[0-9]+) ]]; then
        echo "${BASH_REMATCH[1]}"
        return 0
    fi
    # "ldd (GNU libc) 2.31", "ldd (Ubuntu GLIBC 2.35-0ubuntu3.8) 2.35".
    out="$(ldd --version 2>&1 | head -n 1 || true)"
    if [[ "$out" =~ ^ldd\ .*\)\ ([0-9]+\.[0-9]+) ]]; then
        echo "${BASH_REMATCH[1]}"
    fi
}

glibc_hint() {
    echo "quickhop's Linux build needs glibc ${min_glibc} or later (Debian 11, Ubuntu 20.04, Fedora 32, RHEL 9 or newer)"
}

# Refuses a Linux system the release binary cannot start on, before anything is downloaded.
preflight_linux() {
    local have ldd_says
    # musl's ldd prints its version on stderr and exits 1.
    ldd_says="$(ldd --version 2>&1 || true)"
    if [[ "$ldd_says" == *[Mm]usl* ]]; then
        fail "this system uses musl libc (Alpine, Void musl), and $(glibc_hint). Build quickhop from source instead: ${source_url}"
    fi
    have="$(glibc_version)"
    if [ -n "$have" ] && ! version_at_least "$have" "$min_glibc"; then
        fail "$(glibc_hint); this system has glibc ${have}. Build quickhop from source instead: ${source_url}"
    fi
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

# Copies or downloads one release asset from `base` (a URL, or a directory with --from), and
# fails on anything but a non-empty file: a 404 (no such release asset), a network error, or an
# empty body.
fetch() {
    local from="$1" base="$2" name="$3" to="$4" url code
    if [ -n "$from" ]; then
        [ -f "${base}/${name}" ] || fail "no ${name} in ${base}"
        cp "${base}/${name}" "$to"
    else
        url="${base}/${name}"
        if ! code="$(curl "${net[@]}" --write-out '%{http_code}' --output "$to" "$url")"; then
            case "$code" in
                404) fail "${url} does not exist (HTTP 404): there is no such release asset. Check the version at https://github.com/${repo}/releases" ;;
                000 | "") fail "could not download ${url} (no answer: check the network or proxy)" ;;
                *) fail "could not download ${url} (HTTP ${code})" ;;
            esac
        fi
    fi
    [ -s "$to" ] || fail "${name} from ${base} is empty"
}

# Puts `file` at `dest` with `mode`: copied beside it, then renamed over it, so a running daemon
# keeps its old inode and a failed copy never leaves a half-written file behind.
place() {
    local file="$1" dest="$2" mode="$3" tmp
    mkdir -p "$(dirname "$dest")"
    tmp="$(dirname "$dest")/.$(basename "$dest").new"
    rm -f "$tmp"
    cp "$file" "$tmp"
    chmod "$mode" "$tmp"
    mv -f "$tmp" "$dest"
}

# Makes `link` a symlink to `target`, replacing a file or link there in one rename.
place_link() {
    local target="$1" link="$2" tmp
    mkdir -p "$(dirname "$link")"
    tmp="$(dirname "$link")/.$(basename "$link").new"
    rm -f "$tmp"
    ln -s "$target" "$tmp"
    mv -f "$tmp" "$link"
}

# The `--version` line of the downloaded program, run before anything is replaced: a build this
# system cannot start (too old a glibc, a missing library) fails the install with the loader's
# own message and what to do, and leaves an installed copy as it was.
runnable_version() {
    local program="$1" out hint=""
    if out="$("$program" --version 2>&1)" && [[ "$out" =~ ^quickhop\ [0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        echo "$out"
        return 0
    fi
    if [ "$(uname -s)" = Linux ]; then
        hint=" $(glibc_hint); build from source on older systems: ${source_url}"
    fi
    fail "the downloaded quickhop does not run on this system: ${out:-no output}.${hint}"
}

# The executable of process `pid`, or nothing.
process_exe() {
    if [ -L "/proc/$1/exe" ]; then
        readlink "/proc/$1/exe" 2>/dev/null || true
    else
        ps -o comm= -p "$1" 2>/dev/null || true
    fi
}

# How the daemon with `pid`, running from `exe`, was started, so it can be started again the
# same way: "launchd" (its LaunchAgent), "app" (quickhop.app opened), "systemd UNIT" (a
# quickhop systemd user unit, including the one XDG autostart generates), or "spawn" (a
# `quickhop daemon` from a terminal; "spawn --no-window" when it ran without the window).
start_method() {
    local pid="$1" exe="$2" unit
    if [ "$(uname -s)" = Darwin ]; then
        if launchctl print "gui/$(id -u)/${launchd_label}" 2>/dev/null |
            grep -Eq "^[[:space:]]*pid = ${pid}\$"; then
            echo launchd
            return 0
        fi
        if [[ "$exe" == *.app/Contents/MacOS/* ]]; then
            echo app
            return 0
        fi
    elif [ -r "/proc/${pid}/cgroup" ]; then
        # "0::/user.slice/user-1000.slice/user@1000.service/app.slice/quickhop.service"; only a
        # unit of quickhop's own is restarted, never the terminal or session it runs in.
        unit="$(sed -n 's,^0::.*/user@[0-9]*\.service/\(.*/\)\{0,1\}\([^/]*\)$,\2,p' "/proc/${pid}/cgroup")"
        if [[ "$unit" =~ ^(app-)?quickhop[^/]*\.service$ ]]; then
            echo "systemd ${unit}"
            return 0
        fi
    fi
    case " $(ps -o args= -p "$pid" 2>/dev/null || true) " in
        *" --no-window "*) echo "spawn --no-window" ;;
        *) echo spawn ;;
    esac
}

# Waits until process `pid` has ended; false when it still runs after `daemon_wait`.
wait_gone() {
    local pid="$1" tries=0
    while kill -0 "$pid" 2>/dev/null; do
        [ "$tries" -lt "$daemon_wait" ] || return 1
        tries=$((tries + 1))
        sleep 0.1
    done
}

# Stops the daemon that answered `status` (the `daemon status` line) with the new binary
# `program`, before anything is replaced. Prints the start method to restart it with.
stop_daemon() {
    local program="$1" status="$2" pid exe method
    [[ "$status" =~ pid\ ([0-9]+) ]] || fail "cannot read the daemon's pid from: ${status}"
    pid="${BASH_REMATCH[1]}"
    if [[ "$status" =~ \;\ exe\ (.+)\;\ clipboard: ]]; then
        exe="${BASH_REMATCH[1]}"
    else
        exe="$(process_exe "$pid")"
    fi
    method="$(start_method "$pid" "$exe")"
    say "stopping the running quickhop daemon (pid ${pid}) for the update" >&2
    case "$method" in
        systemd\ *) systemctl --user stop "${method#systemd }" >/dev/null 2>&1 || true ;;
        *) "$program" daemon stop >/dev/null 2>&1 || true ;;
    esac
    wait_gone "$pid" ||
        fail "the running quickhop daemon (pid ${pid}) did not stop; nothing was replaced. Stop it (kill ${pid}) and run the installer again"
    echo "$method"
}

# Starts the daemon again with `method`, from the installed `bin` (and `app` on macOS), and
# checks that the one now answering is `version`.
restart_daemon() {
    local method="$1" bin="$2" app="$3" version="$4" tries=0 status
    case "$method" in
        launchd) launchctl kickstart "gui/$(id -u)/${launchd_label}" >/dev/null 2>&1 ||
            fail "could not start the quickhop LaunchAgent again; run: quickhop daemon" ;;
        app) open -g "$app" || fail "could not open ${app} again; open it from Finder" ;;
        systemd\ *) systemctl --user start "${method#systemd }" ||
            fail "could not start ${method#systemd } again; run: systemctl --user start ${method#systemd }" ;;
        spawn*)
            # shellcheck disable=SC2086 # the flags, if any, are separate words
            if command -v setsid >/dev/null 2>&1; then
                setsid "$bin" daemon ${method#spawn} </dev/null >/dev/null 2>&1 &
            else
                nohup "$bin" daemon ${method#spawn} </dev/null >/dev/null 2>&1 &
            fi
            ;;
    esac
    until status="$("$bin" daemon status 2>/dev/null)"; do
        [ "$tries" -lt "$daemon_wait" ] ||
            fail "the updated quickhop daemon did not start; start it with: quickhop daemon"
        tries=$((tries + 1))
        sleep 0.1
    done
    if [[ "$status" =~ version\ ([0-9][0-9A-Za-z.+-]*) ]] && [ "${BASH_REMATCH[1]}" != "$version" ]; then
        fail "the daemon now running is version ${BASH_REMATCH[1]}, not ${version}: ${status%%;*}"
    fi
    say "restarted the daemon: ${status%%; clipboard:*}"
}

install_release() {
    local version="$1" from="$2" bin_dir="$3" target base archive tmp expected actual pkg app=""
    local new_version status="" method="" bin="${3}/quickhop"
    target="$(asset_target)"
    [ "$(uname -s)" != Linux ] || preflight_linux
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
    # One private folder for every download and the unpacked archive, removed on every exit.
    # INT and TERM exit through the EXIT trap, so Ctrl+C or a kill leaves nothing behind either.
    tmp="$(mktemp -d "${TMPDIR:-/tmp}/quickhop-install.XXXXXX")" ||
        fail "could not create a temporary folder in ${TMPDIR:-/tmp}"
    # shellcheck disable=SC2064 # expand now: tmp is local and gone when the trap fires
    trap "rm -rf '${tmp}'" EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM

    say "fetching ${archive} from ${base}"
    fetch "$from" "$base" "$archive" "${tmp}/${archive}"
    fetch "$from" "$base" "${archive}.sha256" "${tmp}/${archive}.sha256"
    expected="$(cut -d ' ' -f 1 <"${tmp}/${archive}.sha256")"
    [[ "$expected" =~ ^[0-9a-f]{64}$ ]] || fail "${archive}.sha256 holds no SHA-256 checksum"
    actual="$(sha256_of "${tmp}/${archive}")"
    [ "$expected" = "$actual" ] ||
        fail "checksum mismatch for ${archive} (expected ${expected}, got ${actual})"
    say "checksum ok (${actual})"

    tar -xzf "${tmp}/${archive}" -C "$tmp" 2>/dev/null || fail "${archive} is not a valid archive"
    pkg="${tmp}/quickhop-${target}"
    [ -f "${pkg}/quickhop" ] || fail "archive has no quickhop-${target}/quickhop"
    new_version="$(runnable_version "${pkg}/quickhop")"
    if [ -n "$version" ] && [ "$new_version" != "quickhop ${version}" ]; then
        fail "the archive holds ${new_version}, not quickhop ${version}"
    fi

    # The new binary speaks to a daemon of any version over its socket.
    if status="$("${pkg}/quickhop" daemon status 2>/dev/null)"; then
        method="$(stop_daemon "${pkg}/quickhop" "$status")"
    fi

    if [ -d "${pkg}/quickhop.app" ]; then
        app="$(app_dir)/quickhop.app"
        mkdir -p "$(dirname "$app")"
        rm -rf "${app}.new"
        cp -R "${pkg}/quickhop.app" "${app}.new"
        rm -rf "$app"
        mv "${app}.new" "$app"
        xattr -dr com.apple.quarantine "$app" 2>/dev/null || true
        say "installed ${app}"
        # One program for the app and the command line: macOS keeps one Accessibility grant.
        place_link "${app}/Contents/MacOS/quickhop" "$bin"
        say "installed ${new_version}: ${bin} -> ${app}/Contents/MacOS/quickhop"
    else
        place "${pkg}/quickhop" "$bin" 755
        say "installed $("$bin" --version) to ${bin}"
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
    if [ -n "$method" ]; then
        restart_daemon "$method" "$bin" "$app" "${new_version#quickhop }"
    elif [ -n "$app" ]; then
        say "next: open ${app} (it asks for Accessibility), then quickhop autostart on"
    else
        say "next: quickhop doctor, then quickhop autostart on"
    fi
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
    else
        say "quickhop is not installed in ${bin_dir}"
    fi
    remove "$bin"
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
