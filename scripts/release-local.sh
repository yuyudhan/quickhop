#!/usr/bin/env bash
# Builds, checks and publishes a quickhop release from a Mac, with the same assets the GitHub
# release workflow (.github/workflows/release.yml, disabled in the repository's Actions settings
# since 2026-11-14: the yuyudhan account does not pay for Actions minutes) produced:
#   quickhop-universal-apple-darwin.tar.gz   quickhop.app and the binary, arm64 + x86_64 (lipo)
#   quickhop-{x86_64,aarch64}-unknown-linux-gnu.tar.gz
#   quickhop_X.Y.Z_{amd64,arm64}.deb         both built in debian:bullseye (glibc 2.31 floor)
#   install.sh, quickhop.rb, quickhop-cask.rb, a .sha256 beside every asset and SHA256SUMS.
#
# Steps: export the release commit (tag vX.Y.Z, or HEAD for a dry run) into
# target/release-local/src; run `just verify` and `just msrv` there; build, smoke-test, bundle,
# sign (scripts/sign-macos.sh) and package the universal macOS archive and test install.sh on it;
# build both Linux targets in debian:bullseye containers (docker; x86_64 runs emulated on Apple
# silicon) and install each .deb and tarball in a clean debian:bullseye; then
# scripts/release-finish.sh. Unless --dry-run: publish the GitHub release (notes from
# scripts/release-notes.sh), push the formula and cask to yuyudhan/homebrew-tap when
# HOMEBREW_TAP_TOKEN is set, and start yuyudhan.github.io's installer sync. The assets stay in
# target/release-local/dist.
#
# Usage: scripts/release-local.sh [--dry-run]
#   The tag vX.Y.Z (X.Y.Z = [workspace.package] version) must exist here and on origin, at the
#   same commit. --dry-run builds and checks HEAD and publishes nothing.
#
# Environment (all optional, as the workflow's repository secrets were):
#   MACOS_CERTIFICATE, MACOS_CERTIFICATE_PASSWORD, MACOS_SIGN_IDENTITY, NOTARY_KEY,
#   NOTARY_KEY_ID, NOTARY_ISSUER   Developer ID signing and notarization (scripts/sign-macos.sh)
#   HOMEBREW_TAP_TOKEN             a token that can push to yuyudhan/homebrew-tap
#
# Inside the containers this script runs itself as
#   scripts/release-local.sh container-build TARGET OUT
#   scripts/release-local.sh container-check DEB_ARCH VERSION DIST
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
repo="yuyudhan/quickhop"
pages_repo="yuyudhan/yuyudhan.github.io"
image="debian:bullseye"

fail() {
    printf 'release-local: %s\n' "$*" >&2
    exit 1
}

say() {
    printf 'release-local: %s\n' "$*" >&2
}

# The [workspace.package] version of the Cargo.toml in directory $1.
workspace_version() {
    local version
    version="$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' "$1/Cargo.toml")"
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] ||
        fail "no X.Y.Z version in $1/Cargo.toml [workspace.package]: '${version}'"
    printf '%s\n' "$version"
}

# In debian:bullseye, /src the exported release tree: build TARGET (this container's
# architecture), smoke-test it and put its archive, .sha256 and .deb into OUT.
container_build() {
    local target="$1" out="$2" version binary stage deb
    version="$(workspace_version "$root")"
    "${root}/scripts/release-linux-deps.sh"
    binary="$("${root}/scripts/release-build.sh" "$target")"
    "$binary" --version
    [ "$("$binary" --version)" = "quickhop ${version}" ] || fail "${binary} --version is not quickhop ${version}"
    "$binary" query "2 + 2" | grep -q "^4 " || fail "${binary} query '2 + 2' did not answer 4"
    stage="$(mktemp -d)"
    "${root}/scripts/dist.sh" --target "$target" --bin "$binary" --out "${stage}/dist"
    deb="$("${root}/scripts/bundle-linux.sh" --bin "$binary" --out "${stage}/bundle" | grep '\.deb$')"
    cp "$deb" "${stage}/dist/"
    rm "${stage}/dist/install.sh"
    cp "${stage}/dist/"* "$out/"
    rm -rf "$stage"
}

# In a clean debian:bullseye, the oldest system the release promises: the .deb installs with
# apt (resolving its Depends) and runs, and install.sh installs the tarball.
container_check() {
    local deb_arch="$1" version="$2" dist="$3" home copy
    printf 'deb http://archive.debian.org/debian bullseye main\ndeb http://archive.debian.org/debian-security bullseye-security main\n' >/etc/apt/sources.list
    rm -f /etc/apt/sources.list.d/debian.sources
    apt-get update -q
    apt-get install -y -q "${dist}/quickhop_${version}_${deb_arch}.deb" curl python3 procps
    [ "$(quickhop --version)" = "quickhop ${version}" ] || fail "the installed .deb is not quickhop ${version}"
    quickhop doctor
    apt-get remove -y -q quickhop
    copy="$(mktemp -d)"
    cp "${dist}/"* "$copy/"
    cp "${root}/scripts/install.sh" "${copy}/install.sh"
    home="$(mktemp -d)"
    env -i HOME="$home" PATH=/usr/bin:/bin sh "${copy}/install.sh" --from "$copy" --version "$version"
    [ "$(env -i HOME="$home" "${home}/.local/bin/quickhop" --version)" = "quickhop ${version}" ] ||
        fail "install.sh did not install quickhop ${version}"
}

# docker run for one Linux architecture, with its toolchain, cargo cache and target directory
# kept in named volumes between releases.
linux_run() {
    local platform="$1" deb_arch="$2"
    shift 2
    docker run --rm --platform "$platform" \
        -v "${src}:/src:ro" -v "${dist}:/dist" \
        -v "quickhop-release-${deb_arch}-cargo:/root/.cargo" \
        -v "quickhop-release-${deb_arch}-rustup:/root/.rustup" \
        -v "quickhop-release-${deb_arch}-target:/target" \
        -e CARGO_TARGET_DIR=/target \
        -e PATH=/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        "$image" "$@"
}

release() {
    local dry_run="$1" version commit tag remote work runner notarized binary arm intel app home
    local notes tap target deb_arch platform tool signed
    [ "$(uname -s)" = Darwin ] || fail "runs on macOS (the universal macOS archive builds there)"
    for tool in docker gh git just lipo tar; do
        command -v "$tool" >/dev/null 2>&1 || fail "${tool} is not on PATH"
    done
    docker info >/dev/null 2>&1 || fail "docker is not running"

    version="$(workspace_version "$root")"
    tag="v${version}"
    if [ "$dry_run" = true ]; then
        commit="$(git -C "$root" rev-parse HEAD)"
        say "dry run of ${version} at HEAD ${commit:0:12}: nothing is published"
    else
        commit="$(git -C "$root" rev-parse --verify --quiet "refs/tags/${tag}^{commit}")" ||
            fail "no tag ${tag}: tag the release commit and push the tag first (CONTRIBUTING.md, Releases)"
        remote="$(git -C "$root" ls-remote origin "refs/tags/${tag}^{}" "refs/tags/${tag}" |
            awk -v peeled="refs/tags/${tag}^{}" -v plain="refs/tags/${tag}" \
                '$2 == peeled { p = $1 } $2 == plain { t = $1 } END { print (p != "" ? p : t) }')"
        [ "$remote" = "$commit" ] || fail "origin's ${tag} is '${remote}', not ${commit}: push the tag"
        say "releasing ${tag} (${commit:0:12})"
    fi

    work="${root}/target/release-local"
    src="${work}/src"
    dist="${work}/dist"
    rm -rf "$src" "$dist" "${work}/bundle" "${work}/runner"
    mkdir -p "$src" "$dist" "${work}/bundle" "${work}/runner"
    git -C "$root" archive "$commit" | tar -x -C "$src"
    [ "$(workspace_version "$src")" = "$version" ] || fail "${commit:0:12}'s version is not ${version}"
    if [ "$dry_run" = false ]; then
        "${src}/scripts/release-notes.sh" "$version" >/dev/null
    fi

    say "gate: just verify and just msrv on ${commit:0:12}"
    (cd "$src" && CARGO_TARGET_DIR="${root}/target" just verify && CARGO_TARGET_DIR="${root}/target" just msrv)

    say "macOS universal"
    export CARGO_TARGET_DIR="${root}/target"
    arm="$("${src}/scripts/release-build.sh" aarch64-apple-darwin)"
    intel="$("${src}/scripts/release-build.sh" x86_64-apple-darwin)"
    mkdir -p "${work}/universal-apple-darwin"
    binary="${work}/universal-apple-darwin/quickhop"
    lipo -create -output "$binary" "$arm" "$intel"
    lipo -info "$binary"
    [ "$(lipo -archs "$binary")" = "x86_64 arm64" ] || fail "${binary} is not x86_64 + arm64"
    [ "$(arch -arm64 "$binary" --version)" = "quickhop ${version}" ] || fail "the arm64 slice is not quickhop ${version}"
    if arch -x86_64 /usr/bin/true 2>/dev/null; then
        [ "$(arch -x86_64 "$binary" --version)" = "quickhop ${version}" ] ||
            fail "the x86_64 slice is not quickhop ${version}"
    else
        say "no Rosetta here: the x86_64 slice was built and joined but not run"
    fi
    app="$("${src}/scripts/bundle-macos.sh" --bin "$binary" --out "${work}/bundle")"
    runner="${work}/runner"
    : >"${runner}/output"
    signed=true
    RUNNER_TEMP="$runner" GITHUB_OUTPUT="${runner}/output" "${src}/scripts/sign-macos.sh" "$app" "$binary" ||
        signed=false
    # sign-macos.sh's temporary keychain (only with MACOS_CERTIFICATE) leaves the search list.
    if [ -f "${runner}/quickhop-signing.keychain-db" ]; then
        security delete-keychain "${runner}/quickhop-signing.keychain-db"
    fi
    [ "$signed" = true ] || fail "scripts/sign-macos.sh failed"
    notarized="$(sed -n 's/^notarized=//p' "${runner}/output")"
    "${src}/scripts/dist.sh" --target universal-apple-darwin --bin "$binary" --app "$app" --out "$dist"
    rm "${dist}/install.sh"
    home="$(mktemp -d)"
    env -i HOME="$home" PATH=/usr/bin:/bin QUICKHOP_APP_DIR="${home}/Applications" \
        sh "${src}/scripts/install.sh" --from "$dist" --version "$version"
    [ "$(readlink "${home}/.local/bin/quickhop")" = "${home}/Applications/quickhop.app/Contents/MacOS/quickhop" ] ||
        fail "install.sh did not link quickhop into quickhop.app"
    codesign -dv "${home}/.local/bin/quickhop" 2>&1 | grep -x 'Identifier=io.github.yuyudhan.quickhop' ||
        fail "the installed quickhop is not signed as io.github.yuyudhan.quickhop"
    rm -rf "$home"
    unset CARGO_TARGET_DIR

    for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
        case "$target" in
            x86_64-*) platform=linux/amd64 deb_arch=amd64 ;;
            aarch64-*) platform=linux/arm64 deb_arch=arm64 ;;
        esac
        say "Linux ${target} in ${image}"
        linux_run "$platform" "$deb_arch" /src/scripts/release-local.sh container-build "$target" /dist
        say "check on Debian 11 (${target})"
        docker run --rm --platform "$platform" -v "${src}:/src:ro" -v "${dist}:/dist:ro" \
            "$image" /src/scripts/release-local.sh container-check "$deb_arch" "$version" /dist
    done

    QUICKHOP_NOTARIZED="${notarized:-false}" "${src}/scripts/release-finish.sh" "$version" "$dist"
    ls -l "$dist"
    cat "${dist}/SHA256SUMS"

    if [ "$dry_run" = true ]; then
        say "dry run done: the assets are in ${dist}"
        return
    fi

    notes="${runner}/notes.md"
    "${src}/scripts/release-notes.sh" "$version" >"$notes"
    gh release create "$tag" "${dist}/"* --repo "$repo" \
        --title "quickhop ${tag}" --notes-file "$notes" --generate-notes --verify-tag --latest

    if [ -z "${HOMEBREW_TAP_TOKEN:-}" ]; then
        say "no HOMEBREW_TAP_TOKEN: the formula and cask are attached to the release but not published to the tap"
    else
        tap="$(mktemp -d)"
        git clone --depth 1 "https://x-access-token:${HOMEBREW_TAP_TOKEN}@github.com/yuyudhan/homebrew-tap.git" "$tap"
        mkdir -p "${tap}/Formula" "${tap}/Casks"
        cp "${dist}/quickhop.rb" "${tap}/Formula/quickhop.rb"
        cp "${dist}/quickhop-cask.rb" "${tap}/Casks/quickhop.rb"
        git -C "$tap" add Formula/quickhop.rb Casks/quickhop.rb
        if git -C "$tap" diff --cached --quiet; then
            say "the tap already has quickhop ${version}"
        else
            git -C "$tap" commit -m "quickhop ${version}"
            git -C "$tap" push origin HEAD
        fi
        rm -rf "$tap"
    fi

    gh workflow run sync-installers.yml --repo "$pages_repo"
    say "released ${tag}: https://github.com/${repo}/releases/tag/${tag}; installer sync started on ${pages_repo}"
}

case "${1:-}" in
    container-build)
        [ "$#" -eq 3 ] || fail "usage: $0 container-build TARGET OUT"
        container_build "$2" "$3"
        ;;
    container-check)
        [ "$#" -eq 4 ] || fail "usage: $0 container-check DEB_ARCH VERSION DIST"
        container_check "$2" "$3" "$4"
        ;;
    "")
        release false
        ;;
    --dry-run)
        [ "$#" -eq 1 ] || fail "usage: $0 [--dry-run]"
        release true
        ;;
    *)
        fail "usage: $0 [--dry-run]"
        ;;
esac
