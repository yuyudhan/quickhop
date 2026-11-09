#!/usr/bin/env bash
# End-to-end test of scripts/install.sh and scripts/dist.sh: packages a stub quickhop binary with
# dist.sh, then installs it from a directory (--from) and over HTTP (127.0.0.1), runs it,
# uninstalls and purges, all inside a throwaway HOME. A stub keeps the test fast and lets it
# record what the installer runs; scripts/bundle-linux.sh and the release build cover the real
# binary.
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

work="$(mktemp -d)"
server=""
cleanup() {
    if [ -n "$server" ]; then
        kill "$server" 2>/dev/null || true
    fi
    rm -rf "$work"
}
trap cleanup EXIT

home="${work}/home"
decoy="${work}/decoy"
dist="${work}/dist"
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

# A stand-in binary: prints a version and logs every other invocation to $HOME/stub.log.
stub="${work}/quickhop"
cat >"$stub" <<'EOF'
#!/bin/sh
if [ "$1" = "--version" ]; then
    echo "quickhop 9.8.7"
else
    echo "$*" >>"$HOME/stub.log"
fi
EOF
chmod 755 "$stub"
app_args=()
if [ "$target" = universal-apple-darwin ]; then
    mkdir -p "${work}/quickhop.app/Contents/MacOS"
    cp "$stub" "${work}/quickhop.app/Contents/MacOS/quickhop"
    app_args=(--app "${work}/quickhop.app")
fi
"${root}/scripts/dist.sh" --target "$target" --bin "$stub" --out "$dist" "${app_args[@]}" >/dev/null

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
    if [ "$target" = universal-apple-darwin ]; then
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

# 3. Install from a local directory.
if out="$(sandbox bash "${installer[@]}" --from "$dist" 2>&1)" &&
    [[ "$out" == *"checksum ok"* ]] && [ -x "$bin" ] && desktop_files true; then
    pass "--from installs the binary and the desktop files"
else
    fail "--from install: ${out}"
fi

# 4. The installed binary runs.
if [ "$(sandbox "$bin" --version 2>&1)" = "quickhop 9.8.7" ]; then
    pass "installed binary runs"
else
    fail "installed binary does not run"
fi

# 5. Install over HTTP, pinned to the archive's version; a different pin is refused.
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

# 6. Uninstall stops the daemon, turns autostart off and keeps the config and data.
mkdir -p "${home}/.config/quickhop" "${home}/.local/share/quickhop"
touch "${home}/.config/quickhop/config.toml" "${home}/.local/share/quickhop/state.sqlite"
out="$(sandbox bash "${installer[@]}" --uninstall 2>&1)"
log="$(cat "${home}/stub.log" 2>/dev/null || true)"
if [ ! -e "$bin" ] && desktop_files false && [[ "$log" == *"daemon stop"* ]] &&
    [[ "$log" == *"autostart off"* ]] && [ -f "${home}/.config/quickhop/config.toml" ] &&
    [ -f "${home}/.local/share/quickhop/state.sqlite" ]; then
    pass "--uninstall removes quickhop, keeps the config and data"
else
    fail "--uninstall: ${out} (stub log: ${log})"
fi

# 7. Purge names each path, then deletes only those.
out="$(sandbox bash "${installer[@]}" --uninstall --purge 2>&1)"
if [[ "$out" == *"--purge deletes: ${home}/.config/quickhop"* ]] &&
    [[ "$out" == *"--purge deletes: ${home}/.local/share/quickhop"* ]] &&
    [ ! -e "${home}/.config/quickhop" ] && [ ! -e "${home}/.local/share/quickhop" ] &&
    [ -d "${home}/.local/share" ]; then
    pass "--purge deletes the config and data directories only"
else
    fail "--purge: ${out}"
fi

# 8. A wrong checksum aborts without installing.
bad="${work}/bad"
mkdir -p "$bad"
cp "${dist}/quickhop-${target}.tar.gz" "$bad/"
echo "0000  quickhop-${target}.tar.gz" >"${bad}/quickhop-${target}.tar.gz.sha256"
if out="$(sandbox bash "${installer[@]}" --from "$bad" 2>&1)"; then
    fail "a wrong checksum was accepted"
elif [[ "$out" == *"checksum mismatch"* ]] && [ ! -e "$bin" ]; then
    pass "a wrong checksum aborts before installing"
else
    fail "wrong checksum: ${out}"
fi

# 9. Bad options are refused with a reason.
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

# 10. The caller's environment was never used.
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
