#!/usr/bin/env bash
# Builds the Linux packages into target/bundle/: a .deb (dpkg-deb) and a tarball, each with
# the release binary, the desktop entry, the icon and a systemd user unit. Needs dpkg-deb and
# dpkg-shlibdeps (Debian/Ubuntu: dpkg-dev).
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(cargo metadata --no-deps --format-version 1 |
    jq -r '.packages[] | select(.name == "quickhop") | .version')
arch=$(dpkg --print-architecture)
out=target/bundle
stage=$out/deb/quickhop_${version}_${arch}

cargo build --release --locked -p quickhop

rm -rf "$stage"
install -Dm755 target/release/quickhop "$stage/usr/bin/quickhop"
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
shlibs=$out/deb/shlibs
rm -rf "$shlibs"
mkdir -p "$shlibs/debian"
touch "$shlibs/debian/control"
depends=$(cd "$shlibs" && dpkg-shlibdeps -O -e "$OLDPWD/$stage/usr/bin/quickhop" 2>/dev/null |
    sed -n 's/^shlibs:Depends=//p')

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
Description: keyboard launcher for Linux, macOS and Windows
 Apps, files, calculations, clipboard history, snippets and Raycast-compatible
 script commands, one hotkey away. The daemon keeps the clipboard history and
 the file index; enable it with "quickhop autostart on" or
 "systemctl --user enable --now quickhop".
EOF

dpkg-deb --root-owner-group --build "$stage" "$out/quickhop_${version}_${arch}.deb" >/dev/null
tar -C "$stage/usr" --owner=0 --group=0 --transform "s,^\.,quickhop-$version," \
    -czf "$out/quickhop-$version-$(uname -m)-linux.tar.gz" .
echo "$out/quickhop_${version}_${arch}.deb"
echo "$out/quickhop-$version-$(uname -m)-linux.tar.gz"
