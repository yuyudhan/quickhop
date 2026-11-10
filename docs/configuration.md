# Configuration

quickhop reads two plain TOML files, both optional. Every key has a default, unknown keys are
errors (so a typo never goes unnoticed), and `quickhop config check` validates a file before you
rely on it.

    ~/.config/quickhop/
    ├── config.toml          # settings, providers, quicklinks, per-command settings
    ├── snippets.toml        # snippets with Raycast placeholders
    └── scripts/             # Raycast-compatible script commands

Start from the defaults:

    mkdir -p ~/.config/quickhop
    quickhop config default > ~/.config/quickhop/config.toml
    quickhop config check     # "ok", or the line and key that is wrong

`quickhop config check` checks `config.toml` and the `snippets.toml` next to it.

The daemon reads the files when it starts; `quickhop reload` makes a running daemon read them
again (it does not watch them). Every other command reads them each time it runs. What each
key changes on a reload: [when changes apply](#when-changes-apply).

## Where files live

The same layout on macOS and Linux:

- **Config**: `$XDG_CONFIG_HOME/quickhop/config.toml`, by default
  `~/.config/quickhop/config.toml`. `QUICKHOP_CONFIG=/path/to/file.toml` uses another file;
  `snippets.toml` and `scripts/` are then looked up next to it. `quickhop config path` prints
  the file in use.
- **Data**: `$XDG_DATA_HOME/quickhop/`, by default `~/.local/share/quickhop/`, or
  `QUICKHOP_DATA`. It holds `frecency.sqlite` (what you pick for which query),
  `state.sqlite` (clipboard history) and `index.sqlite` (the file index).
- **Runtime**: the daemon's socket and lock file live in `$XDG_RUNTIME_DIR/quickhop`, else in
  `quickhop-<user>` under `$TMPDIR` or `/tmp`; `QUICKHOP_RUNTIME` overrides it.

quickhop 0.1 on macOS kept everything in `~/Library/Application Support/quickhop`; the first run
of a newer version moves those files to the locations above and says so.

**Development builds** (`cargo run`, any debug build, `just bundle-macos`) use
`~/.config/quickhop-dev/` and `~/.local/share/quickhop-dev/` instead
(`$XDG_CONFIG_HOME/quickhop-dev`, `$XDG_DATA_HOME/quickhop-dev`), their own daemon socket, and
default the `hotkey` to `Alt+Shift+Space`, so they can run beside an installed release
([install.md](install.md#development-builds)). `quickhop doctor` says which flavour runs
(`flavour: dev …` or `flavour: release …`) and its config folder. In a clone, `just dev-config`
installs a config there with every option set and commented (from `config/dev/`); it keeps
files already there unless `--force`, which backs each one up first.

## config.toml

Keys are written as TOML paths: `providers.apps` is `apps` under `[providers]`. Array tables
such as `[[quicklinks]]` repeat once per entry; their keys are written `quicklinks[].name`.

### Top level

| Key | Default | Meaning |
|-----|---------|---------|
| `hotkey` | `"Alt+Space"` | Global shortcut that shows and hides the launcher window: modifiers and one key joined by `+`, in any order and case, e.g. `"Ctrl+Space"`, `"cmd+shift+k"`, `"F13"`. `Cmd`, `Command`, `Super` and `Meta` are one modifier, `Alt`, `Option` and `Opt` another; a key that types a character needs a modifier, function keys may stand alone. A development build defaults to `"Alt+Shift+Space"`. The macOS daemon registers it (`quickhop daemon status` says whether it holds it, or why not, such as another app holding it) and binds the new one on `quickhop reload`; a reload with an invalid key is refused and the old one kept. On Linux the window, and so the hotkey, is still being built. |
| `max_results` | `8` | Results shown per search, 1 to 100. `quickhop query --limit N` overrides it once; the window uses the new value from its next search. |

### `[providers]`

Turn a built-in source of results on or off. A provider switched off loads nothing; with
`snippets` or `files` off, the window's Search Snippets or Search Files command goes too, and
`files = false` also stops the daemon's file index.

| Key | Default | Meaning |
|-----|---------|---------|
| `providers.apps` | `true` | Installed applications: macOS bundles, Linux desktop entries. |
| `providers.calc` | `true` | Calculator and unit conversion (`2+2*3`, `5 km to miles`). |
| `providers.emoji` | `true` | Emoji by name and keyword (`smile`, `thumbs`). |
| `providers.system` | `true` | Lock, log out, sleep, restart and shut down. |
| `providers.quicklinks` | `true` | The `[[quicklinks]]` below. |
| `providers.scripts` | `true` | Raycast-compatible script commands. |
| `providers.snippets` | `true` | Snippets from `snippets.toml`. |
| `providers.files` | `true` | Files and folders: the quickhop index once built, else Spotlight on macOS. |

### `[apps]`

| Key | Default | Meaning |
|-----|---------|---------|
| `apps.dirs` | `[]` | Folders to scan for applications instead of the OS defaults (`/Applications`, `~/Applications` and the system folders on macOS; every XDG `applications` folder on Linux). |
| `apps.icon_theme` | `""` | Linux icon theme looked up before `hicolor`, e.g. `"Papirus"`. Empty uses the GTK setting (`gtk-icon-theme-name` in `~/.config/gtk-3.0/settings.ini`), else `hicolor` only. |

### `[scripts]`

| Key | Default | Meaning |
|-----|---------|---------|
| `scripts.dirs` | `[]` | More folders of script commands, scanned after `~/.config/quickhop/scripts`, in the order given. Not recursive; a leading `~` is your home folder. |

### `[files]`

What `quickhop index` and the daemon's live index cover. On macOS, Spotlight answers until the
index exists.

| Key | Default | Meaning |
|-----|---------|---------|
| `files.roots` | `[]` | Folders to index; empty means your home folder. A leading `~` is your home folder; a folder that does not exist indexes nothing, without a warning. |
| `files.hidden` | `false` | Index hidden files and folders (names starting with `.`). |
| `files.exclude` | `[]` | More file or folder names to skip. Version control, dependency, build and cache folders (`.git`, `node_modules`, `target`, …) are always skipped. |

### `[clipboard]`

| Key | Default | Meaning |
|-----|---------|---------|
| `clipboard.enabled` | `true` | Record copied text into the history. `false` stops the daemon's watcher, makes `quickhop clipboard watch` refuse and hides the Clipboard History command; `quickhop clipboard add` by hand still records. |
| `clipboard.max_entries` | `1000` | Most entries kept, 1 to 100000; the oldest go first. |
| `clipboard.max_age_days` | `90` | Entries not copied again for this many days are deleted; `0` keeps them forever. |

### `[[quicklinks]]`

A URL opened with what you type after its alias. Every `{query}` in `url` is replaced with the
percent-encoded text.

| Key | Default | Meaning |
|-----|---------|---------|
| `quicklinks[].name` | `required` | Display name, e.g. `"GitHub search"`. |
| `quicklinks[].url` | `required` | URL or path; every `{query}` marks where the percent-encoded text goes (`a b&c` becomes `a%20b%26c`). |
| `quicklinks[].alias` | `unset` | One word that pins this link: `gh tokio` searches GitHub for `tokio`. |

```toml
[[quicklinks]]
name = "GitHub search"
url = "https://github.com/search?q={query}"
alias = "gh"

[[quicklinks]]
name = "Crates"
url = "https://crates.io/search?q={query}"
alias = "cr"
```

### `[[commands]]`

Settings for any single result, named by the id `quickhop query --json` prints
(`provider:key`, e.g. `apps:org.mozilla.firefox.desktop`, `system:lock`, `scripts:hello.sh`).
An id that matches nothing is a config error, so stale entries are caught.

| Key | Default | Meaning |
|-----|---------|---------|
| `commands[].id` | `required` | The result, `provider:key`. Each id may appear once. |
| `commands[].alias` | `unset` | One word that pins the result when typed alone or as the first word. |
| `commands[].enabled` | `true` | `false` removes the result from every search and from `quickhop open`. |
| `commands[].hotkey` | `unset` | Global shortcut that runs the result directly, e.g. `"Ctrl+Alt+T"`, written as `hotkey` above. Checked today, including that no two commands, nor a command and the launcher, share one; registered once per-command hotkeys exist. Until then, bind the key in your desktop to `quickhop open ID`. |
| `commands[].order` | `unset` | Position in the empty search: lower first, ahead of results without one. Works for any result, including emoji and files, which the empty search otherwise leaves out. |

```toml
[[commands]]
id = "system:lock"
alias = "lk"
order = 1

[[commands]]
id = "system:shutdown"
enabled = false
```

## snippets.toml

Each `[[snippets]]` entry becomes a search result whose action copies, prints or pastes the
expanded text. The id is `snippets:` plus the lower-case name (`snippets:reply`).

| Key | Default | Meaning |
|-----|---------|---------|
| `snippets[].name` | `required` | Display name, unique ignoring case; `{snippet name="..."}` includes it in another snippet. |
| `snippets[].text` | `required` | The text, with placeholders. |
| `snippets[].keyword` | `unset` | A word that finds the snippet in search, e.g. `";re"`. |

```toml
[[snippets]]
name = "Reply"
keyword = ";re"
text = """
Hi {argument name="name"},

{cursor}

Sent {date format="EEEE, MMM d"} · {clipboard | trim}"""
```

### Placeholders

The same dynamic placeholders as Raycast snippets:

| Placeholder | Expands to |
|-------------|------------|
| `{clipboard}` | The newest clipboard entry; `{clipboard offset=1}` the one before. |
| `{date}`, `{time}`, `{datetime}`, `{day}` | Now, formatted; `format="yyyy-MM-dd"` takes Unicode date patterns, `offset="+3d"` or `"-2h"` shifts the moment. |
| `{argument name="name"}` | A value you fill in (`--arg Ada` on the command line); `default="..."` makes it optional and `options="a, b"` offers a choice. The same name used twice is asked for once; up to 3 distinct arguments per snippet. |
| `{snippet name="Signature"}` | Another snippet, expanded. |
| `{uuid}` | A fresh random UUID. |
| `{cursor}` | Where the cursor lands after pasting; removed from printed text. |
| `{selection}` | The selected text in the focused app, where the caller can read it; otherwise the placeholder stays as written. |

Modifiers chain after a pipe: `{clipboard | trim | uppercase}`. Available: `trim`,
`uppercase`, `lowercase`, `percent-encode`, `json-stringify` and `raw`. Anything that is not a
valid placeholder (an unknown keyword, attribute or modifier) stays in the text as written.

    quickhop launch snippets:reply --arg Ada            # prints the expanded text
    quickhop launch snippets:reply --arg Ada --paste    # pastes it into the focused app

## A complete example

```toml
# ~/.config/quickhop/config.toml
hotkey = "Alt+Space"
max_results = 8

[providers]
emoji = false

[scripts]
dirs = ["~/code/raycast-scripts"]

[files]
roots = ["~/code", "~/Documents"]
exclude = ["vendor"]

[clipboard]
max_entries = 2000
max_age_days = 30

[[quicklinks]]
name = "GitHub search"
url = "https://github.com/search?q={query}"
alias = "gh"

[[commands]]
id = "system:lock"
alias = "lk"
order = 1
```

A config with every option set, each with a comment giving its default and effect, is in the
repository at `config/dev/config.toml` (and `config/dev/snippets.toml`); `just dev-config`
installs it for a development build.

## When changes apply

Commands other than the daemon read the files every time they run, so a change applies to the
next `quickhop query`, `launch` or `index`. The daemon reads them at start and on
`quickhop reload`:

| Key | After `quickhop reload` |
|-----|-------------------------|
| `hotkey` | The new one is bound and the old one released; an invalid one is refused and the old one kept. |
| `max_results`, `[providers]`, `[apps]`, `[scripts]`, `[[quicklinks]]`, `[[commands]]`, snippets | The window's next search uses them; a provider switched on or off appears or goes. |
| `[files]`, `providers.files` | The file index restarts over the new roots, hidden setting and exclusions. |
| `clipboard.enabled` | The clipboard watcher stops or starts. |
| `clipboard.max_entries`, `clipboard.max_age_days` | Applied when the next copy is recorded. |

A reload of a file with an error changes nothing: the daemon keeps the settings it had and
`quickhop reload` prints the error.
