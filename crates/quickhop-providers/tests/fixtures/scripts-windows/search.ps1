# @raycast.schemaVersion 1
# @raycast.title Search Docs
# @raycast.mode compact
# @raycast.argument1 { "type": "text", "placeholder": "Query", "percentEncoded": true }
# @raycast.argument2 { "type": "dropdown", "placeholder": "Site", "optional": true, "data": [{"title": "Rust", "value": "rust"}, {"title": "Python", "value": "python"}] }
param([string]$Query, [string]$Site)
if (-not $Site) { $Site = "all" }
Write-Output "searching $Query on $Site"
