#!/usr/bin/env bash
# Installs what the Linux release build needs on a Debian or Ubuntu system (as root, as in the
# release workflow's debian:bullseye container): a C toolchain, the libraries gpui links
# (fontconfig, freetype, wayland, xkbcommon/X11), dpkg-dev for the .deb, and rustup with the
# toolchain rust-toolchain.toml pins. Safe to run again.
#
# Debian 11 (bullseye) is past its end of life, and its packages are leaving deb.debian.org
# (security updates already 404 there), so on bullseye apt is pointed at archive.debian.org,
# which keeps every release for good. The release builds there on purpose: its glibc 2.31 is
# the oldest the binaries run on.
#
# Usage: scripts/release-linux-deps.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
export DEBIAN_FRONTEND=noninteractive
if grep -qx 'VERSION_CODENAME=bullseye' /etc/os-release; then
    rm -f /etc/apt/sources.list.d/debian.sources
    cat >/etc/apt/sources.list <<'EOF'
deb http://archive.debian.org/debian bullseye main
deb http://archive.debian.org/debian bullseye-updates main
deb http://archive.debian.org/debian-security bullseye-security main
EOF
fi
apt-get update -q
apt-get install -y -q --no-install-recommends \
    build-essential binutils ca-certificates curl dpkg-dev file git jq pkg-config xz-utils \
    libfontconfig1-dev libfreetype6-dev libwayland-dev libx11-xcb-dev libxcb1-dev \
    libxkbcommon-dev libxkbcommon-x11-dev

if ! command -v rustup >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh
    sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain none
    rm -f /tmp/rustup-init.sh
fi
export PATH="${CARGO_HOME:-${HOME}/.cargo}/bin:${PATH}"
(cd "$root" && { rustup show active-toolchain >/dev/null 2>&1 || rustup toolchain install; })
rustc --version
