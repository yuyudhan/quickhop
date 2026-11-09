# Architecture

quickhop is one Rust workspace producing one binary, `quickhop`: a CLI today, and the
launcher daemon and window once the UI lands. Everything below the window is toolkit-free and
runs headless, so it is tested on every OS in CI. Decisions with lasting weight are recorded
as ADRs in [adr/](adr/); the feature inventory is [features.md](features.md) and the build
order [plan.md](plan.md).

## Crates

| Crate | State | Role |
|-------|-------|------|
| `quickhop-core` | built | Platform-independent model: `Item`, `ItemId`, `Action` (plain data: `Open`, `Copy`, `Run`, `Script`, `Snippet`, `Reveal`, `Trash`), `Query`, `Cancel`, the `Provider` trait, the nucleo fuzzy matcher (`fuzzy`), `Search`/`rank` (tiers, frecency, dedupe, limit, `[[commands]]` overrides), `Config` (TOML, XDG paths, the one-time macOS migration), the snippet library (`snippets`) and Raycast's dynamic placeholders (`placeholders`, with LDML date patterns in `datefmt`), expanded from a caller-supplied context; `clipboard` (`Contents`, what every platform watcher reports, secrets included so they can be skipped; `Watcher` over a polled `ClipboardSource`) and `paste` (`paste_text` over the `ClipboardWriter` and `PasteKeys` traits). |
| `quickhop-providers` | built | Built-in providers over an explicit `Env` (OS + variables): `apps`, `calc`, `emoji`, `system`, `quicklinks`, `scripts`, `snippets`, `files` (behind the `FileSource` trait: Spotlight on macOS, quickhop's index elsewhere). `builtin(config, env)` loads the enabled ones; `script_dirs` resolves script directories. |
| `quickhop-store` | built | SQLite (rusqlite, bundled): `frecency.sqlite`, frecency per (query prefix, item), half-life 7 days; `state.sqlite` (`state`), the clipboard history. Grows the snippet, favorites and chat tables. |
| `quickhop` | built | The binary: `query`, `launch`, `run`, `index [--watch]`, `clipboard watch\|add\|list\|paste\|clear`, `daemon [status\|stop]`, `open`, `reload`, `autostart`, `config default\|path\|check`, `doctor`; later `toggle`. |
| `quickhop-ui` | model built | The launcher window in gpui ([ADR 0001](adr/0001-ui-toolkit.md)). `model` is built: the toolkit-free view model (query and numbered searches, results, wrapping selection, navigation stack, action panel with filtering, toasts). The window and its views are next (plan slice U2); `examples/spike.rs` is the U0 toolkit spike. |
| `quickhop-index` | built | File index for Linux (and as a Spotlight fallback): parallel walk (`ignore`), `notify` watcher, SQLite FTS5 trigram index searched through `fuzzy`. |
| `quickhop-ext` | planned | Extension host: spawns extensions, speaks JSON-RPC over stdio, maps their views to the UI model. |
| `quickhop-platform-macos` | started | The only place for FFI on macOS. Built: `icon` (any file's or app's icon as PNG through `NSWorkspace iconForFile:`, behind the `IconRenderer` trait), `pasteboard` (the `NSPasteboard` as a `ClipboardSource`, secrets reported as such, writing text), `keys` (`⌘V` through `CGEventPost`, the Accessibility check and prompt). Planned: AX (window management, selected text), `NSRunningApplication`, `NSStatusItem`, EventKit. |
| `quickhop-platform-linux` | started | Built: the X11 clipboard watcher (`x11rb`, `XFixes`, `INCR` transfers) and Ctrl+V through `XTest`. Planned: Wayland protocols (data-control, virtual-keyboard, foreign-toplevel) via `wayland-client`, D-Bus/portals via `zbus`/`ashpd`. |

Unsafe code is forbidden everywhere except the two platform crates, which must document every
unsafe block (`scripts/check-unsafe.sh`, CONTRIBUTING.md). Platform crates compile on every OS
(OS modules are `cfg`-gated) and expose safe traits the rest of the workspace calls: a
`Clipboard`, a `WindowManager`, a `Paster`, a `HotkeyRegistry`, each with a fake for tests.

## Directories

Configuration is XDG on macOS and Linux alike (Windows keeps `%APPDATA%`):

    ~/.config/quickhop/                 $XDG_CONFIG_HOME/quickhop, or $QUICKHOP_CONFIG's folder
    ├── config.toml                     settings, providers, quicklinks, hotkeys, aliases
    ├── scripts/                        Raycast-compatible script commands, always scanned
    ├── snippets.toml                   snippets with Raycast placeholders, editable by hand
    ├── themes/                         (planned) custom themes
    └── extensions.toml                 (planned) installed extensions and their preferences

    ~/.local/share/quickhop/            $XDG_DATA_HOME/quickhop, or $QUICKHOP_DATA
    ├── frecency.sqlite                 picks per query prefix
    ├── state.sqlite                    clipboard history; later snippets usage, favorites, AI chats
    ├── index.sqlite                    file index (`quickhop index`)
    └── extensions/                     (planned) installed extensions

    $XDG_RUNTIME_DIR/quickhop/          daemon.lock and daemon.sock; $QUICKHOP_RUNTIME, else
                                        quickhop-$USER in $TMPDIR (per user on macOS) or /tmp

quickhop 0.1 kept both the config and the database in `~/Library/Application Support/quickhop`
on macOS. The first run of a newer binary moves `config.toml` and `frecency.sqlite*` to the
XDG locations when those are still free (`config::migrate_legacy`), and leaves locations
overridden by `QUICKHOP_CONFIG` / `QUICKHOP_DATA` alone.

## Process model

CLI calls such as `query` and `launch` load the providers and search once. The resident
process, started at login (`SMAppService` / LaunchAgent on macOS, XDG autostart or a systemd
user unit on Linux, with U4):

1. `quickhop daemon` (built, Unix): takes an exclusive lock on `daemon.lock` (a second daemon
   exits with "already running (pid N)"; the kernel drops the lock when a daemon dies, so the
   next one removes the stale socket), listens on `daemon.sock`, starts the clipboard watcher,
   and rebuilds the file index on a thread before watching it (catching up changes missed
   while no daemon ran). Later: loads the providers once, the inline-script scheduler,
   hotkeys, and the hidden gpui window kept ready.
2. Requests are one JSON line each way per connection (`{"cmd": "status" | "open" |
   "reload" | "stop", …}` → `{"ok", "pid", "message"}`): `quickhop daemon status|stop`,
   `quickhop open <id>` (runs the action in the daemon, as `launch` does, starting a daemon
   first when none answers), `quickhop reload` (a broken config is refused and the old one
   kept; the file index restarts with the new roots). Later: `toggle` with the window, and
   `run <script>`. On Wayland without the GlobalShortcuts portal, the compositor binds a key
   to `quickhop toggle`.
3. Per keystroke the UI runs `Search::run` with a fresh `Cancel` (the previous search is
   cancelled), on a worker thread, and renders the ranked list.
4. Config, scripts and snippets are watched (`notify`) and reloaded in place.

## Providers and search

| Provider | Source | Default action |
|----------|--------|----------------|
| `apps` (Linux, BSD) | XDG desktop entries in `$XDG_DATA_HOME/applications` then each `$XDG_DATA_DIRS/applications`; desktop-file-id precedence, localised `Name`/`GenericName`/`Keywords`, `Exec` quoting and field codes, `TryExec`, `NoDisplay`/`Hidden`, `OnlyShowIn`/`NotShowIn` against `$XDG_CURRENT_DESKTOP`, `Terminal=true` via `$TERMINAL`; icons by the Icon Theme spec | `Run` |
| `apps` (macOS) | `.app` bundles in `/Applications`, `/System/Applications`, `~/Applications` (two folder levels), `Info.plist` name, identifier, executable, icon; background agents skipped | `Open` the bundle |
| `apps` (Windows) | `.lnk` shortcuts in both Start Menu `Programs` folders; packaged apps from `Get-StartApps` | `Open` |
| `calc` | fend-core: arithmetic, units, bases; only queries with a digit; 500 ms limit, cancellable | `Copy` the result |
| `emoji` | `emojis` crate + CLDR English keywords; GitHub shortcodes as keywords, `:code` aliases | `Copy` the emoji |
| `system` | lock, log out, suspend, reboot, shut down: logind, macOS `pmset`/`osascript`, Windows `shutdown.exe` | `Run` with `confirm: true` |
| `quicklinks` | `[[quicklinks]]` with `{query}` templates (percent-encoded); `alias text` pins the link | `Open` |
| `scripts` | Raycast script commands from `~/.config/quickhop/scripts` and `[scripts] dirs` (see below) | `Script` |
| `snippets` | `[[snippets]]` in `snippets.toml` next to the config (name, text, keyword) | `Snippet`: expand placeholders, then copy |
| `files` | the file index (`index.sqlite`), queries of 3 or more characters, 50 best | `Open`; then `Reveal` in the file manager, `Copy` the path, `Trash` (confirmed) |

Ranking: `rank` classifies each candidate into a tier, best first: `Pinned` (an alias equals
the query or its first word), `Computed` (calculator), `Matched` (nucleo score of the title, or
of a keyword minus `KEYWORD_PENALTY`). The score gains `FRECENCY_WEIGHT * ln(1 + frecency)`;
duplicates keep their best rank; results sort by tier, score, then provider order, and are cut
to `max_results`. Picking an item runs its first `Action` and records the pick for every prefix
of the query (frecency halves every 7 days).

## Script commands

Raycast's script-command format is quickhop's simplest extension mechanism, kept compatible so
the hundreds of scripts in raycast/script-commands run unchanged where their interpreter exists:

- Discovery (`scripts::discover`): every file directly in each script directory with
  `@raycast.` headers in comment lines (`#`, `//`, `--`, `;`, `'`, `::`, `REM`); hidden files and
  `*.template.*` files skipped; at most 512 KiB.
- Metadata (`scripts::parse`): `schemaVersion` 1, `title`, `mode` required; `packageName`
  (defaults to the folder name), `icon`/`iconDark` (emoji, https URL or path relative to the
  script), `description`, `author`, `authorURL`, `platform`, `currentDirectoryPath` (relative
  to the script, `~` expanded at run time), `needsConfirmation`, `refreshTime` (10 s floor;
  `inline` without it runs as `compact`, as in Raycast), `argument1`..`argument3` (JSON: `text`,
  `password`, `dropdown` with `data`, `optional`, `percentEncoded`).
- Running (`scripts::run`): directly on Unix (the shebang decides), by extension or shebang on
  Windows; arguments are validated and passed positionally; stdout and stderr are captured;
  a timeout kills the process. Output is shaped per mode: all of it for `fullOutput` (ANSI
  colour kept), the last line for `compact` and `silent` (colour stripped), the first line for
  `inline`; a failing script reports that line, or the last stderr line when it printed none.
- Surfaces: `quickhop run <script> [args]` (by path, stem or title), `quickhop launch
  scripts:<file>`, root search items; the UI adds argument fields, the output view, toasts and
  the inline refresh scheduler.

## Extension model

Three tiers, from simplest to richest:

1. **Built-in providers**: Rust, in-process, implementing `Provider` (`items(query, cancel)`)
   and passing the provider contract tests. For things every user has.
2. **Script commands**: any executable with Raycast metadata, as above. No SDK, no build.
3. **Extensions**: out-of-process programs in any language, installed under
   `~/.local/share/quickhop/extensions/<id>/` with an `extension.toml` manifest (id, title,
   commands with modes `view`/`no-view`/`menu-bar`, preferences, the executable and its
   arguments). quickhop starts the command's process and exchanges newline-delimited
   JSON-RPC 2.0 over stdio: the extension sends a view tree (`List`, `Detail`, `Form`, `Grid`
   with sections, items, accessories and actions) and updates; quickhop sends search text,
   selections, action invocations and form values. Capabilities (clipboard, open URL, local
   storage, preferences, secrets via the OS keyring, toasts, AI) are host calls the manifest must
   declare. A crashed or slow extension only fails its own command.

A compatibility layer for Raycast's TypeScript extensions (an `@raycast/api` shim running on
Node or Bun and translating the React tree to tier-3 views) fits on top of tier 3 without
changing it. The owner decided to build quickhop's own extension API first and the shim later
([ADR 0002](adr/0002-licence-extensions-ai.md)).

## Indexing

`quickhop-index` serves file and folder search (FIL-01). On macOS the first source will be
Spotlight (`mdfind` scoped to the home folder), with the index as a fallback for excluded
volumes. On Linux the index is primary: a parallel walk of the configured roots (`[files]
roots`, home by default, honouring `.gitignore` inside git repositories, skipping hidden
folders and version-control, dependency, build and cache folders), paths stored in SQLite
(`files`) with an FTS5 trigram index (`detail=none`, contentless) kept current by `notify`
(inotify, FSEvents). A query looks up the two rarest trigrams of each word of 3 or more
characters (entry counts are kept per trigram at each rebuild), verifies every word is a
substring of the path, and ranks the candidates by the fuzzy score of the name, else of the
path. `quickhop index` rebuilds it; `--watch` keeps it current until stopped; the daemon
rebuilds it at start, then watches it. On itlinux3 a 200,000-file tree indexes in about 4.5 s
and queries answer in under 15 ms (`crates/quickhop-index/tests/scale.rs`).

File actions: `Reveal` calls `org.freedesktop.FileManager1.ShowItems` on the session bus
(`gdbus`; the parent folder opens when no file manager answers), `open -R` on macOS and
`explorer /select,` on Windows. `Trash` follows the FreeDesktop.org Trash specification for
the home trash (`$XDG_DATA_HOME/Trash/files` plus an `info/*.trashinfo` record, `.2`, `.3`, …
on name clashes) and hands files on other filesystems to `gio trash`; Finder (`osascript`) and
the Recycle Bin (`Microsoft.VisualBasic`) elsewhere. `quickhop launch <id> --action <kind>`
runs any of an item's actions by kind.

## Storage

- `frecency.sqlite` (built): one row per (normalised query prefix, item id) holding a score
  already decayed to its last update.
- `state.sqlite` (clipboard built), migrated by `PRAGMA user_version` with a list of SQL steps
  in `quickhop-store` (`rusqlite_migration` lags behind the `rusqlite` version in use):
  `clipboard` (SHA-256 content hash for dedupe, kind, text or blob path, source app, pinned,
  first and last copy times, copy count, a sequence number for order; images and files stored
  as files beside it), later `snippets_usage`, `favorites`, `aliases`, `hotkeys`,
  `chats`/`messages`. `[clipboard] max_entries` (default 1000) and `max_age_days` (default 90,
  0 keeps forever) are applied after every copy, and blank texts and texts over 1 MiB are not
  kept. Encryption at rest uses a key from the OS keyring (Keychain, Secret Service).
- Clipboard watching (`quickhop clipboard watch`, and inside the daemon): every platform
  watcher reports `quickhop_core::clipboard::Contents`, and copies a password manager marks as
  concealed or transient are never recorded. macOS polls `NSPasteboard`'s change count through
  `Watcher` and skips `org.nspasteboard.ConcealedType`/`TransientType`; X11 uses `XFixes`
  selection-owner events on `CLIPBOARD`, asks the owner for `TARGETS` and, unless it offers
  `x-kde-passwordManagerHint`, for `UTF8_STRING` (large texts in `INCR` chunks); Wayland
  compositors with a data-control protocol go through `wl-paste --watch quickhop clipboard
  add`, which skips what `wl-paste` calls `sensitive` (older `wl-paste`: the hint in
  `--list-types`). GNOME's Wayland session has no such protocol, so there copies are only seen
  while quickhop has focus (with the window, later).
- Pasting (`quickhop clipboard paste [n]`, `launch`/`open --paste` for copy and snippet
  results, checked before anything runs) is `quickhop_core::paste::paste_text`: the text goes
  on the clipboard first, then the paste shortcut is sent when the platform allows it: `⌘V`
  through `CGEventPost` with the Accessibility permission on macOS, Ctrl+V through `XTest` on
  X11. On Wayland (the virtual-keyboard protocol and the `RemoteDesktop` portal come later;
  wtype did not paste into GTK in headless sway 1.7) and without the permission the text stays
  on the clipboard and quickhop says why.
- Snippets and quicklinks are authored in TOML under the config directory so they sync and
  diff as text; the database only keeps usage data.

## Packaging

| OS | Artefacts | Notes |
|----|-----------|-------|
| macOS | `quickhop.app` (LSUIElement agent app, the CLI inside `Contents/MacOS`), DMG, Homebrew cask; `cargo install` for the CLI | signed with a Developer ID and notarized in CI; universal binary (`aarch64` + `x86_64`) with `lipo`; `quickhop://` URL scheme in Info.plist |
| Linux | built: `.deb` (`dpkg-deb`, dependencies from `dpkg-shlibdeps`) and tarball from `just bundle-linux`; later `.rpm`, AUR `PKGBUILD`; `cargo install` | ships `quickhop.desktop` (later with `x-scheme-handler/quickhop`), a hicolor SVG icon and a systemd user unit (`systemctl --user enable --now quickhop`); `quickhop autostart on` writes an XDG autostart entry instead (GNOME and KDE turn it into a unit with `systemd-xdg-autostart-generator`). `just bundle-linux-test` installs the `.deb` in `debian:bookworm-slim` and runs `doctor` and a query. Flatpak later, since launching host apps needs `flatpak-spawn --host` |

Release builds run on GitHub Actions for both OSes from a tag; the UI test jobs in
[plan.md](plan.md) gate them. Each release also attaches, per target
(`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `universal-apple-darwin`),
`quickhop-<target>.tar.gz` and its `.sha256` from `scripts/dist.sh`, plus `scripts/install.sh`,
the no-sudo per-user installer ([install.md](install.md#the-installer-script)) that verifies
the checksum and installs into `~/.local/bin`, `~/.local/share` and, for an archive carrying
`quickhop.app`, `/Applications`.

## Testing

See [testing.md](testing.md).
