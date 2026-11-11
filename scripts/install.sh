#!/bin/sh
# Installs, updates or uninstalls quickhop on Linux and macOS for the current user, without sudo:
#
#     curl -fsSL https://yuyudhan.github.io/quickhop/install | sh
#     curl -fsSL https://yuyudhan.github.io/quickhop/install | sh -s -- --uninstall
#
# That URL serves this file as attached to the latest GitHub release (releases/latest/download/
# install.sh), so `sh install.sh` works the same. POSIX sh: it runs piped to sh (dash, bash in
# POSIX mode), bash and zsh. Everything is a function and the last line calls main, so a
# download cut short runs nothing; every command that could read stdin (the script itself when
# piped) reads /dev/null instead.
#
# It picks the release archive for this OS and CPU (scripts/dist.sh builds it), checks its
# SHA-256 against the release's SHA256SUMS (a --from folder's SHA256SUMS, else the archive's
# .sha256) and that its binary starts on this system, then:
#   macOS: puts quickhop.app in /Applications (~/Applications when /Applications is not
#     writable) and makes ~/.local/bin/quickhop a symlink to the app's executable, so the
#     command line and the app are one signed program with one Accessibility grant.
#   Linux (x86_64, aarch64; glibc 2.31 or later): puts the `quickhop` binary in ~/.local/bin,
#     and the desktop entry and icon under ~/.local/share. Systems the build cannot run on (an
#     older glibc, musl) are refused before anything is downloaded.
# A daemon that was running is stopped before anything is replaced and started again the same
# way afterwards (its LaunchAgent, quickhop.app, its systemd user unit, or `quickhop daemon`),
# so an update takes effect without a manual restart. Running it again reinstalls in place.
#
# Every download goes into one private temporary folder (mktemp -d), removed when the script
# exits for any reason: success, an error, Ctrl+C (INT), HUP or TERM. Nothing is written to the
# current folder. Every failure is one `quickhop: error: <what happened>; <the fix>` line on
# stderr and exit status 1. Colour only on a terminal, without NO_COLOR, and TERM not dumb.
#
# Options:
#   --version X.Y.Z    Install that release instead of the latest (vX.Y.Z works too).
#   --from DIR         Install from a local directory holding the release assets (no network).
#   --uninstall        Stop the daemon, turn autostart off, remove what install put in place;
#                      keep the config and data.
#   --purge            With --uninstall, also delete the quickhop config and data directories.
#   -h, --help         Show this help.
#
# Environment:
#   QUICKHOP_VERSION        The release to install, as --version (the option wins).
#   QUICKHOP_INSTALL_DIR    Where the command goes (default ~/.local/bin).
#   QUICKHOP_APP_DIR        Where the macOS app bundle goes (default /Applications).
#   QUICKHOP_INSTALL_BASE   URL of the directory holding the release assets, overriding GitHub
#                           (for mirrors and for testing a locally built release).

# zsh (`curl … | zsh`) runs it as sh would: an EXIT trap set in a function then fires when the
# script exits rather than when that function returns. Before `set`, which emulate resets.
if [ -n "${ZSH_VERSION:-}" ]; then
    emulate sh
fi
set -eu

repo="yuyudhan/quickhop"
releases_url="https://github.com/${repo}/releases"
source_url="https://github.com/${repo}/blob/nightly/docs/install.md#from-source"
install_url="https://yuyudhan.github.io/quickhop/install"
# The LaunchAgent label `quickhop autostart on` writes, also quickhop.app's bundle identifier.
launchd_label="io.github.yuyudhan.quickhop"
# The release's default `hotkey` (docs/configuration.md).
hotkey="Alt+Shift+Space"
# The Linux release is built in a debian:bullseye container, so it needs that glibc or later.
min_glibc="2.31"
# How long a daemon may take to stop or to start, in tenths of a second.
daemon_wait=150

# The private temporary folder of this run (install_release creates it); removed on every exit.
work=""
# Whether curl knows --retry-all-errors (7.71+); install_release finds out.
retry_all=false
# The exit status of the HUP, INT or TERM that ended this run (the traps set it).
signalled=""
# Escape sequences, set by setup_colour when the stream is a colour terminal.
bold=""
green=""
dim=""
reset=""
red=""
red_reset=""

remove_work() {
    if [ -n "$work" ]; then
        rm -rf "$work"
    fi
}

setup_colour() {
    esc="$(printf '\033')"
    if [ -z "${NO_COLOR:-}" ] && [ -n "${TERM:-}" ] && [ "$TERM" != dumb ]; then
        if [ -t 1 ]; then
            bold="${esc}[1m"
            green="${esc}[32m"
            dim="${esc}[2m"
            reset="${esc}[0m"
        fi
        if [ -t 2 ]; then
            red="${esc}[31m"
            red_reset="${esc}[0m"
        fi
    fi
}

# One checked step: `  ✓ TEXT DETAIL`, the detail dimmed.
step() {
    if [ "$#" -gt 1 ]; then
        printf '  %s✓%s %s %s%s%s\n' "$green" "$reset" "$1" "$dim" "$2" "$reset"
    else
        printf '  %s✓%s %s\n' "$green" "$reset" "$1"
    fi
}

# One line under `Next:`.
next() {
    printf '  %s\n' "$1"
}

# The one error line, newlines in it (a loader's message, say) folded into spaces, and exit 1.
fail() {
    f_message="$(printf '%s' "$*" | tr '\n' ' ')"
    printf 'quickhop: %serror%s: %s\n' "$red" "$red_reset" "$f_message" >&2
    exit 1
}

usage() {
    cat <<EOF
Install, update or uninstall quickhop (Linux and macOS, no sudo).

Usage: sh install.sh [options]
Piped from ${install_url}, the options go after "sh -s --":
  ... | sh -s -- --version 0.1.0

Options:
  --version X.Y.Z    install that release instead of the latest (vX.Y.Z works too)
  --from DIR         install from a local directory of release assets (no network)
  --uninstall        stop the daemon, remove quickhop, keep the config and data
  --purge            with --uninstall, also delete the config and data directories
  -h, --help         show this help

Environment: QUICKHOP_VERSION (as --version), QUICKHOP_INSTALL_DIR (default ~/.local/bin),
QUICKHOP_APP_DIR (default /Applications), QUICKHOP_INSTALL_BASE (asset URL base).
EOF
}

# Path $1 with a leading $HOME shown as ~.
tildify() {
    # shellcheck disable=SC2088 # a path shown to the user, not expanded
    case "$1" in
        "$HOME"/*) printf '~/%s\n' "${1#"$HOME"/}" ;;
        *) printf '%s\n' "$1" ;;
    esac
}

# 1 on Apple silicon, even under Rosetta (where uname -m says x86_64), else 0.
mac_arm64() {
    sysctl -n hw.optional.arm64 2>/dev/null </dev/null ||
        /usr/sbin/sysctl -n hw.optional.arm64 2>/dev/null </dev/null ||
        echo 0
}

# Sets `system`, `target` (the release asset's target) and `platform` (what the first step
# names) for this machine, or fails when the release has no build for it.
detect_platform() {
    system="$(uname -s)"
    cpu="$(uname -m)"
    case "$system" in
        Linux)
            case "$cpu" in
                x86_64 | amd64)
                    target="x86_64-unknown-linux-gnu"
                    platform="Linux (x86_64)"
                    ;;
                aarch64 | arm64)
                    target="aarch64-unknown-linux-gnu"
                    platform="Linux (aarch64)"
                    ;;
                *) fail "no quickhop build for Linux ${cpu}; build from source: ${source_url}" ;;
            esac
            ;;
        Darwin)
            target="universal-apple-darwin"
            if [ "$(mac_arm64)" = 1 ]; then
                platform="macOS (Apple silicon)"
            else
                platform="macOS (Intel)"
            fi
            ;;
        MINGW* | MSYS* | CYGWIN*)
            fail "no quickhop installer for Windows; build the command line from source: ${source_url}"
            ;;
        *) fail "no quickhop build for ${system} ${cpu}; build from source: ${source_url}" ;;
    esac
}

# True when version $1 (MAJOR.MINOR) is $2 or later.
version_at_least() {
    v_have_major="${1%%.*}"
    v_have_minor="${1#*.}"
    v_have_minor="${v_have_minor%%.*}"
    v_want_major="${2%%.*}"
    v_want_minor="${2#*.}"
    [ "$v_have_major" -gt "$v_want_major" ] ||
        { [ "$v_have_major" -eq "$v_want_major" ] && [ "$v_have_minor" -ge "$v_want_minor" ]; }
}

# This system's glibc version (MAJOR.MINOR), or nothing when neither getconf nor ldd names one:
# "glibc 2.31"; "ldd (GNU libc) 2.31", "ldd (Ubuntu GLIBC 2.35-0ubuntu3.8) 2.35".
glibc_version() {
    g_out="$(getconf GNU_LIBC_VERSION 2>/dev/null || true)"
    g_version="$(printf '%s\n' "$g_out" | sed -n '1s/^glibc \([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p')"
    if [ -z "$g_version" ]; then
        g_out="$(ldd --version 2>&1 </dev/null || true)"
        g_version="$(printf '%s\n' "$g_out" | sed -n '1s/^ldd .*) \([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p')"
    fi
    printf '%s' "$g_version"
}

glibc_hint() {
    printf '%s' "quickhop's Linux build needs glibc ${min_glibc} or later (Debian 11, Ubuntu 20.04, Fedora 32, RHEL 9 or newer)"
}

# Refuses a Linux system the release binary cannot start on, before anything is downloaded.
preflight_linux() {
    # musl's ldd prints its version on stderr and exits 1.
    p_ldd="$(ldd --version 2>&1 </dev/null || true)"
    case "$p_ldd" in
        *[Mm]usl*)
            fail "this system uses musl libc (Alpine, Void musl), and $(glibc_hint); build quickhop from source instead: ${source_url}"
            ;;
    esac
    p_glibc="$(glibc_version)"
    if [ -n "$p_glibc" ] && ! version_at_least "$p_glibc" "$min_glibc"; then
        fail "$(glibc_hint), and this system has glibc ${p_glibc}; build quickhop from source instead: ${source_url}"
    fi
}

# The same rules as quickhop's own config and data directories on Linux and macOS.
config_dir() {
    printf '%s\n' "${XDG_CONFIG_HOME:-${HOME}/.config}/quickhop"
}

data_dir() {
    printf '%s\n' "${XDG_DATA_HOME:-${HOME}/.local/share}/quickhop"
}

share_dir() {
    printf '%s\n' "${XDG_DATA_HOME:-${HOME}/.local/share}"
}

# /Applications when writable, else ~/Applications; QUICKHOP_APP_DIR overrides both.
app_dir() {
    if [ -n "${QUICKHOP_APP_DIR:-}" ]; then
        printf '%s\n' "$QUICKHOP_APP_DIR"
    elif [ -w /Applications ]; then
        printf '%s\n' /Applications
    else
        printf '%s\n' "${HOME}/Applications"
    fi
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" </dev/null | cut -d ' ' -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" </dev/null | cut -d ' ' -f 1
    else
        fail "need sha256sum or shasum to verify the download; install one (coreutils or perl) and run the command again"
    fi
}

# Every request: retry transient failures, give up on a connection that never opens, and abort
# a transfer that stalls (under 1 KB/s for 30 s) instead of hanging. --retry alone retries only
# timeouts and HTTP 408/429/5xx; --retry-all-errors (curl 7.71+) retries a cut connection
# (`curl: (35) ...`) too. No --fail: fetch judges the HTTP status itself, so a 404 is reported
# at once instead of being retried.
net() {
    if [ "$retry_all" = true ]; then
        curl --silent --show-error --location --connect-timeout 15 --retry 5 --retry-delay 2 \
            --speed-limit 1024 --speed-time 30 --retry-all-errors "$@" </dev/null
    else
        curl --silent --show-error --location --connect-timeout 15 --retry 5 --retry-delay 2 \
            --speed-limit 1024 --speed-time 30 "$@" </dev/null
    fi
}

# The host[:port] of URL $1.
url_host() {
    printf '%s\n' "$1" | sed 's,^[A-Za-z][A-Za-z0-9+.-]*://\([^/]*\).*,\1,'
}

# Downloads asset $2 from URL directory $1 into file $3, and fails with one line on anything
# but a non-empty file: a 404 (no such release or asset), a network error (curl's own reason),
# another HTTP status, or an empty body.
fetch() {
    d_url="${1}/${2}"
    d_status=0
    d_code="$(net --stderr "${3}.err" --write-out '%{http_code}' --output "$3" "$d_url")" || d_status=$?
    # A download ended by a signal (Ctrl+C, a closed terminal, a kill) exits with that
    # signal's status, as the HUP, INT and TERM traps do. zsh 5.8 runs the TERM and INT traps
    # only after the command that waited on curl, and returns from the HUP trap's exit into
    # it, so the cut download would otherwise be reported as a network error.
    if [ "$d_status" -gt 128 ]; then
        exit "$d_status"
    fi
    if [ -n "$signalled" ]; then
        exit "$signalled"
    fi
    if [ "$d_status" -ne 0 ]; then
        d_reason="$(sed -n 's/^curl: ([0-9]*) //p' "${3}.err" | tail -n 1)"
        case "$d_url" in
            file:*) fail "could not read ${d_url} (${d_reason:-no reason given}); check QUICKHOP_INSTALL_BASE" ;;
        esac
        fail "could not reach $(url_host "$d_url") (${d_reason:-no answer}); check the network or proxy and run the command again"
    fi
    rm -f "${3}.err"
    # A file:// base (a local mirror) has no HTTP status: curl reports 000.
    case "${d_code}:${d_url%%://*}" in
        200:* | 000:file) ;;
        404:*)
            if [ -n "$version" ]; then
                fail "release v${version} not found (HTTP 404); the versions are at ${releases_url}"
            fi
            fail "release not found: ${d_url} (HTTP 404); the versions are at ${releases_url}"
            ;;
        *) fail "could not download ${d_url} (HTTP ${d_code}); run the command again later" ;;
    esac
    [ -s "$3" ] || fail "${2} from ${1} is empty; run the command again"
}

# Checks archive file $1 (asset name $2) against the checksum file $3 (its name $4): the line
# naming the asset, as sha256sum writes it (`<hash>  <name>`, or `<hash> *<name>`).
verify() {
    c_expected="$(awk -v name="$2" '$2 == name || $2 == "*" name { print $1; exit }' "$3")"
    expr "X${c_expected}" : 'X[0-9a-f]\{64\}$' >/dev/null ||
        fail "${4} holds no SHA-256 checksum for ${2}; nothing was installed, run the command again"
    c_actual="$(sha256_of "$1")"
    [ "$c_expected" = "$c_actual" ] ||
        fail "checksum mismatch for ${2} (expected ${c_expected}, got ${c_actual}); nothing was installed, run the command again"
}

# Puts file $1 at $2 with mode $3: copied beside it, then renamed over it, so a running daemon
# keeps its old inode and a failed copy never leaves a half-written file behind.
place() {
    p_dir="$(dirname "$2")"
    mkdir -p "$p_dir"
    p_tmp="${p_dir}/.$(basename "$2").new"
    rm -f "$p_tmp"
    cp "$1" "$p_tmp"
    chmod "$3" "$p_tmp"
    mv -f "$p_tmp" "$2"
}

# Makes $2 a symlink to $1, replacing a file or link there in one rename.
place_link() {
    p_dir="$(dirname "$2")"
    mkdir -p "$p_dir"
    p_tmp="${p_dir}/.$(basename "$2").new"
    rm -f "$p_tmp"
    ln -s "$1" "$p_tmp"
    mv -f "$p_tmp" "$2"
}

# Sets `new_version` from the `--version` line of the downloaded program $1, run before
# anything is replaced: a build this system cannot start (too old a glibc, a missing library)
# fails the install with the loader's own message, and leaves an installed copy as it was.
check_runnable() {
    if r_out="$("$1" --version 2>&1 </dev/null)" &&
        expr "X${r_out}" : 'Xquickhop [0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*$' >/dev/null; then
        new_version="${r_out#quickhop }"
        return 0
    fi
    if [ "$system" = Linux ]; then
        fail "the downloaded quickhop does not run on this system (${r_out:-no output}); nothing was replaced. $(glibc_hint); build from source on older systems: ${source_url}"
    fi
    fail "the downloaded quickhop does not run on this system (${r_out:-no output}); nothing was replaced, build from source: ${source_url}"
}

# The executable of process $1, or nothing.
process_exe() {
    if [ -L "/proc/$1/exe" ]; then
        readlink "/proc/$1/exe" 2>/dev/null || true
    else
        ps -o comm= -p "$1" 2>/dev/null || true
    fi
}

# How the daemon with pid $1, running from $2, was started, so it can be started again the same
# way: "launchd" (its LaunchAgent), "app" (quickhop.app opened), "systemd UNIT" (a quickhop
# systemd user unit, including the one XDG autostart generates), or "spawn" (a `quickhop
# daemon` from a terminal; "spawn --no-window" when it ran without the window).
start_method() {
    if [ "$system" = Darwin ]; then
        if launchctl print "gui/$(id -u)/${launchd_label}" 2>/dev/null </dev/null |
            grep -Eq "^[[:space:]]*pid = ${1}\$"; then
            echo launchd
            return 0
        fi
        case "$2" in
            *.app/Contents/MacOS/*)
                echo app
                return 0
                ;;
        esac
    elif [ -r "/proc/${1}/cgroup" ]; then
        # "0::/user.slice/user-1000.slice/user@1000.service/app.slice/quickhop.service"; only a
        # unit of quickhop's own is restarted, never the terminal or session it runs in.
        s_unit="$(sed -n 's|^0::.*/user@[0-9]*\.service/\(.*/\)\{0,1\}\([^/]*\)$|\2|p' "/proc/${1}/cgroup")"
        case "$s_unit" in
            quickhop*.service | app-quickhop*.service)
                echo "systemd ${s_unit}"
                return 0
                ;;
        esac
    fi
    case " $(ps -o args= -p "$1" 2>/dev/null || true) " in
        *" --no-window "*) echo "spawn --no-window" ;;
        *) echo spawn ;;
    esac
}

# Waits until process $1 has ended; false when it still runs after `daemon_wait`.
wait_gone() {
    w_tries=0
    while kill -0 "$1" 2>/dev/null; do
        [ "$w_tries" -lt "$daemon_wait" ] || return 1
        w_tries=$((w_tries + 1))
        sleep 0.1
    done
}

# Stops the daemon that answered `daemon status` with line $2, using the new binary $1, before
# anything is replaced. Sets `daemon_method` to restart it with.
stop_daemon() {
    s_pid="$(printf '%s\n' "$2" | sed -n '1s/^running (pid \([0-9][0-9]*\)).*/\1/p')"
    [ -n "$s_pid" ] ||
        fail "cannot read the daemon's pid from: ${2}; stop it (quickhop daemon stop) and run the command again"
    s_exe="$(printf '%s\n' "$2" | sed -n '1s/.*; exe \(.*\); clipboard:.*/\1/p')"
    if [ -z "$s_exe" ]; then
        s_exe="$(printf '%s\n' "$2" | sed -n '1s/.*; exe \(.*\)$/\1/p')"
    fi
    if [ -z "$s_exe" ]; then
        s_exe="$(process_exe "$s_pid")"
    fi
    daemon_method="$(start_method "$s_pid" "$s_exe")"
    case "$daemon_method" in
        systemd\ *) systemctl --user stop "${daemon_method#systemd }" </dev/null >/dev/null 2>&1 || true ;;
        *) "$1" daemon stop </dev/null >/dev/null 2>&1 || true ;;
    esac
    wait_gone "$s_pid" ||
        fail "the running quickhop daemon (pid ${s_pid}) did not stop; nothing was replaced. Stop it (kill ${s_pid}) and run the command again"
    step "Stopped the running daemon" "(pid ${s_pid}) for the update"
}

# Starts `quickhop daemon` with the arguments given, detached from this script.
spawn_daemon() {
    if command -v setsid >/dev/null 2>&1; then
        setsid "$bin" daemon "$@" </dev/null >/dev/null 2>&1 &
    else
        nohup "$bin" daemon "$@" </dev/null >/dev/null 2>&1 &
    fi
}

# Starts the daemon again the way `daemon_method` says, from the installed `bin` (and `app` on
# macOS), and checks that the one now answering is `new_version`.
restart_daemon() {
    case "$daemon_method" in
        launchd)
            launchctl kickstart "gui/$(id -u)/${launchd_label}" </dev/null >/dev/null 2>&1 ||
                fail "could not start the quickhop LaunchAgent again; start it with: quickhop daemon"
            r_how="its LaunchAgent"
            ;;
        app)
            open -g "$app" </dev/null >/dev/null 2>&1 ||
                fail "could not open ${app} again; open it from Finder"
            r_how="quickhop.app"
            ;;
        systemd\ *)
            r_unit="${daemon_method#systemd }"
            systemctl --user start "$r_unit" </dev/null >/dev/null 2>&1 ||
                fail "could not start ${r_unit} again; run: systemctl --user start ${r_unit}"
            r_how="systemd unit ${r_unit}"
            ;;
        "spawn --no-window")
            spawn_daemon --no-window
            r_how="quickhop daemon --no-window"
            ;;
        *)
            spawn_daemon
            r_how="quickhop daemon"
            ;;
    esac
    r_tries=0
    until r_status="$("$bin" daemon status 2>/dev/null </dev/null)"; do
        [ "$r_tries" -lt "$daemon_wait" ] ||
            fail "the updated quickhop daemon did not start; start it with: quickhop daemon"
        r_tries=$((r_tries + 1))
        sleep 0.1
    done
    r_version="$(printf '%s\n' "$r_status" | sed -n '1s/.*; version \([0-9][0-9A-Za-z.+-]*\).*/\1/p')"
    if [ -n "$r_version" ] && [ "$r_version" != "$new_version" ]; then
        fail "the daemon now running is version ${r_version}, not ${new_version}; restart it: quickhop daemon stop, then quickhop daemon"
    fi
    r_pid="$(printf '%s\n' "$r_status" | sed -n '1s/^running (pid \([0-9][0-9]*\)).*/\1/p')"
    step "Restarted the daemon" "(pid ${r_pid}, ${r_how})"
}

# Where macOS lists the apps allowed to control the computer: macOS 27 renamed the
# Accessibility list "Device Control and Data Access".
accessibility_pane() {
    a_major="$(sw_vers -productVersion 2>/dev/null </dev/null || true)"
    a_major="${a_major%%.*}"
    case "$a_major" in
        '' | *[!0-9]*) a_major=0 ;;
    esac
    if [ "$a_major" -ge 27 ]; then
        printf '%s\n' "System Settings → Privacy & Security → Device Control and Data Access"
    else
        printf '%s\n' "System Settings → Privacy & Security → Accessibility"
    fi
}

install_release() {
    bin="${bin_dir}/quickhop"
    app=""
    daemon_method=""
    detect_platform
    if [ "$system" = Linux ]; then
        preflight_linux
    fi
    if [ -n "$from" ]; then
        [ -d "$from" ] || fail "--from ${from} is not a directory; give the folder holding the release assets"
        base="$(cd "$from" && pwd)"
        source_label="from $(tildify "$base")"
    elif [ -n "${QUICKHOP_INSTALL_BASE:-}" ]; then
        base="${QUICKHOP_INSTALL_BASE%/}"
        source_label="from ${base}"
        if [ -n "$version" ]; then
            source_label="release v${version} ${source_label}"
        fi
    elif [ -n "$version" ]; then
        base="https://github.com/${repo}/releases/download/v${version}"
        source_label="release v${version}"
    else
        base="https://github.com/${repo}/releases/latest/download"
        source_label="latest release"
    fi
    archive="quickhop-${target}.tar.gz"

    if [ -z "$from" ]; then
        command -v curl >/dev/null 2>&1 || fail "curl is required; install curl and run the command again"
        if curl --retry-all-errors --version </dev/null >/dev/null 2>&1; then
            retry_all=true
        fi
    fi
    command -v tar >/dev/null 2>&1 || fail "tar is required; install tar and run the command again"
    # One private folder for every download and the unpacked archive, removed on every exit.
    # The traps are set before the folder exists, and HUP, INT and TERM exit through the EXIT
    # trap, so Ctrl+C, a closed terminal or a kill leaves nothing behind either. Each records
    # its status for fetch.
    trap remove_work EXIT
    trap 'signalled=129; exit 129' HUP
    trap 'signalled=130; exit 130' INT
    trap 'signalled=143; exit 143' TERM
    work="$(mktemp -d "${TMPDIR:-/tmp}/quickhop-install.XXXXXX")" ||
        fail "could not create a temporary folder in ${TMPDIR:-/tmp}; set TMPDIR to a writable folder"

    step "${platform}:" "${archive}, ${source_label}"
    if [ -n "$from" ]; then
        [ -f "${base}/${archive}" ] ||
            fail "no ${archive} in ${base}; download it from ${releases_url} into that folder"
        [ -s "${base}/${archive}" ] || fail "${archive} in ${base} is empty; download it again"
        cp "${base}/${archive}" "${work}/${archive}"
        if [ -f "${base}/SHA256SUMS" ]; then
            sums="SHA256SUMS"
        elif [ -f "${base}/${archive}.sha256" ]; then
            sums="${archive}.sha256"
        else
            fail "no SHA256SUMS or ${archive}.sha256 in ${base}; download it from ${releases_url} into that folder"
        fi
        cp "${base}/${sums}" "${work}/${sums}"
        verify "${work}/${archive}" "$archive" "${work}/${sums}" "$sums"
        step "Copied and verified" "(SHA-256 from ${sums})"
    else
        sums="SHA256SUMS"
        fetch "$base" "$archive" "${work}/${archive}"
        fetch "$base" "$sums" "${work}/${sums}"
        verify "${work}/${archive}" "$archive" "${work}/${sums}" "$sums"
        step "Downloaded and verified" "(SHA-256 from ${sums})"
    fi

    tar -xzf "${work}/${archive}" -C "$work" </dev/null 2>/dev/null ||
        fail "${archive} is not a valid archive; nothing was installed, run the command again"
    pkg="${work}/quickhop-${target}"
    [ -f "${pkg}/quickhop" ] ||
        fail "${archive} has no quickhop-${target}/quickhop; nothing was installed, report it at https://github.com/${repo}/issues"
    check_runnable "${pkg}/quickhop"
    if [ -n "$version" ] && [ "$new_version" != "$version" ]; then
        fail "the archive holds quickhop ${new_version}, not quickhop ${version}; nothing was installed, pick a version from ${releases_url}"
    fi

    # The new binary speaks to a daemon of any version over its socket.
    if d_status="$("${pkg}/quickhop" daemon status 2>/dev/null </dev/null)"; then
        stop_daemon "${pkg}/quickhop" "$d_status"
    fi

    if [ -d "${pkg}/quickhop.app" ]; then
        app="$(app_dir)/quickhop.app"
        mkdir -p "$(dirname "$app")"
        rm -rf "${app}.new"
        cp -R "${pkg}/quickhop.app" "${app}.new"
        rm -rf "$app"
        mv "${app}.new" "$app"
        xattr -dr com.apple.quarantine "$app" </dev/null 2>/dev/null || true
        step "Installed $(tildify "$app")"
        # One program for the app and the command line: macOS keeps one Accessibility grant.
        place_link "${app}/Contents/MacOS/quickhop" "$bin"
        step "Linked $(tildify "$bin")" "→ quickhop.app/Contents/MacOS/quickhop"
    else
        place "${pkg}/quickhop" "$bin" 755
        step "Installed $(tildify "$bin")"
    fi
    if [ -f "${pkg}/share/applications/quickhop.desktop" ]; then
        place "${pkg}/share/applications/quickhop.desktop" \
            "$(share_dir)/applications/quickhop.desktop" 644
        place "${pkg}/share/icons/hicolor/scalable/apps/quickhop.svg" \
            "$(share_dir)/icons/hicolor/scalable/apps/quickhop.svg" 644
        step "Installed the desktop entry and icon" "in $(tildify "$(share_dir)")"
    fi
    if [ -n "$daemon_method" ]; then
        restart_daemon
    fi

    installed="$("$bin" --version 2>&1 </dev/null)" ||
        fail "the installed ${bin} does not run (${installed}); run the command again"
    printf '%s%s is installed.%s\n' "$bold" "$installed" "$reset"
    printf '\n%sNext:%s\n' "$bold" "$reset"
    case ":${PATH}:" in
        *":${bin_dir}:"*) ;;
        *)
            # The line for the user's profile, with $HOME and $PATH left for it to expand.
            case "$bin_dir" in
                "$HOME"/*) path_entry="\$HOME/${bin_dir#"$HOME"/}" ;;
                *) path_entry="$bin_dir" ;;
            esac
            next "Put $(tildify "$bin_dir") on your PATH (in ~/.profile, ~/.zprofile for zsh): export PATH=\"${path_entry}:\$PATH\""
            ;;
    esac
    if [ -n "$daemon_method" ]; then
        if [ -n "$app" ]; then
            next "Press ${hotkey} to open the launcher"
        else
            next "Try it: quickhop query 5 km to miles"
        fi
    elif [ -n "$app" ]; then
        next "Start quickhop now and at every login: quickhop autostart on"
        next "Allow pasting when it asks: $(accessibility_pane) → quickhop"
        next "Then press ${hotkey} to open the launcher"
    else
        next "Check the setup: quickhop doctor"
        next "Start the daemon now and at every login: quickhop autostart on"
        next "Then try it: quickhop query 5 km to miles"
    fi
}

# Removes path $1 and says so; absent paths are skipped quietly.
remove() {
    if [ -e "$1" ] || [ -L "$1" ]; then
        rm -rf "$1"
        step "Removed $(tildify "$1")"
    fi
}

uninstall() {
    bin="${bin_dir}/quickhop"
    if [ -x "$bin" ]; then
        # Best effort: a daemon that is not running and autostart that is off are both fine.
        "$bin" daemon stop </dev/null >/dev/null 2>&1 || true
        "$bin" autostart off </dev/null >/dev/null 2>&1 || true
        step "Stopped the daemon and turned autostart off"
    else
        step "Nothing installed in $(tildify "$bin_dir")"
    fi
    remove "$bin"
    remove "$(app_dir)/quickhop.app"
    remove "$(share_dir)/applications/quickhop.desktop"
    remove "$(share_dir)/icons/hicolor/scalable/apps/quickhop.svg"
    for u_dir in "$(config_dir)" "$(data_dir)"; do
        if [ "$purge" = true ]; then
            if [ -e "$u_dir" ]; then
                rm -rf "$u_dir"
                step "Deleted $(tildify "$u_dir")" "(--purge)"
            fi
        elif [ -d "$u_dir" ]; then
            step "Kept $(tildify "$u_dir")" "(add --purge to delete it)"
        fi
    done
    printf '%squickhop is uninstalled.%s\n' "$bold" "$reset"
}

main() {
    setup_colour
    opt_version=""
    from=""
    action=install
    purge=false
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --version)
                [ "$#" -ge 2 ] || fail "--version needs a value; give one such as --version 0.1.0"
                opt_version="$2"
                shift 2
                ;;
            --from)
                [ "$#" -ge 2 ] || fail "--from needs a directory; give the folder holding the release assets"
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
            *) fail "unknown option $1; run it with --help for the options" ;;
        esac
    done
    if [ "$purge" = true ] && [ "$action" != uninstall ]; then
        fail "--purge only goes with --uninstall; run it with --uninstall --purge"
    fi
    if [ "$action" = uninstall ] && { [ -n "$opt_version" ] || [ -n "$from" ]; }; then
        fail "--uninstall takes no --version or --from; run it with --uninstall alone"
    fi
    # The option wins over QUICKHOP_VERSION.
    version="${opt_version:-${QUICKHOP_VERSION:-}}"
    version="${version#v}"
    if [ "$action" = install ] && [ -n "$version" ] &&
        ! expr "X${version}" : 'X[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*$' >/dev/null; then
        fail "--version wants MAJOR.MINOR.PATCH, got ${version}; give one such as 0.1.0"
    fi
    [ -n "${HOME:-}" ] || fail "HOME is not set; set it to your home folder and run the command again"
    bin_dir="${QUICKHOP_INSTALL_DIR:-${HOME}/.local/bin}"

    printf '%squickhop installer%s\n' "$bold" "$reset"
    if [ "$action" = uninstall ]; then
        uninstall
    else
        install_release
    fi
}

main "$@"
