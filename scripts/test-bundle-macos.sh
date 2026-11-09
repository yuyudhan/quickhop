#!/usr/bin/env bash
# Test of scripts/bundle-macos.sh: bundles a stub quickhop binary and checks what macOS reads
# from the quickhop.app it builds: the Info.plist (identifier, executable, the binary's
# version, an agent app without a Dock icon, the icon), every size of the icon, the ad-hoc
# signature, and that the bundle runs the binary. Skipped on other systems. A stub keeps it
# fast; the real binary in the bundle is tested by the UI tests
# (crates/quickhop-uitest/tests/app.rs).
#
# Usage: scripts/test-bundle-macos.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
if [ "$(uname -s)" != Darwin ]; then
    echo "test-bundle-macos: skipped, quickhop.app is built on macOS only"
    exit 0
fi

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

# A stand-in binary that prints a version, and its arguments otherwise.
stub="${work}/quickhop"
cat >"$stub" <<'EOF'
#!/bin/sh
if [ "$1" = "--version" ]; then
    echo "quickhop 9.8.7"
else
    echo "stub ran with: $*"
fi
EOF
chmod 755 "$stub"

out="${work}/out"
mkdir -p "${out}/quickhop.app/Contents"
touch "${out}/quickhop.app/Contents/stale"
app="$("${root}/scripts/bundle-macos.sh" --bin "$stub" --out "$out")"
if [ "$app" = "${out}/quickhop.app" ] && [ -d "$app" ]; then
    pass "builds ${out}/quickhop.app and prints its path"
else
    fail "printed '${app}', expected ${out}/quickhop.app"
fi
if [ ! -e "${app}/Contents/stale" ]; then
    pass "replaces the bundle that was there"
else
    fail "a file of the earlier bundle is left"
fi

plist="${app}/Contents/Info.plist"
if plutil -lint -s "$plist"; then
    pass "Info.plist is a valid property list"
else
    fail "plutil -lint rejects Info.plist"
fi
for expected in \
    CFBundleIdentifier=io.github.yuyudhan.quickhop \
    CFBundleExecutable=quickhop \
    CFBundlePackageType=APPL \
    CFBundleShortVersionString=9.8.7 \
    CFBundleVersion=9.8.7 \
    LSUIElement=true \
    CFBundleIconFile=AppIcon \
    LSMinimumSystemVersion=11.0 \
    NSHighResolutionCapable=true; do
    key="${expected%%=*}" value="${expected#*=}"
    actual="$(plutil -extract "$key" raw -o - "$plist" 2>&1 || true)"
    if [ "$actual" = "$value" ]; then
        pass "Info.plist ${key} = ${value}"
    else
        fail "Info.plist ${key} is '${actual}', expected '${value}'"
    fi
done

ran="$("${app}/Contents/MacOS/quickhop" daemon 2>&1 || true)"
if [ "$ran" = "stub ran with: daemon" ]; then
    pass "the bundle's executable is the binary"
else
    fail "Contents/MacOS/quickhop printed '${ran}', not the stub's line"
fi

icns="${app}/Contents/Resources/AppIcon.icns"
if iconutil -c iconset -o "${work}/back.iconset" "$icns" 2>/dev/null; then
    missing=""
    for points in 16 32 128 256 512; do
        for suffix in "" @2x; do
            pixels="$points"
            [ -z "$suffix" ] || pixels=$((points * 2))
            name="icon_${points}x${points}${suffix}.png"
            width="$(sips -g pixelWidth "${work}/back.iconset/${name}" 2>/dev/null |
                awk '/pixelWidth/ {print $2}' || true)"
            [ "$width" = "$pixels" ] || missing="${missing} ${name}"
        done
    done
    if [ -z "$missing" ]; then
        pass "AppIcon.icns holds every size from 16 to 1024 px"
    else
        fail "AppIcon.icns lacks or misdraws:${missing}"
    fi
else
    fail "iconutil cannot read AppIcon.icns"
fi

if codesign --verify --deep --strict "$app" 2>/dev/null; then
    pass "the signature verifies"
else
    fail "codesign --verify --deep --strict fails"
fi
signed="$(codesign -dv "$app" 2>&1 || true)"
if grep -qx "Identifier=io.github.yuyudhan.quickhop" <<<"$signed"; then
    pass "the signature carries the bundle identifier"
else
    fail "codesign -dv says: ${signed}"
fi

if "${root}/scripts/bundle-macos.sh" --bin "${work}/nonexistent" --out "$out" 2>/dev/null; then
    fail "a missing --bin is accepted"
else
    pass "refuses a missing --bin"
fi

if [ "$failures" -gt 0 ]; then
    echo "test-bundle-macos: ${failures} failed"
    exit 1
fi
echo "test-bundle-macos: all passed"
