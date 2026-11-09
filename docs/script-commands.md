# Script commands

A script command is an executable file with Raycast metadata in its comments. quickhop reads
the same `@raycast.*` headers Raycast does, so the scripts you already have, and the hundreds in
[raycast/script-commands](https://github.com/raycast/script-commands), run unchanged wherever
their interpreter exists.

## Your first script

```bash
#!/usr/bin/env bash

# @raycast.schemaVersion 1
# @raycast.title Hello
# @raycast.mode compact
# @raycast.packageName Greetings
# @raycast.icon 👋
# @raycast.argument1 { "type": "text", "placeholder": "Name", "optional": true }

echo "Hello, ${1:-world}!"
```

    mkdir -p ~/.config/quickhop/scripts
    $EDITOR ~/.config/quickhop/scripts/hello.sh
    chmod +x ~/.config/quickhop/scripts/hello.sh
    quickhop run hello Ada                 # → Hello, Ada!
    quickhop query hello                   # Hello  (Greetings)  [scripts:hello.sh]

`quickhop run` finds a script by path, file name, file stem or title (ignoring case). Values
after the name fill `argument1` to `argument3` in order.

## Where quickhop looks

1. `~/.config/quickhop/scripts/` (next to the config file in use).
2. Every folder in `[scripts] dirs` in `config.toml`, in order:

```toml
[scripts]
dirs = ["~/code/raycast-scripts/commands/developer-utils", "~/bin/quickhop"]
```

Folders are not searched recursively, matching Raycast. A file counts when it sits directly in
one of these folders, carries `@raycast.` headers in comment lines (`#`, `//`, `--`, `;`, `'`,
`::` or `REM`), is at most 512 KiB, and is not hidden. Raycast's `*.template.*` files are
skipped.

## Raycast compatibility

Every metadata field Raycast documents for script commands, and what quickhop does with it:

| Field | Raycast meaning | quickhop |
|-------|-----------------|----------|
| `schemaVersion` | Format version, `1` | ✅ Required, must be `1` |
| `title` | Name in search | ✅ Required |
| `mode` | How output is shown | ✅ Required; all four modes, see below |
| `packageName` | Group shown as subtitle | ✅ Defaults to the folder name, as in Raycast |
| `icon` | Emoji, image path or URL | ✅ Emoji, `https://` URL, or a path relative to the script |
| `iconDark` | Icon for dark appearance | ✅ Resolved like `icon` |
| `description` | Longer text | ✅ |
| `author` | Author name | ✅ |
| `authorURL` | Author link | ✅ |
| `currentDirectoryPath` | Working directory | ✅ Relative to the script; `~` expands at run time |
| `needsConfirmation` | Ask before running | ✅ `quickhop run` refuses without `--yes` |
| `refreshTime` | Inline refresh interval (`10s`, `1m`, `12h`, `1d`) | ✅ Parsed with Raycast's 10 s floor; `inline` without it runs as `compact`, as in Raycast |
| `argument1` … `argument3` | Up to three inputs, as JSON | ✅ All of the argument fields below |

Argument JSON fields:

| Field | Raycast meaning | quickhop |
|-------|-----------------|----------|
| `type` | `text`, `password` or `dropdown` | ✅ All three |
| `placeholder` | Hint in the input | ✅ |
| `optional` | May be left empty | ✅ Required arguments must be given |
| `percentEncoded` | Encode the value before passing it | ✅ |
| `secure` | Older spelling of a password input | ✅ Read as `password` |
| `data` | Dropdown choices, `[{"title": "...", "value": "..."}]` | ✅ The `value` is what the script receives |

Output modes:

| Mode | Raycast shows | quickhop's window (macOS) | `quickhop run` prints |
|------|---------------|---------------------------|-----------------------|
| `fullOutput` | All output in a view, ANSI colours kept | An output page, foreground colours drawn | All of standard output, colours kept |
| `compact` | The last line in a toast | The last line in a toast | The last line, colours stripped |
| `silent` | The last line in a HUD after the window closes | The same | The last line, colours stripped |
| `inline` | The first line as the command's subtitle, refreshed every `refreshTime` | The same | The first line |

A script that exits non-zero reports its last output line, or the last line of standard error
when it printed nothing, and `quickhop run` exits with status 1.

quickhop also reads one header Raycast does not use: `@raycast.platform`, kept as written and
shown for information.

### What differs from Raycast

- **The interpreter must exist.** Scripts run directly; the kernel follows the shebang.
  AppleScript (`osascript`) and Swift scripts run on macOS only. A macOS-only script on Linux
  fails with its interpreter's error.
- **Timeout.** `quickhop run` kills a script after 60 seconds by default
  (`--timeout SECONDS`, up to 3600).
- **Inline refresh.** The daemon re-runs the first 10 `inline` scripts that have a
  `refreshTime` and take no arguments (Raycast's limit); `quickhop run` runs one once.
- **Colours.** The output page draws the 16 standard foreground colours; 256 and 24-bit
  colours and backgrounds show as plain text.

## Running scripts

    quickhop run hello                       # by stem
    quickhop run "Kill Process on Port" 3000 # by title, with an argument
    quickhop run ./tools/backup.sh --yes     # by path, confirming needsConfirmation
    quickhop launch scripts:hello.sh --arg Ada   # by result id, as the launcher does

Arguments starting with a hyphen pass through unchanged (`quickhop run offset -5`).

## Using Raycast's community scripts

    git clone https://github.com/raycast/script-commands ~/code/raycast-scripts

Then list the folders you want in `[scripts] dirs`; each is scanned on its own because
scanning is not recursive. Raycast's own example scripts are part of quickhop's test fixtures
and parse exactly as Raycast documents them.
