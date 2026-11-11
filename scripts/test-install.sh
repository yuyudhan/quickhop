#!/usr/bin/env bash
# End-to-end test of scripts/install.sh and scripts/dist.sh: packages a stub quickhop binary with
# dist.sh, then installs it from a directory (--from) and over HTTP (127.0.0.1), runs it, updates
# it over a running daemon, uninstalls and purges, all inside a throwaway HOME. It also checks
# that dist.sh refuses a single-architecture "universal" macOS binary, and that the installer
# fails loudly on a 404, an empty download, a bad checksum, a binary this system cannot start
# and a daemon that does not stop. It runs the documented one-line install
# (`curl -fsSL <url> | sh`) from an empty scratch folder against the local HTTP server, piped to
# sh, dash, bash and zsh (those the system has), and checks that neither the one-line install
# nor the installer (after a success, a 404, a bad checksum, TERM, INT or HUP) leaves anything in
# the folder it ran from or in TMPDIR, that output off a terminal (or with NO_COLOR) has no
# escape codes, and that every failure is exactly one error line. A stub keeps the test fast and
# lets it record what the installer runs; the release workflow covers the real binary.
#
# On macOS the stub is a tiny universal (arm64 + x86_64) C program that runs a shell script, so
# dist.sh's lipo check accepts it, wrapped in a quickhop.app signed ad hoc with the app's
# identifier, as scripts/bundle-macos.sh signs the real one.
#
# Every installer run goes through `sandbox`, which starts from an empty environment (env -i)
# with only HOME (the temp dir), a minimal PATH, TERM=dumb, an empty TMPDIR of its own and
# QUICKHOP_APP_DIR inside HOME, so nothing the caller has set - XDG_CONFIG_HOME, QUICKHOP_*, a
# real HOME, a writable /Applications - can point the installer at real files. The harness
# proves that about itself: it plants decoy directories in its own environment and fails if
# anything touches them.
#
# Usage: scripts/test-install.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
case "$(uname -s)" in
    Linux) target="$(uname -m)-unknown-linux-gnu" ;;
    Darwin) target="universal-apple-darwin" ;;
    *)
        echo "test-install: skipped, install.sh supports Linux and macOS only"
        exit 0
        ;;
esac
[ "$target" != "arm64-unknown-linux-gnu" ] || target="aarch64-unknown-linux-gnu"
macos=false
[ "$target" != universal-apple-darwin ] || macos=true

work="$(mktemp -d)"
home="${work}/home"
server=""
# Ends every stub daemon the test (or an installer it ran) started.
stop_daemons() {
    local pid
    pid="$(cat "${home}/daemon.pid" 2>/dev/null || true)"
    [ -z "$pid" ] || kill -9 "$pid" 2>/dev/null || true
    rm -f "${home}/daemon.pid"
}
cleanup() {
    stop_daemons
    if [ -n "$server" ]; then
        kill "$server" 2>/dev/null || true
    fi
    rm -rf "$work"
}
trap cleanup EXIT

decoy="${work}/decoy"
mkdir -p "$home" "${decoy}/config/quickhop" "${decoy}/bin" "${decoy}/apps"
touch "${decoy}/config/quickhop/config.toml" "${decoy}/bin/quickhop"
export XDG_CONFIG_HOME="${decoy}/config" XDG_DATA_HOME="${decoy}/data"
export QUICKHOP_INSTALL_DIR="${decoy}/bin" QUICKHOP_APP_DIR="${decoy}/apps"

failures=0
pass() {
    echo "ok   - $*"
}
fail() {
    echo "FAIL - $*"
    failures=$((failures + 1))
}

# A stand-in quickhop at version $1, as a shell script in $2: prints the version, runs a fake
# daemon (`daemon`, `daemon status`, `daemon stop`, with the status line the real daemon
# prints), and logs every other invocation to $HOME/stub.log. With $HOME/stubborn present the
# daemon ignores `daemon stop` and SIGTERM. Version "broken" fails as a binary built for a newer
# glibc does.
write_stub_script() {
    cat >"$2" <<EOF
#!/bin/sh
version="$1"
EOF
    cat >>"$2" <<'EOF'
if [ "$version" = broken ]; then
    echo "quickhop: /lib/libc.so.6: version \`GLIBC_2.99' not found (required by quickhop)" >&2
    exit 1
fi
[ "$1" = "--version" ] && { echo "quickhop $version"; exit 0; }
pidfile="$HOME/daemon.pid"
running() {
    pid="$(cat "$pidfile" 2>/dev/null)" && kill -0 "$pid" 2>/dev/null
}
case "$1 $2" in
    "daemon " | "daemon --no-window")
        echo $$ >"$pidfile"
        echo "$version" >"$HOME/daemon.version"
        echo "$2" >"$HOME/daemon.args"
        [ ! -e "$HOME/stubborn" ] || trap '' TERM
        while :; do sleep 0.1; done
        ;;
    "daemon status")
        if running; then
            echo "running (pid $pid); version $(cat "$HOME/daemon.version"); exe $0; clipboard: off"
            exit 0
        fi
        echo "quickhop: error: the quickhop daemon is not running" >&2
        exit 1
        ;;
esac
echo "$*" >>"$HOME/stub.log"
if [ "$1 $2" = "daemon stop" ] && [ ! -e "$HOME/stubborn" ] && running; then
    kill "$pid"
fi
EOF
    chmod 755 "$2"
}

# The stub binary for version $1 in directory $2/quickhop: the script itself on Linux; on macOS
# a C program for arm64 and x86_64 (or only $3 when given) that runs it.
make_stub() {
    local version="$1" dir="$2" arch="${3:-}" script
    mkdir -p "$dir"
    script="${dir}/stub.sh"
    write_stub_script "$version" "$script"
    if [ "$macos" = false ]; then
        cp "$script" "${dir}/quickhop"
        return 0
    fi
    cat >"${dir}/stub.c" <<EOF
#include <stdlib.h>
#include <unistd.h>
int main(int argc, char **argv) {
    char **args = calloc((size_t)argc + 2, sizeof *args);
    if (args == NULL) {
        return 127;
    }
    args[0] = "sh";
    args[1] = "${script}";
    for (int i = 1; i < argc; i++) {
        args[i + 1] = argv[i];
    }
    execv("/bin/sh", args);
    return 127;
}
EOF
    if [ -n "$arch" ]; then
        cc -arch "$arch" -o "${dir}/quickhop" "${dir}/stub.c"
    else
        cc -arch arm64 -arch x86_64 -o "${dir}/quickhop" "${dir}/stub.c"
    fi
}

# Release assets for stub version $1 in directory $2, with SHA256SUMS over their .sha256 files
# as scripts/release-finish.sh writes it for a release.
make_dist() {
    local version="$1" out="$2" stub="${work}/stub-$1"
    local app_args=()
    make_stub "$version" "$stub"
    if [ "$macos" = true ]; then
        mkdir -p "${stub}/quickhop.app/Contents/MacOS"
        cp "${stub}/quickhop" "${stub}/quickhop.app/Contents/MacOS/quickhop"
        codesign --force --sign - --identifier io.github.yuyudhan.quickhop \
            "${stub}/quickhop.app" 2>/dev/null
        app_args=(--app "${stub}/quickhop.app")
    fi
    "${root}/scripts/dist.sh" --target "$target" --bin "${stub}/quickhop" --out "$out" \
        "${app_args[@]}" >/dev/null
    (cd "$out" && cat ./*.sha256 >SHA256SUMS)
}

dist="${work}/dist"
old="${work}/dist-old"
make_dist 9.8.7 "$dist"
make_dist 9.8.6 "$old"

# The installer's TMPDIR: every run must leave it empty, whatever the outcome.
sbtmp="${work}/tmp"
mkdir -p "$sbtmp"
sandbox() {
    env -i HOME="$home" PATH=/usr/bin:/bin TERM=dumb TMPDIR="$sbtmp" \
        QUICKHOP_APP_DIR="${home}/Applications" "$@"
}
# The entries of directory $1 (dotfiles included), one per line.
entries() {
    find "$1" -mindepth 1 -maxdepth 1 -exec basename {} \;
}
# True when directory $1 holds nothing at all.
empty_dir() {
    [ -d "$1" ] && [ -z "$(entries "$1")" ]
}
# Fails the test when an installer run ($1 names it) left anything in its TMPDIR.
tmp_left() {
    if empty_dir "$sbtmp"; then
        pass "${1}: nothing left in TMPDIR"
    else
        fail "${1}: left in TMPDIR: $(entries "$sbtmp" | tr '\n' ' ')"
        find "$sbtmp" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
    fi
}
# True when text $1 holds no escape character (no colour, no cursor movement).
plain() {
    [[ "$1" != *$'\033'* ]]
}
installer=("${dist}/install.sh")
bin="${home}/.local/bin/quickhop"
desktop="${home}/.local/share/applications/quickhop.desktop"
icon="${home}/.local/share/icons/hicolor/scalable/apps/quickhop.svg"
app="${home}/Applications/quickhop.app"
releases="https://github.com/yuyudhan/quickhop/releases"
source_docs="https://github.com/yuyudhan/quickhop/blob/nightly/docs/install.md#from-source"

# The desktop files this OS gets are all present (present=true) or all absent (false).
desktop_files() {
    local want="$1"
    if [ "$macos" = true ]; then
        [ "$want" = true ] && [ -x "${app}/Contents/MacOS/quickhop" ] && return 0
        [ "$want" = false ] && [ ! -e "$app" ] && return 0
    else
        [ "$want" = true ] && [ -f "$desktop" ] && [ -f "$icon" ] && return 0
        [ "$want" = false ] && [ ! -e "$desktop" ] && [ ! -e "$icon" ] && return 0
    fi
    return 1
}

# 1. The sandbox environment is exactly HOME, PATH, TERM, TMPDIR and QUICKHOP_APP_DIR.
seen="$(sandbox env | cut -d = -f 1 | sort | tr '\n' ' ')"
if [ "$seen" = "HOME PATH QUICKHOP_APP_DIR TERM TMPDIR " ]; then
    pass "sandbox sees only HOME, PATH, QUICKHOP_APP_DIR, TERM and TMPDIR"
else
    fail "sandbox leaks variables: ${seen}"
fi

# 2. The archive and checksum dist.sh wrote match.
if (cd "$dist" && sha256sum -c "quickhop-${target}.tar.gz.sha256" >/dev/null 2>&1 ||
    shasum -a 256 -c "quickhop-${target}.tar.gz.sha256" >/dev/null 2>&1); then
    pass "dist.sh writes a matching .sha256"
else
    fail "dist.sh checksum does not verify"
fi

# 3. dist.sh refuses a universal-apple-darwin binary that is not arm64 + x86_64 (on Linux,
# where lipo cannot check it, any).
refused() {
    local why="$1" binary="$2" out
    if out="$("${root}/scripts/dist.sh" --target universal-apple-darwin --bin "$binary" \
        --out "${work}/refused" 2>&1)"; then
        fail "dist.sh packed ${why} as universal-apple-darwin"
    elif [[ "$out" == "dist: "* ]] && [ ! -e "${work}/refused" ]; then
        pass "dist.sh refuses ${why} as universal-apple-darwin"
    else
        fail "dist.sh refusal of ${why}: ${out}"
    fi
}
if [ "$macos" = true ]; then
    make_stub 9.8.7 "${work}/thin" arm64
    refused "an arm64-only binary" "${work}/thin/quickhop"
    refused "a shell script" "${work}/thin/stub.sh"
    if [ "$(lipo -archs "${work}/stub-9.8.7/quickhop")" = "x86_64 arm64" ]; then
        pass "the packed stub is universal (lipo -archs: x86_64 arm64)"
    else
        fail "the packed stub is not universal: $(lipo -archs "${work}/stub-9.8.7/quickhop")"
    fi
else
    refused "a Linux binary" "${work}/stub-9.8.7/quickhop"
fi

# 4. Install from a local directory, checked against its SHA256SUMS: the header, the steps, the
# installed version and Next, with no escape codes off a terminal even where TERM has colour.
if out="$(sandbox TERM=xterm-256color sh "${installer[@]}" --from "$dist" 2>&1)" &&
    [[ "$out" == "quickhop installer"$'\n'* ]] &&
    [[ "$out" == *"  ✓ Copied and verified (SHA-256 from SHA256SUMS)"* ]] &&
    [[ "$out" == *$'\nquickhop 9.8.7 is installed.\n\nNext:\n  '* ]] &&
    [[ "$out" == *"quickhop autostart on"* ]] &&
    [ -x "$bin" ] && desktop_files true; then
    pass "--from installs the binary and the desktop files, saying so in steps"
else
    fail "--from install: ${out}"
fi
if plain "$out"; then
    pass "output that is not a terminal has no escape codes"
else
    fail "escape codes in output that is not a terminal: ${out}"
fi
if [ "$macos" = true ]; then
    if [[ "$out" == *"  ✓ Installed ~/Applications/quickhop.app"$'\n'"  ✓ Linked ~/.local/bin/quickhop"* ]] &&
        [[ "$out" == *"Then press Alt+Shift+Space to open the launcher"* ]]; then
        pass "macOS steps name the app and the link, and Next the default hotkey"
    else
        fail "macOS steps: ${out}"
    fi
elif [[ "$out" == *"  ✓ Installed ~/.local/bin/quickhop"* ]] &&
    [[ "$out" == *"  ✓ Installed the desktop entry and icon in ~/.local/share"* ]]; then
    pass "Linux steps name the binary and the desktop entry"
else
    fail "Linux steps: ${out}"
fi
# shellcheck disable=SC2016 # the literal line for the user's profile
if [[ "$out" == *'Put ~/.local/bin on your PATH (in ~/.profile, ~/.zprofile for zsh): export PATH="$HOME/.local/bin:$PATH"'* ]]; then
    pass "a bin directory off PATH gets a Next line"
else
    fail "no PATH line in Next: ${out}"
fi

# 5. The installed binary runs; on macOS it is a symlink into quickhop.app, one program
# signed with the app's identifier.
if [ "$(sandbox "$bin" --version 2>&1)" = "quickhop 9.8.7" ]; then
    pass "installed binary runs"
else
    fail "installed binary does not run"
fi
if [ "$macos" = true ]; then
    if [ "$(readlink "$bin")" = "${app}/Contents/MacOS/quickhop" ]; then
        pass "the quickhop command links to quickhop.app's executable"
    else
        fail "the quickhop command is not a link into the app: $(ls -l "$bin")"
    fi
    signed="$(codesign -dv "$bin" 2>&1 || true)"
    if grep -qx "Identifier=io.github.yuyudhan.quickhop" <<<"$signed"; then
        pass "codesign sees the app's identity through the quickhop command"
    else
        fail "codesign -dv ${bin}: ${signed}"
    fi
fi

# 6. Updating over a running daemon stops it before replacing anything and starts the new one
# the same way, without a manual step.
rm -f "$bin"
sandbox sh "${old}/install.sh" --from "$old" >/dev/null 2>&1
sandbox "$bin" daemon --no-window </dev/null >/dev/null 2>&1 &
for _ in $(seq 50); do
    sandbox "$bin" daemon status >/dev/null 2>&1 && break
    sleep 0.1
done
before="$(cat "${home}/daemon.pid" 2>/dev/null || true)"
if out="$(sandbox sh "${installer[@]}" --from "$dist" 2>&1)" &&
    [[ "$out" == *"  ✓ Stopped the running daemon (pid ${before}) for the update"* ]] &&
    [[ "$out" == *"  ✓ Restarted the daemon (pid "*", quickhop daemon --no-window)"* ]] &&
    ! kill -0 "$before" 2>/dev/null &&
    [[ "$(sandbox "$bin" daemon status 2>&1)" == "running (pid "*"; version 9.8.7;"* ]] &&
    [ "$(cat "${home}/daemon.args")" = "--no-window" ]; then
    pass "an update stops the running 9.8.6 daemon and starts 9.8.7 the same way (--no-window)"
else
    fail "update over a running daemon (pid ${before}): ${out}"
fi
stop_daemons

# 7. A daemon that does not stop fails the update before anything is replaced.
sandbox sh "${old}/install.sh" --from "$old" >/dev/null 2>&1
touch "${home}/stubborn"
sandbox "$bin" daemon </dev/null >/dev/null 2>&1 &
for _ in $(seq 50); do
    sandbox "$bin" daemon status >/dev/null 2>&1 && break
    sleep 0.1
done
if out="$(sandbox sh "${installer[@]}" --from "$dist" 2>&1)"; then
    fail "an update went ahead over a daemon that did not stop"
elif [[ "$out" == *"did not stop; nothing was replaced"* ]] &&
    [ "$(sandbox "$bin" --version 2>&1)" = "quickhop 9.8.6" ]; then
    pass "a daemon that does not stop fails the update, leaving 9.8.6 installed"
else
    fail "stubborn daemon: ${out}"
fi
stop_daemons
rm -f "${home}/stubborn"

# 8. Install over HTTP, pinned to the archive's version by --version or QUICKHOP_VERSION (the
# option winning); a different pin is refused; a missing asset (404) and an empty one fail
# loudly.
mkdir -p "${dist}/empty"
: >"${dist}/empty/quickhop-${target}.tar.gz"
port=$((20000 + RANDOM % 20000))
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$dist" >/dev/null 2>&1 &
server=$!
base="http://127.0.0.1:${port}"
for _ in $(seq 50); do
    curl --silent --fail --output /dev/null "${base}/install.sh" && break
    sleep 0.1
done
rm -f "$bin"
if out="$(sandbox QUICKHOP_INSTALL_BASE="$base" sh "${installer[@]}" --version 9.8.7 2>&1)" &&
    [[ "$out" == *"release v9.8.7 from ${base}"* ]] &&
    [[ "$out" == *"  ✓ Downloaded and verified (SHA-256 from SHA256SUMS)"* ]] && [ -x "$bin" ]; then
    pass "HTTP install with a matching --version"
else
    fail "HTTP install: ${out}"
fi
tmp_left "an HTTP install"
rm -f "$bin"
if out="$(sandbox QUICKHOP_INSTALL_BASE="$base" QUICKHOP_VERSION=v9.8.7 sh "${installer[@]}" 2>&1)" &&
    [[ "$out" == *"release v9.8.7 from ${base}"* ]] && [ -x "$bin" ]; then
    pass "HTTP install with a matching QUICKHOP_VERSION"
else
    fail "HTTP install with QUICKHOP_VERSION: ${out}"
fi
tmp_left "an HTTP install with QUICKHOP_VERSION"
if out="$(sandbox QUICKHOP_INSTALL_BASE="$base" sh "${installer[@]}" --version 1.2.3 2>&1)"; then
    fail "a --version the archive does not hold was accepted"
elif [[ "$out" == *"not quickhop 1.2.3; nothing was installed"* ]]; then
    pass "a --version the archive does not hold is refused"
else
    fail "wrong --version refusal message: ${out}"
fi
tmp_left "a refused --version"
if out="$(sandbox QUICKHOP_INSTALL_BASE="$base" QUICKHOP_VERSION=1.2.3 sh "${installer[@]}" 2>&1)"; then
    fail "a QUICKHOP_VERSION the archive does not hold was accepted"
elif [[ "$out" == *"not quickhop 1.2.3; nothing was installed"* ]]; then
    pass "a QUICKHOP_VERSION the archive does not hold is refused"
else
    fail "wrong QUICKHOP_VERSION refusal message: ${out}"
fi
tmp_left "a refused QUICKHOP_VERSION"
if out="$(sandbox QUICKHOP_INSTALL_BASE="$base" QUICKHOP_VERSION=1.2.3 sh "${installer[@]}" --version 9.8.7 2>&1)" &&
    [[ "$out" == *"release v9.8.7 from ${base}"* ]]; then
    pass "--version wins over QUICKHOP_VERSION"
else
    fail "--version over QUICKHOP_VERSION: ${out}"
fi
tmp_left "--version over QUICKHOP_VERSION"
if out="$(sandbox QUICKHOP_INSTALL_BASE="${base}/v0.0.0" sh "${installer[@]}" 2>&1)"; then
    fail "a 404 counted as an install"
elif [[ "$out" == *"quickhop: error: release not found: ${base}/v0.0.0/quickhop-${target}.tar.gz (HTTP 404); the versions are at ${releases}" ]]; then
    pass "a missing release asset (404) fails with 'release not found' and its URL"
else
    fail "404 message: ${out}"
fi
tmp_left "a missing release asset"
if out="$(sandbox QUICKHOP_INSTALL_BASE="${base}/empty" sh "${installer[@]}" 2>&1)"; then
    fail "an empty download counted as an install"
elif [[ "$out" == *"quickhop: error: quickhop-${target}.tar.gz from ${base}/empty is empty; run the command again" ]]; then
    pass "an empty download fails with the reason"
else
    fail "empty download message: ${out}"
fi
tmp_left "an empty download"

# 8b. The documented one-line install (docs/install.md, "The installer script"; README.md opens
# with the same line), with the URL pointed at the local server's install.sh and the
# installer's assets taken from it too. Piped to every shell the system has (sh, dash, bash,
# zsh) from a new empty scratch folder, every path ends as the docs say and leaves nothing in
# that folder or in TMPDIR: an install, a second run over it, --version vX.Y.Z,
# QUICKHOP_VERSION, install.sh not found (404), no answer from the server, a missing release
# asset, a network error and a bad checksum in the installer, and an unsupported OS or CPU.
# Every failure prints exactly one error line, and the installer's own exit non-zero; when curl
# itself fails, `| sh` runs an empty script, so only curl's line says so.
pages="https://yuyudhan.github.io/quickhop/install"
one_line="$(awk '
    /^## The installer script$/ { section = 1; next }
    section && /^## / { exit }
    section && /^    curl / { sub(/^    /, ""); print; exit }
' "${root}/docs/install.md")"
if [ "$one_line" = "curl -fsSL ${pages} | sh" ]; then
    pass "docs/install.md has the one-line install"
else
    fail "no one-line install under \"## The installer script\" in docs/install.md: ${one_line}"
fi
# The first fenced block of README.md, after the title and tagline.
readme_first="$(awk '/^```/ { if (inside) exit; inside = 1; next } inside { print }' "${root}/README.md")"
if [ "$readme_first" = "$one_line" ]; then
    pass "README.md's first code block is docs/install.md's install line"
else
    fail "README.md's first code block is not the install line: ${readme_first}"
fi
# Runs the one-line install from scratch folder $1, piped to shell $2, with the URL replaced by
# $3, the installer's assets from $4, the installer options $5 (as the docs place them, after
# `| sh -s --`) and extra environment assignments after that (PATH=..., say).
one_line_run() {
    local dir="$1" shell="$2" url="$3" assets="$4" options="$5" line
    shift 5
    line="${one_line//"$pages"/$url}"
    line="${line%| sh}| ${shell}${options:+ -s -- ${options}}"
    (cd "$dir" && sandbox QUICKHOP_INSTALL_BASE="$assets" "$@" sh -c "$line")
}
# Fails the test when a one-line run ($1 names it) left anything in scratch folder $2 or TMPDIR.
scratch_left() {
    if empty_dir "$2"; then
        pass "${1}: nothing left in the folder it ran from"
    else
        fail "${1}: left in the folder it ran from: $(entries "$2" | tr '\n' ' ')"
        find "$2" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
    fi
    tmp_left "$1"
}
# The error lines (stderr) of a failed run: exactly one, holding $2; $1 names the case.
one_error() {
    local name="$1" want="$2" errors
    errors="$(cat "${work}/stderr")"
    if [ "$(printf '%s\n' "$errors" | wc -l | tr -d ' ')" = 1 ] && [[ "$errors" == *"$want"* ]] &&
        plain "$errors"; then
        pass "${name}: one error line: ${errors}"
    else
        fail "${name}: want one error line holding '${want}', got: ${errors}"
    fi
}
# A successful one-line run ($1 names it, in shell $2): the steps on stdout, none of curl's or
# tar's output, nothing on stderr, no escape codes, and 9.8.7 installed.
one_line_ok() {
    local name="$1" out errors
    out="$(cat "${work}/stdout")"
    errors="$(cat "${work}/stderr")"
    if [[ "$out" == "quickhop installer"$'\n'* ]] &&
        [[ "$out" == *"  ✓ Downloaded and verified (SHA-256 from SHA256SUMS)"* ]] &&
        [[ "$out" == *$'\nquickhop 9.8.7 is installed.\n\nNext:\n'* ]] &&
        [ -z "$errors" ] && plain "$out" && [ -x "$bin" ] &&
        [ "$(sandbox "$bin" --version 2>&1)" = "quickhop 9.8.7" ]; then
        pass "${name} installs quickhop 9.8.7, quietly"
    else
        fail "${name}: ${out} (stderr: ${errors})"
    fi
}
# A port nothing listens on: opened, then closed again.
closed_port="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
nowhere="http://127.0.0.1:${closed_port}"
# A release whose SHA256SUMS does not match its archive.
mkdir -p "${dist}/badsum"
cp "${dist}/quickhop-${target}.tar.gz" "${dist}/badsum/"
printf '%064d  %s\n' 0 "quickhop-${target}.tar.gz" >"${dist}/badsum/SHA256SUMS"
zeros="$(printf '%064d' 0)"
# Stand-in uname for systems the release has no build for.
for system in "FreeBSD amd64" "Linux riscv64"; do
    dir="${work}/uname-${system%% *}"
    mkdir -p "$dir"
    # shellcheck disable=SC2016 # the stand-in's own script, expanded when it runs
    printf '#!/bin/sh\ncase "$1" in -m) echo %s ;; *) echo %s ;; esac\n' \
        "${system#* }" "${system%% *}" >"${dir}/uname"
    chmod 755 "${dir}/uname"
done
shells=()
for shell in sh dash bash zsh; do
    if sandbox sh -c "command -v ${shell}" >/dev/null 2>&1; then
        shells+=("$shell")
    fi
done
echo "test-install: the one-line install piped to: ${shells[*]}"
for shell in "${shells[@]}"; do
    rm -f "$bin"
    scratch="$(mktemp -d "${work}/scratch.XXXXXX")"
    one_line_run "$scratch" "$shell" "${base}/install.sh" "$base" "" \
        >"${work}/stdout" 2>"${work}/stderr" || true
    one_line_ok "| ${shell}: the one-line install"
    scratch_left "| ${shell}: a one-line install" "$scratch"

    one_line_run "$scratch" "$shell" "${base}/install.sh" "$base" "" \
        >"${work}/stdout" 2>"${work}/stderr" || true
    one_line_ok "| ${shell}: running the one-line install again"
    scratch_left "| ${shell}: a second one-line install" "$scratch"

    rm -f "$bin"
    one_line_run "$scratch" "$shell" "${base}/install.sh" "$base" "--version v9.8.7" \
        >"${work}/stdout" 2>"${work}/stderr" || true
    one_line_ok "| ${shell} -s -- --version v9.8.7"
    scratch_left "| ${shell}: a one-line install with --version" "$scratch"

    rm -f "$bin"
    one_line_run "$scratch" "$shell" "${base}/install.sh" "$base" "" QUICKHOP_VERSION=9.8.7 \
        >"${work}/stdout" 2>"${work}/stderr" || true
    one_line_ok "| ${shell} with QUICKHOP_VERSION=9.8.7"
    scratch_left "| ${shell}: a one-line install with QUICKHOP_VERSION" "$scratch"

    # case|URL|assets|options|extra environment|status (fail, or any when curl fails)|the one error line holds
    for spec in \
        "install.sh not found|${base}/missing/install.sh|${base}|||any|curl: (22) The requested URL returned error: 404" \
        "no answer for install.sh|${nowhere}/install.sh|${base}|||any|curl: (7) Failed to connect to 127.0.0.1 port ${closed_port}" \
        "missing release asset|${base}/install.sh|${base}/v0.0.0|--version v0.0.0||fail|quickhop: error: release v0.0.0 not found (HTTP 404); the versions are at ${releases}" \
        "network error in the installer|${base}/install.sh|${nowhere}|||fail|quickhop: error: could not reach 127.0.0.1:${closed_port} (Failed to connect" \
        "bad checksum|${base}/install.sh|${base}/badsum|||fail|quickhop: error: checksum mismatch for quickhop-${target}.tar.gz (expected ${zeros}, got " \
        "unsupported OS|${base}/install.sh|${base}||PATH=${work}/uname-FreeBSD:/usr/bin:/bin|fail|quickhop: error: no quickhop build for FreeBSD amd64; build from source: ${source_docs}" \
        "unsupported CPU|${base}/install.sh|${base}||PATH=${work}/uname-Linux:/usr/bin:/bin|fail|quickhop: error: no quickhop build for Linux riscv64; build from source: ${source_docs}"; do
        IFS='|' read -r name url assets options extra status want <<<"$spec"
        rm -f "$bin"
        env_args=()
        [ -z "$extra" ] || env_args=("$extra")
        if one_line_run "$scratch" "$shell" "$url" "$assets" "$options" ${env_args[@]+"${env_args[@]}"} \
            >"${work}/stdout" 2>"${work}/stderr"; then
            code=0
        else
            code=$?
        fi
        if [ "$status" = fail ] && [ "$code" -ne 1 ]; then
            fail "| ${shell}: ${name}: exit status ${code}, not 1: $(cat "${work}/stdout" "${work}/stderr")"
        elif [ -e "$bin" ]; then
            fail "| ${shell}: ${name}: installed something"
        else
            pass "| ${shell}: ${name}: installs nothing (exit status ${code})"
        fi
        one_error "| ${shell}: ${name}" "$want"
        scratch_left "| ${shell}: ${name}" "$scratch"
    done
done

# 8c. TERM, Ctrl+C (INT) or a closed terminal (HUP) while the installer downloads ends it with
# that signal's status and leaves nothing in TMPDIR, in every shell. A stand-in curl first on
# the PATH answers the installer's `--version` probe, and for a download records its pid and
# hangs.
slow="${work}/slow-curl"
mkdir -p "$slow"
printf '#!/bin/sh\ncase "$*" in *--version*) echo "curl 8.0.0"; exit 0 ;; esac\necho $$ >"%s/curl.pid"\nexec sleep 30\n' \
    "$work" >"${slow}/curl"
chmod 755 "${slow}/curl"
for shell in "${shells[@]}"; do
    for signal in TERM INT HUP; do
        rm -f "${work}/curl.pid" "${work}/group.pid"
        # The installer runs in its own process group, started through Python so that, though
        # run in the background, it does not inherit an ignored INT; the signal goes to the
        # whole group, as Ctrl+C or a closed terminal does, so the hanging download ends with it.
        python3 -c 'import os, signal, sys
os.setsid()
signal.signal(signal.SIGINT, signal.SIG_DFL)
with open(sys.argv[1], "w") as out:
    out.write(str(os.getpid()))
os.execvp(sys.argv[2], sys.argv[2:])' "${work}/group.pid" env -i HOME="$home" \
            PATH="${slow}:/usr/bin:/bin" TERM=dumb TMPDIR="$sbtmp" \
            QUICKHOP_APP_DIR="${home}/Applications" QUICKHOP_INSTALL_BASE="$base" \
            "$shell" "${installer[@]}" >/dev/null 2>&1 &
        pid=$!
        for _ in $(seq 100); do
            [ -s "${work}/curl.pid" ] && break
            sleep 0.1
        done
        kill -s "$signal" -- "-$(cat "${work}/group.pid")" 2>/dev/null || true
        status=0
        wait "$pid" || status=$?
        curl_pid="$(cat "${work}/curl.pid" 2>/dev/null || true)"
        [ -z "$curl_pid" ] || kill "$curl_pid" 2>/dev/null || true
        case "$signal" in
            TERM) want=143 ;;
            INT) want=130 ;;
            HUP) want=129 ;;
        esac
        if [ -n "$curl_pid" ] && [ "$status" -eq "$want" ]; then
            pass "${shell}: ${signal} during a download ends the installer (status ${status})"
        else
            fail "${shell}: ${signal} during a download: status ${status} (want ${want}), curl pid '${curl_pid}'"
        fi
        tmp_left "${shell}: ${signal} during a download"
    done
done
# The checks below start from 9.8.7 installed, as section 8 left it.
if ! out="$(sandbox QUICKHOP_INSTALL_BASE="$base" sh "${installer[@]}" 2>&1)" || [ ! -x "$bin" ]; then
    fail "reinstalling 9.8.7 over HTTP: ${out}"
fi

# 9. A binary this system cannot start (a newer glibc's loader error) fails the install before
# the installed copy is touched.
broken="${work}/dist-broken"
make_dist broken "$broken"
if out="$(sandbox sh "${installer[@]}" --from "$broken" 2>&1)"; then
    fail "a binary that does not start was installed"
elif [[ "$out" == *"does not run on this system"*"GLIBC_2.99"*"nothing was replaced"* ]] &&
    { [ "$macos" = true ] || [[ "$out" == *"needs glibc 2.31 or later"* ]]; } &&
    [ "$(sandbox "$bin" --version 2>&1)" = "quickhop 9.8.7" ]; then
    pass "a binary that does not start is refused, the installed one kept"
else
    fail "unrunnable binary: ${out}"
fi

# 9b. Linux: a system the release cannot run on (musl, a glibc older than 2.31) is refused
# before anything is fetched, with what to do instead. Stand-in ldd and getconf on the PATH
# say what such a system says.
if [ "$macos" = false ]; then
    fake="${work}/fakebin"
    mkdir -p "${fake}/musl" "${fake}/old"
    printf '#!/bin/sh\necho "musl libc (aarch64)" >&2\nexit 1\n' >"${fake}/musl/ldd"
    printf '#!/bin/sh\nexit 1\n' >"${fake}/musl/getconf"
    printf '#!/bin/sh\necho "glibc 2.28"\n' >"${fake}/old/getconf"
    chmod 755 "${fake}"/*/*
    for kind in musl old; do
        if out="$(env -i HOME="$home" PATH="${fake}/${kind}:/usr/bin:/bin" TERM=dumb \
            sh "${installer[@]}" --from "$dist" 2>&1)"; then
            fail "C library '${kind}' was not refused"
        elif [[ "$out" != *"✓"* ]] && [[ "$out" == *"needs glibc 2.31 or later"* ]] &&
            { [ "$kind" = old ] || [[ "$out" == *"uses musl libc"* ]]; } &&
            { [ "$kind" = musl ] || [[ "$out" == *"this system has glibc 2.28"* ]]; }; then
            pass "C library '${kind}' (musl, or glibc 2.28) is refused before anything is fetched"
        else
            fail "${kind} C library refusal: ${out}"
        fi
    done
fi

# 10. Uninstall stops the daemon, turns autostart off and keeps the config and data.
mkdir -p "${home}/.config/quickhop" "${home}/.local/share/quickhop"
touch "${home}/.config/quickhop/config.toml" "${home}/.local/share/quickhop/state.sqlite"
rm -f "${home}/stub.log"
out="$(sandbox sh "${installer[@]}" --uninstall 2>&1)"
log="$(cat "${home}/stub.log" 2>/dev/null || true)"
if [ ! -e "$bin" ] && [ ! -L "$bin" ] && desktop_files false && [[ "$log" == *"daemon stop"* ]] &&
    [[ "$log" == *"autostart off"* ]] && [ -f "${home}/.config/quickhop/config.toml" ] &&
    [ -f "${home}/.local/share/quickhop/state.sqlite" ] &&
    [[ "$out" == *"  ✓ Removed ~/.local/bin/quickhop"* ]] &&
    [[ "$out" == *"  ✓ Kept ~/.config/quickhop (add --purge to delete it)"* ]] &&
    [[ "$out" == *"quickhop is uninstalled." ]]; then
    pass "--uninstall removes quickhop, keeps the config and data"
else
    fail "--uninstall: ${out} (stub log: ${log})"
fi

# 11. Purge names each path, then deletes only those.
out="$(sandbox sh "${installer[@]}" --uninstall --purge 2>&1)"
if [[ "$out" == *"  ✓ Deleted ~/.config/quickhop (--purge)"* ]] &&
    [[ "$out" == *"  ✓ Deleted ~/.local/share/quickhop (--purge)"* ]] &&
    [ ! -e "${home}/.config/quickhop" ] && [ ! -e "${home}/.local/share/quickhop" ] &&
    [ -d "${home}/.local/share" ]; then
    pass "--purge deletes the config and data directories only"
else
    fail "--purge: ${out}"
fi

# 11b. Colour only on a terminal: with TERM set and no NO_COLOR the steps and the error word
# are coloured; NO_COLOR or TERM=dumb leaves them plain. A pseudo-terminal (Python's pty)
# stands in for the user's. tty_run runs the installer with option $1 on one, with the
# environment assignments after it.
tty_run() {
    local option="$1"
    shift
    env -i HOME="$home" PATH=/usr/bin:/bin TMPDIR="$sbtmp" QUICKHOP_APP_DIR="${home}/Applications" \
        "$@" "$(command -v python3)" -c 'import os, pty, sys
pid, fd = pty.fork()
if pid == 0:
    os.execvp(sys.argv[1], sys.argv[1:])
out = b""
while True:
    try:
        data = os.read(fd, 4096)
    except OSError:
        break
    if not data:
        break
    out += data
os.waitpid(pid, 0)
sys.stdout.write(out.decode("utf-8", "replace"))' sh "${installer[@]}" "$option"
}
for option in "--uninstall" "--bogus"; do
    coloured="$(tty_run "$option" TERM=xterm-256color)"
    no_color="$(tty_run "$option" TERM=xterm-256color NO_COLOR=1)"
    dumb="$(tty_run "$option" TERM=dumb)"
    if ! plain "$coloured" && plain "$no_color" && plain "$dumb" &&
        [[ "$no_color" == *"quickhop"* ]] && [[ "$dumb" == *"quickhop"* ]]; then
        pass "${option} on a terminal: colour, none with NO_COLOR or TERM=dumb"
    else
        fail "${option} colour on a terminal: '${coloured}' / NO_COLOR '${no_color}' / dumb '${dumb}'"
    fi
done

# 12. A malformed or wrong checksum aborts without installing; without SHA256SUMS a --from
# folder is checked against the archive's .sha256.
bad="${work}/bad"
mkdir -p "$bad"
cp "${dist}/quickhop-${target}.tar.gz" "$bad/"
echo "0000  quickhop-${target}.tar.gz" >"${bad}/quickhop-${target}.tar.gz.sha256"
sha="$(cut -d ' ' -f 1 <"${dist}/quickhop-${target}.tar.gz.sha256")"
wrong="$(printf '%s' "$sha" | tr '0-9a-f' '1-9a-f0')"
mkdir -p "${work}/bad2"
cp "${dist}/quickhop-${target}.tar.gz" "${work}/bad2/"
echo "${wrong}  quickhop-${target}.tar.gz" >"${work}/bad2/quickhop-${target}.tar.gz.sha256"
if out="$(sandbox sh "${installer[@]}" --from "$bad" 2>&1)"; then
    fail "a malformed checksum file was accepted"
elif [[ "$out" == *"quickhop-${target}.tar.gz.sha256 holds no SHA-256 checksum"* ]] && [ ! -e "$bin" ]; then
    pass "a malformed checksum file aborts before installing"
else
    fail "malformed checksum: ${out}"
fi
if out="$(sandbox sh "${installer[@]}" --from "${work}/bad2" 2>&1)"; then
    fail "a wrong checksum was accepted"
elif [[ "$out" == *"quickhop: error: checksum mismatch for quickhop-${target}.tar.gz (expected ${wrong}, got ${sha}); nothing was installed, run the command again" ]] &&
    [ ! -e "$bin" ]; then
    pass "a wrong checksum aborts before installing"
else
    fail "wrong checksum: ${out}"
fi
tmp_left "a wrong checksum"

# 13. Bad options are refused with a reason, in one line; --help shows the piped form.
for args in "--purge" "--version 1.2" "--from" "--uninstall --from ${dist}" "--bogus"; do
    # shellcheck disable=SC2086 # split the option list on purpose
    if out="$(sandbox sh "${installer[@]}" $args 2>&1)"; then
        fail "accepted: ${args}"
    elif [[ "$out" == "quickhop: error: "*"; "* ]] && [[ "$out" != *$'\n'* ]]; then
        pass "refuses ${args}"
    else
        fail "no error message for ${args}: ${out}"
    fi
done
if out="$(sandbox sh "${installer[@]}" --help 2>&1)" && [[ "$out" == *"${pages}"* ]] &&
    [[ "$out" == *"| sh -s -- --version 0.1.0"* ]]; then
    pass "--help shows the piped form"
else
    fail "--help: ${out}"
fi

# 14. The caller's environment was never used.
if [ -f "${decoy}/config/quickhop/config.toml" ] && [ -f "${decoy}/bin/quickhop" ] &&
    [ -z "$(ls -A "${decoy}/apps")" ] && [ ! -e "${decoy}/data" ]; then
    pass "the caller's XDG and QUICKHOP_* locations were left alone"
else
    fail "the installer touched the caller's environment"
fi

if [ "$failures" -ne 0 ]; then
    echo "test-install: ${failures} failed"
    exit 1
fi
echo "test-install: all passed"
