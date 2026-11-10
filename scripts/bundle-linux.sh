#!/usr/bin/env bash
# Builds the Linux packages: a .deb (dpkg-deb) and a tarball, each with the release binary, the
# desktop entry, the icon and a systemd user unit. Needs dpkg-deb and dpkg-shlibdeps
# (Debian/Ubuntu: dpkg-dev). The package's version is the binary's `--version`, its
# architecture this system's (dpkg --print-architecture): build on the architecture you package.
# Prints the paths of the .deb and the tarball.
#
# Usage: scripts/bundle-linux.sh [--bin PATH] [--out DIR]
#   --bin  the binary to package (default: the release build, built first with cargo)
#   --out  where the packages go (default: target/bundle)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin="" out="${root}/target/bundle"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --bin | --out)
            [ "$#" -ge 2 ] || { echo "bundle-linux: $1 needs a value" >&2; exit 2; }
            case "$1" in
                --bin) bin="$2" ;;
                --out) out="$2" ;;
            esac
            shift 2
            ;;
        *) echo "bundle-linux: unknown option $1" >&2; exit 2 ;;
    esac
done
if [ -z "$bin" ]; then
    (cd "$root" && cargo build --release --locked -p quickhop)
    bin="${root}/target/release/quickhop"
fi
[ -f "$bin" ] || { echo "bundle-linux: --bin ${bin}: no such file" >&2; exit 2; }

# `quickhop --version` prints "quickhop X.Y.Z".
version="$("$bin" --version)"
version="${version#quickhop }"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
    echo "bundle-linux: ${bin} --version printed no X.Y.Z version: ${version}" >&2
    exit 2
}
arch="$(dpkg --print-architecture)"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
stage="${out}/deb/quickhop_${version}_${arch}"

cd "$root"
rm -rf "$stage"
install -Dm755 "$bin" "$stage/usr/bin/quickhop"
install -Dm644 packaging/linux/quickhop.desktop "$stage/usr/share/applications/quickhop.desktop"
install -Dm644 packaging/linux/quickhop.svg "$stage/usr/share/icons/hicolor/scalable/apps/quickhop.svg"
install -Dm644 packaging/linux/quickhop.service "$stage/usr/lib/systemd/user/quickhop.service"
install -Dm644 LICENSE-MIT "$stage/usr/share/doc/quickhop/LICENSE-MIT"
install -Dm644 LICENSE-APACHE "$stage/usr/share/doc/quickhop/LICENSE-APACHE"
{
    printf 'quickhop is licensed under the MIT license or the Apache License 2.0, at your option.\n\n'
    cat LICENSE-MIT
    printf '\n'
    cat LICENSE-APACHE
} >"$stage/usr/share/doc/quickhop/copyright"
chmod 644 "$stage/usr/share/doc/quickhop/copyright"

# The shared libraries the binary links, as Debian dependencies.
shlibs="${out}/deb/shlibs"
rm -rf "$shlibs"
mkdir -p "$shlibs/debian"
touch "$shlibs/debian/control"
depends="$(cd "$shlibs" && dpkg-shlibdeps -O -e "$stage/usr/bin/quickhop" 2>/dev/null |
    sed -n 's/^shlibs:Depends=//p')"
[ -n "$depends" ] || {
    echo "bundle-linux: dpkg-shlibdeps found no library dependencies for ${bin}" >&2
    exit 1
}

mkdir -p "$stage/DEBIAN"
cat >"$stage/DEBIAN/control" <<EOF
Package: quickhop
Version: $version
Architecture: $arch
Maintainer: quickhop maintainers <https://github.com/yuyudhan/quickhop/issues>
Installed-Size: $(du -sk "$stage/usr" | cut -f1)
Depends: $depends
Recommends: xdg-utils, wl-clipboard | xclip
Section: utils
Priority: optional
Homepage: https://github.com/yuyudhan/quickhop
Description: keyboard launcher for macOS and Linux
 Apps, files, calculations, clipboard history, snippets and Raycast-compatible
 script commands, one hotkey away. The daemon keeps the clipboard history and
 the file index; enable it with "quickhop autostart on" or
 "systemctl --user enable --now quickhop".
EOF

deb="${out}/quickhop_${version}_${arch}.deb"
tarball="${out}/quickhop-${version}-$(uname -m)-linux.tar.gz"
dpkg-deb --root-owner-group --build "$stage" "$deb" >/dev/null
tar -C "$stage/usr" --owner=0 --group=0 --transform "s,^\.,quickhop-$version," \
    -czf "$tarball" .
echo "$deb"
echo "$tarball"
