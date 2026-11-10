#!/usr/bin/env bash
set -euo pipefail
# @raycast.schemaVersion 1
# @raycast.title Dev Hello
# @raycast.mode compact
# @raycast.argument1 { "type": "text", "placeholder": "Name", "optional": true }
echo "hello from the dev config${1:+, $1}"
