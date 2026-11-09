#!/bin/sh
# @raycast.schemaVersion 1
# @raycast.title Search Docs
# @raycast.mode compact
# @raycast.argument1 { "type": "text", "placeholder": "Query" }
# @raycast.argument2 { "type": "dropdown", "placeholder": "Site", "data": [{"title": "Rust", "value": "rust"}, {"title": "Python", "value": "python"}] }
echo "searching $1 on $2"
