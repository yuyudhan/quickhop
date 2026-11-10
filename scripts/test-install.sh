#!/usr/bin/env bash
# End-to-end test of scripts/install.sh and scripts/dist.sh: packages a stub quickhop binary with
# dist.sh, then installs it from a directory (--from) and over HTTP (127.0.0.1), runs it, updates
# it over a running daemon, uninstalls and purges, all inside a throwaway HOME. It also checks
# that dist.sh refuses a single-architecture "universal" macOS binary, and that the installer
# fails loudly on a 404, an empty download, a bad checksum, a binary this system cannot start
# and a daemon that does not stop. A stub keeps the test fast and lets it record what the
# installer runs; the release workflow covers the real binary.
#
# On macOS the stub is a tiny universal (arm64 + x86_64) C program that runs a shell script, so
# dist.sh's lipo check accepts it, wrapped in a quickhop.app signed ad hoc with the app's
# identifier, as scripts/bundle-macos.sh signs the real one.
#
# Every installer run goes through `sandbox`, which starts from an empty environment (env -i)
# with only HOME (the temp dir), a minimal PATH, TERM=dumb and QUICKHOP_APP_DIR inside HOME, so
# nothing the caller has set - XDG_CONFIG_HOME, QUICKHOP_*, a real HOME, a writable
# /Applications - can point the installer at real files. The harness proves that about itself:
# it plants decoy directories in its own environment and fails if anything touches them.
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

# Release assets for stub version $1 in directory $2.
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
}

dist="${work}/dist"
old="${work}/dist-old"
make_dist 9.8.7 "$dist"
make_dist 9.8.6 "$old"

sandbox() {
    env -i HOME="$home" PATH=/usr/bin:/bin TERM=dumb QUICKHOP_APP_DIR="${home}/Applications" "$@"
}
installer=("${dist}/install.sh")
bin="${home}/.local/bin/quickhop"
desktop="${home}/.local/share/applications/quickhop.desktop"
icon="${home}/.local/share/icons/hicolor/scalable/apps/quickhop.svg"
app="${home}/Applications/quickhop.app"

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

# 1. The sandbox environment is exactly HOME, PATH, TERM and QUICKHOP_APP_DIR.
seen="$(sandbox env | cut -d = -f 1 | sort | tr '\n' ' ')"
if [ "$seen" = "HOME PATH QUICKHOP_APP_DIR TERM " ]; then
    pass "sandbox sees only HOME, PATH, QUICKHOP_APP_DIR and TERM"
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

# 4. Install from a local directory.
if out="$(sandbox bash "${installer[@]}" --from "$dist" 2>&1)" &&
    [[ "$out" == *"checksum ok"* ]] && [ -x "$bin" ] && desktop_files true; then
    pass "--from installs the binary and the desktop files"
else
    fail "--from install: ${out}"
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
sandbox bash "${old}/install.sh" --from "$old" >/dev/null 2>&1
sandbox "$bin" daemon --no-window </dev/null >/dev/null 2>&1 &
for _ in $(seq 50); do
    sandbox "$bin" daemon status >/dev/null 2>&1 && break
    sleep 0.1
done
before="$(cat "${home}/daemon.pid" 2>/dev/null || true)"
if out="$(sandbox bash "${installer[@]}" --from "$dist" 2>&1)" &&
    [[ "$out" == *"stopping the running quickhop daemon (pid ${before})"* ]] &&
    [[ "$out" == *"restarted the daemon: running (pid "*"; version 9.8.7; exe "* ]] &&
    ! kill -0 "$before" 2>/dev/null &&
    [[ "$(sandbox "$bin" daemon status 2>&1)" == "running (pid "*"; version 9.8.7;"* ]] &&
    [ "$(cat "${home}/daemon.args")" = "--no-window" ]; then
    pass "an update stops the running 9.8.6 daemon and starts 9.8.7 the same way (--no-window)"
else
    fail "update over a running daemon (pid ${before}): ${out}"
fi
stop_daemons

# 7. A daemon that does not stop fails the update before anything is replaced.
sandbox bash "${old}/install.sh" --from "$old" >/dev/null 2>&1
touch "${home}/stubborn"
sandbox "$bin" daemon </dev/null >/dev/null 2>&1 &
for _ in $(seq 50); do
    sandbox "$bin" daemon status >/dev/null 2>&1 && break
    sleep 0.1
done
if out="$(sandbox bash "${installer[@]}" --from "$dist" 2>&1)"; then
    fail "an update went ahead over a daemon that did not stop"
elif [[ "$out" == *"did not stop; nothing was replaced"* ]] &&
    [ "$(sandbox "$bin" --version 2>&1)" = "quickhop 9.8.6" ]; then
    pass "a daemon that does not stop fails the update, leaving 9.8.6 installed"
else
    fail "stubborn daemon: ${out}"
fi
stop_daemons
rm -f "${home}/stubborn"

# 8. Install over HTTP, pinned to the archive's version; a different pin is refused; a missing
# asset (404) and an empty one fail loudly.
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
if out="$(sandbox QUICKHOP_INSTALL_BASE="$base" bash "${installer[@]}" --version 9.8.7 2>&1)" &&
    [ -x "$bin" ]; then
    pass "HTTP install with a matching --version"
else
    fail "HTTP install: ${out}"
fi
if out="$(sandbox QUICKHOP_INSTALL_BASE="$base" bash "${installer[@]}" --version 1.2.3 2>&1)"; then
    fail "a --version the archive does not hold was accepted"
elif [[ "$out" == *"not quickhop 1.2.3"* ]]; then
    pass "a --version the archive does not hold is refused"
else
    fail "wrong --version refusal message: ${out}"
fi
if out="$(sandbox QUICKHOP_INSTALL_BASE="${base}/v0.0.0" bash "${installer[@]}" 2>&1)"; then
    fail "a 404 counted as an install"
elif [[ "$out" == *"(HTTP 404): there is no such release asset"* ]]; then
    pass "a missing release asset (404) fails with the reason"
else
    fail "404 message: ${out}"
fi
if out="$(sandbox QUICKHOP_INSTALL_BASE="${base}/empty" bash "${installer[@]}" 2>&1)"; then
    fail "an empty download counted as an install"
elif [[ "$out" == *"quickhop-${target}.tar.gz from ${base}/empty is empty"* ]]; then
    pass "an empty download fails with the reason"
else
    fail "empty download message: ${out}"
fi

# 9. A binary this system cannot start (a newer glibc's loader error) fails the install before
# the installed copy is touched.
broken="${work}/dist-broken"
make_dist broken "$broken"
if out="$(sandbox bash "${installer[@]}" --from "$broken" 2>&1)"; then
    fail "a binary that does not start was installed"
elif [[ "$out" == *"does not run on this system"*"GLIBC_2.99"* ]] &&
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
            bash "${installer[@]}" --from "$dist" 2>&1)"; then
            fail "C library '${kind}' was not refused"
        elif [[ "$out" != *"fetching"* ]] && [[ "$out" == *"needs glibc 2.31 or later"* ]] &&
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
out="$(sandbox bash "${installer[@]}" --uninstall 2>&1)"
log="$(cat "${home}/stub.log" 2>/dev/null || true)"
if [ ! -e "$bin" ] && [ ! -L "$bin" ] && desktop_files false && [[ "$log" == *"daemon stop"* ]] &&
    [[ "$log" == *"autostart off"* ]] && [ -f "${home}/.config/quickhop/config.toml" ] &&
    [ -f "${home}/.local/share/quickhop/state.sqlite" ]; then
    pass "--uninstall removes quickhop, keeps the config and data"
else
    fail "--uninstall: ${out} (stub log: ${log})"
fi

# 11. Purge names each path, then deletes only those.
out="$(sandbox bash "${installer[@]}" --uninstall --purge 2>&1)"
if [[ "$out" == *"--purge deletes: ${home}/.config/quickhop"* ]] &&
    [[ "$out" == *"--purge deletes: ${home}/.local/share/quickhop"* ]] &&
    [ ! -e "${home}/.config/quickhop" ] && [ ! -e "${home}/.local/share/quickhop" ] &&
    [ -d "${home}/.local/share" ]; then
    pass "--purge deletes the config and data directories only"
else
    fail "--purge: ${out}"
fi

# 12. A wrong checksum aborts without installing.
bad="${work}/bad"
mkdir -p "$bad"
cp "${dist}/quickhop-${target}.tar.gz" "$bad/"
echo "0000  quickhop-${target}.tar.gz" >"${bad}/quickhop-${target}.tar.gz.sha256"
sha="$(cut -d ' ' -f 1 <"${dist}/quickhop-${target}.tar.gz.sha256")"
wrong="$(printf '%s' "$sha" | tr '0-9a-f' '1-9a-f0')"
mkdir -p "${work}/bad2"
cp "${dist}/quickhop-${target}.tar.gz" "${work}/bad2/"
echo "${wrong}  quickhop-${target}.tar.gz" >"${work}/bad2/quickhop-${target}.tar.gz.sha256"
if out="$(sandbox bash "${installer[@]}" --from "$bad" 2>&1)"; then
    fail "a malformed checksum file was accepted"
elif [[ "$out" == *"holds no SHA-256 checksum"* ]] && [ ! -e "$bin" ]; then
    pass "a malformed checksum file aborts before installing"
else
    fail "malformed checksum: ${out}"
fi
if out="$(sandbox bash "${installer[@]}" --from "${work}/bad2" 2>&1)"; then
    fail "a wrong checksum was accepted"
elif [[ "$out" == *"checksum mismatch"* ]] && [ ! -e "$bin" ]; then
    pass "a wrong checksum aborts before installing"
else
    fail "wrong checksum: ${out}"
fi

# 13. Bad options are refused with a reason.
for args in "--purge" "--version 1.2" "--from" "--uninstall --from ${dist}" "--bogus"; do
    # shellcheck disable=SC2086 # split the option list on purpose
    if out="$(sandbox bash "${installer[@]}" $args 2>&1)"; then
        fail "accepted: ${args}"
    elif [[ "$out" == "quickhop: error: "* ]]; then
        pass "refuses ${args}"
    else
        fail "no error message for ${args}: ${out}"
    fi
done

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
