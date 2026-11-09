<div align="center">

# quickhop

**Press a key. Type a few letters. Be there.**

A fast, open-source, keyboard-first launcher for **macOS and Linux**, written in Rust.
Apps, files, calculations, clipboard, snippets, scripts and more, one hotkey away.
A free alternative to Raycast that runs your Raycast script commands unchanged.

</div>

---

> **Status: early.** The search core, providers, configuration and Raycast-compatible script
> commands work today from the command line and are tested on macOS and Linux. The launcher
> window is being built next ([plan](docs/plan.md)).

## Why quickhop

- **Instant.** Native Rust from hotkey to pixels: a GPU-rendered window ([gpui](docs/adr/0001-ui-toolkit.md)), a resident index, and fuzzy ranking that learns what you pick.
- **Yours.** Plain-text config in `~/.config/quickhop`, no account, no telemetry, works offline.
- **Scriptable.** Drop any executable with Raycast `@raycast.*` comments into `~/.config/quickhop/scripts` and it is a command.
- **Both desktops.** macOS and Linux, X11 and Wayland, from one codebase.

## Raycast parity

| Area | quickhop today | Planned |
|------|:--------------:|---------|
| Fuzzy root search with frecency | ✅ | launcher window, action panel, navigation |
| Applications (macOS bundles, Linux desktop entries, icons) | ✅ | running apps, window switcher |
| Calculator and unit conversion | ✅ | currency, dates and time zones |
| Emoji search | ✅ | direct paste, skin tones, symbols |
| System commands (lock, sleep, restart, shut down) | ✅ | trash, eject, dark mode, volume |
| Quicklinks with `{query}` and aliases | ✅ | `{clipboard}`, `{selection}`, open-with |
| **Script commands** (all metadata, modes, arguments) | ✅ | inline refresh, ANSI output view |
| Global hotkey and launcher window | | 🛠 next |
| File and folder search (own index, `quickhop index`; open, reveal, copy path, trash) | ✅ | Spotlight on macOS, Quick Look, open with |
| Clipboard history (`quickhop clipboard watch`, X11 and wlroots/KDE Wayland; `clipboard paste` on X11) | ✅ | searchable view, paste on Wayland and macOS, GNOME Wayland watching |
| Snippets with dynamic placeholders | ✅ | paste into the frontmost app, auto-expansion |
| Window management | | 🛠 |
| Aliases, enable/disable and root order per command | ✅ | per-command hotkeys, favorites, fallbacks |
| AI chat, Quick AI, AI commands | | 🛠 |
| Extensions and store | | 🛠 |
| Themes, settings window, live reload | | 🛠 |
| Floating notes, calendar, focus, translate | | 🛠 |

✅ done · 🛠 planned. All 95 features, with how each works on macOS, X11 and Wayland:
[docs/features.md](docs/features.md).

## Install

quickhop is built from source until the first release (signed DMG and Homebrew cask on macOS;
`.rpm` and AUR on Linux). On Debian and Ubuntu, `just bundle-linux` builds a `.deb` and a
tarball into `target/bundle/`.

**macOS** (Apple silicon or Intel):

    xcode-select --install                 # C compiler for the bundled SQLite
    curl https://sh.rustup.rs -sSf | sh
    cargo install --git https://github.com/yuyudhan/quickhop quickhop

**Linux** (Debian/Ubuntu shown; any distro with a C compiler works):

    sudo apt install build-essential xdg-utils
    sudo apt install wl-clipboard            # Wayland sessions
    sudo apt install xclip                   # X11 sessions
    curl https://sh.rustup.rs -sSf | sh
    cargo install --git https://github.com/yuyudhan/quickhop quickhop

Then check your setup:

    quickhop doctor
    quickhop autostart on                  # start the daemon at login

## Use

    quickhop query firefox                    # ranked results
    quickhop query '5 km to miles' --json     # machine-readable
    quickhop launch emoji:1f604 --query smile # run a result's default action (prints 😄)
    quickhop launch files:/home/me/notes.md --action reveal   # any action by kind (copy, trash, …)
    quickhop run hello Ada                    # run a script command with an argument
    quickhop index --watch                    # index files under ~ (or [files] roots), keep it current
    quickhop clipboard watch                  # record copied text (X11, wlroots/KDE Wayland)
    quickhop clipboard list                   # newest first; --json for everything
    quickhop clipboard paste 2                # paste the second newest into the focused app (X11)
    quickhop daemon &                         # one per user: clipboard history, live file index
    quickhop open emoji:1f604 --query smile   # an action through the daemon (starts one if needed)
    mkdir -p ~/.config/quickhop && quickhop config default > ~/.config/quickhop/config.toml

## Configuration

Everything lives in `~/.config/quickhop/` on both macOS and Linux (`$XDG_CONFIG_HOME` is
honoured; `$QUICKHOP_CONFIG` points at another file). Data such as ranking history lives in
`~/.local/share/quickhop/` (`$XDG_DATA_HOME`, `$QUICKHOP_DATA`).

    ~/.config/quickhop/
    ├── config.toml          # settings, providers, quicklinks
    ├── snippets.toml        # snippets with Raycast placeholders
    └── scripts/             # Raycast-compatible script commands
        ├── hello.sh
        └── ip.py

```toml
# ~/.config/quickhop/config.toml
hotkey = "Alt+Space"
max_results = 8

[providers]
emoji = true          # apps, calc, emoji, system, quicklinks, scripts

[scripts]
dirs = ["~/code/raycast-scripts"]   # more folders, after ~/.config/quickhop/scripts

[[quicklinks]]
name = "GitHub search"
url = "https://github.com/search?q={query}"
alias = "gh"          # type `gh tokio`

[[commands]]          # any result, by the id `quickhop query --json` prints
id = "system:lock"
alias = "lk"          # type `lk` to pin it
order = 1             # first in the empty root search

[[commands]]
id = "system:shutdown"
enabled = false       # never shown; unknown ids are config errors

[files]
roots = ["~/code", "~/Documents"]   # what `quickhop index` covers (default: ~)

[clipboard]
max_entries = 1000    # history kept by `quickhop clipboard watch`
max_age_days = 90     # 0 keeps entries forever
```

```toml
# ~/.config/quickhop/snippets.toml
[[snippets]]
name = "Reply"
keyword = ";re"
text = "Hi {argument name=\"name\"},\n\n{cursor}\n\nSent {date format=\"EEEE, MMM d\"} · {clipboard | trim}"
```

Snippets use Raycast's dynamic placeholders: `{clipboard}`, `{date}`, `{time}`, `{datetime}`
and `{day}` with `format` and `offset`, `{uuid}`, `{cursor}`, `{argument}`, `{snippet}`, and
the `| trim | uppercase | lowercase | percent-encode | json-stringify` modifiers.
`quickhop launch snippets:reply --arg Ada` prints the expanded text.

Upgrading from quickhop 0.1 on macOS moves `~/Library/Application Support/quickhop` to these
locations automatically.

## Script commands

A script command is any executable file with Raycast metadata in its comments. Existing Raycast
scripts work as they are (macOS-only ones need their macOS tools, of course).

```bash
#!/bin/bash

# @raycast.schemaVersion 1
# @raycast.title Hello
# @raycast.mode compact
# @raycast.packageName Greetings
# @raycast.icon 👋
# @raycast.argument1 { "type": "text", "placeholder": "Name", "optional": true }

echo "Hello, ${1:-world}!"
```

    chmod +x ~/.config/quickhop/scripts/hello.sh
    quickhop run hello Ada        # → Hello, Ada!

| Mode | Shows |
|------|-------|
| `fullOutput` | all output in a view (ANSI colours kept) |
| `compact` | the last line in a toast |
| `silent` | the last line in a HUD after the window hides |
| `inline` | the first line as the command's subtitle, refreshed every `refreshTime` |

Supported metadata: `schemaVersion`, `title`, `mode`, `packageName`, `icon`, `iconDark`,
`description`, `author`, `authorURL`, `platform`, `currentDirectoryPath`, `needsConfirmation`,
`refreshTime`, `argument1`..`argument3` (`text`, `password`, `dropdown`, `optional`,
`percentEncoded`).

## Keybindings

The launcher window is in development; these are its defaults.

| Key | Action |
|-----|--------|
| `Alt+Space` | open or close quickhop (set `hotkey` in the config) |
| `↑` `↓` / `Ctrl+P` `Ctrl+N` | move the selection |
| `Enter` | run the default action |
| `⌘K` / `Ctrl+K` | all actions for the selection |
| `Tab` | fill arguments, or ask AI |
| `Esc` | back, then close |
| `⌘,` / `Ctrl+,` | settings |

On Wayland desktops without the global-shortcuts portal, bind a key to `quickhop toggle` in
your compositor; `quickhop doctor` tells you which applies.

## Roadmap

1. **Launcher**: window, global hotkey, daemon, launch at login, packages.
2. **Daily drivers**: file search, clipboard history, snippets, script commands in the UI.
3. **Parity**: window management, currency and dates, AI, extensions, settings and themes.
4. **Long tail**: store, menu bar, notes, calendar, focus, translate, sync.

Each step, its target machines and its acceptance tests (including real UI tests on macOS and
on X11 and Wayland) are in [docs/plan.md](docs/plan.md).

## Contributing

Issues and pull requests are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md) (setup, the
`just check` gate, the rules), [docs/architecture.md](docs/architecture.md) and
[docs/testing.md](docs/testing.md). Coding agents: [AGENTS.md](AGENTS.md).

## License

Not chosen yet; see [LICENSE](LICENSE). Raycast's example scripts used as test fixtures are
MIT-licensed by Raycast Technologies Ltd.
