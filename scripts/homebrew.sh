#!/usr/bin/env bash
# Renders the Homebrew formula (Linux) and cask (macOS app) for one release from
# packaging/homebrew/*.rb.in, with the version and the checksums of the archives in DIST:
# DIST/quickhop.rb and DIST/quickhop-cask.rb. `just release` attaches both to the release and,
# with HOMEBREW_TAP_TOKEN set, publishes them to yuyudhan/homebrew-tap as
# Formula/quickhop.rb and Casks/quickhop.rb.
#
# Usage: scripts/homebrew.sh VERSION DIST
#   DIST must hold quickhop-TARGET.tar.gz.sha256 for universal-apple-darwin,
#   x86_64-unknown-linux-gnu and aarch64-unknown-linux-gnu (scripts/dist.sh writes them).
#
# Environment:
#   QUICKHOP_NOTARIZED   "true" when quickhop.app is Developer ID signed and notarized; otherwise
#                        the cask clears the download quarantine after installing, since
#                        Gatekeeper calls an ad-hoc signed app from the internet "damaged".
set -euo pipefail

[ "$#" -eq 2 ] || {
    echo "usage: $0 VERSION DIST" >&2
    exit 2
}
version="$1"
dist="$2"
root="$(cd "$(dirname "$0")/.." && pwd)"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
    echo "homebrew: VERSION must be X.Y.Z, got ${version}" >&2
    exit 2
}

# The checksum dist.sh wrote for target $1.
checksum() {
    local file="${dist}/quickhop-$1.tar.gz.sha256" sum
    [ -f "$file" ] || {
        echo "homebrew: ${file} is missing" >&2
        exit 1
    }
    sum="$(cut -d ' ' -f 1 <"$file")"
    [[ "$sum" =~ ^[0-9a-f]{64}$ ]] || {
        echo "homebrew: ${file} holds no SHA-256 checksum" >&2
        exit 1
    }
    echo "$sum"
}

# The cask's macOS requirement for LSMinimumSystemVersion $1 (e.g. 11.0). Homebrew itself
# supports nothing older than macOS 11, so 11 needs no minimum (brew style calls one redundant).
macos_depends() {
    local name
    case "${1%%.*}" in
        11) echo "depends_on :macos"; return 0 ;;
        12) name=monterey ;;
        13) name=ventura ;;
        14) name=sonoma ;;
        15) name=sequoia ;;
        26) name=tahoe ;;
        *)
            echo "homebrew: no Homebrew name for macOS $1; add it to scripts/homebrew.sh" >&2
            exit 1
            ;;
    esac
    echo "depends_on macos: :${name}"
}

plist="${root}/packaging/macos/Info.plist.in"
bundle_id="$(sed -n '/<key>CFBundleIdentifier<\/key>/{n;s/.*<string>\(.*\)<\/string>.*/\1/p;}' "$plist")"
minimum="$(sed -n '/<key>LSMinimumSystemVersion<\/key>/{n;s/.*<string>\(.*\)<\/string>.*/\1/p;}' "$plist")"
if [ -z "$bundle_id" ] || [ -z "$minimum" ]; then
    echo "homebrew: no CFBundleIdentifier or LSMinimumSystemVersion in ${plist}" >&2
    exit 1
fi
depends="$(macos_depends "$minimum")"

formula="$(<"${root}/packaging/homebrew/quickhop.rb.in")"
formula="${formula//@VERSION@/${version}}"
for target in universal-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
    key="$(printf '%s' "$target" | tr '[:lower:]-' '[:upper:]_')"
    formula="${formula//@SHA256_${key}@/$(checksum "$target")}"
done
printf '%s\n' "$formula" >"${dist}/quickhop.rb"

if [ "${QUICKHOP_NOTARIZED:-}" = true ]; then
    quarantine=""
else
    quarantine='
  # This build is signed ad hoc, not notarized: Gatekeeper would call the downloaded app
  # "damaged". Clearing the quarantine flag is the same as approving it once in System Settings.
  postflight_steps do
    run "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "{{appdir}}/quickhop.app"]
  end
'
fi
cask="$(<"${root}/packaging/homebrew/quickhop-cask.rb.in")"
cask="${cask//@VERSION@/${version}}"
cask="${cask//@SHA256_UNIVERSAL_APPLE_DARWIN@/$(checksum universal-apple-darwin)}"
cask="${cask//@MACOS_DEPENDS@/${depends}}"
cask="${cask//@BUNDLE_ID@/${bundle_id}}"
cask="${cask//@QUARANTINE@/${quarantine}}"
printf '%s\n' "$cask" >"${dist}/quickhop-cask.rb"
echo "${dist}/quickhop.rb"
echo "${dist}/quickhop-cask.rb"
