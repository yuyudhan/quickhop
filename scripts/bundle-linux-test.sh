#!/usr/bin/env bash
# Installs the .deb from `just bundle-linux` in a clean Debian container and checks that it
# runs: `quickhop doctor`, a query, and the packaged files in place. Needs docker.
set -euo pipefail
cd "$(dirname "$0")/.."
image=${QUICKHOP_TEST_IMAGE:-debian:bookworm-slim}
deb=$(ls target/bundle/quickhop_*.deb)
docker run --rm -v "$PWD/$deb:/tmp/quickhop.deb:ro" "$image" sh -euc '
    dpkg -i /tmp/quickhop.deb
    quickhop doctor
    quickhop query "2 + 2" | grep -q "^4 "
    for f in /usr/share/applications/quickhop.desktop \
        /usr/share/icons/hicolor/scalable/apps/quickhop.svg \
        /usr/lib/systemd/user/quickhop.service; do
        test -f "$f"
    done
    echo "quickhop package ok on $(. /etc/os-release && echo "$PRETTY_NAME")"
'
