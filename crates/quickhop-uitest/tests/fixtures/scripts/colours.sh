#!/bin/sh
# @raycast.schemaVersion 1
# @raycast.title Colour Output
# @raycast.mode fullOutput
# Four lines of red text: enough red pixels to find in a capture of the window.
for line in 1 2 3 4; do
    printf '\033[31m%s\033[0m\n' "RED TEXT RED TEXT RED TEXT RED TEXT $line"
done
echo "plain"
