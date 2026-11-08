#!/bin/sh
# @raycast.schemaVersion 1
# @raycast.title Hello CLI
# @raycast.mode compact
# @raycast.argument1 { "type": "text", "placeholder": "Name", "optional": true }
echo "hello ${1:-world}"
