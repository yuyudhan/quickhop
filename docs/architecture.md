# Architecture

quickhop is one Rust workspace producing one binary, `quickhop`: a CLI today, and the
launcher daemon and window once the UI lands. Everything below the window is toolkit-free and
runs headless, so it is tested on every OS in CI. Decisions with lasting weight are recorded
as ADRs in [adr/](adr/); the feature inventory is [features.md](features.md) and the build
order [plan.md](plan.md).

## Crates

| Crate | State | Role |
|-------|-------|------|
| `quickhop-core` | built | Platform-independent model: `Item`, `ItemId`, `Action` (plain data: `Open`, `Copy`, `Run`, `Script`), `Query`, `Cancel`, the `Provider` trait, the nucleo fuzzy matcher (`fuzzy`), `Search`/`rank` (tiers, frecency, dedupe, limit) and `Config` (TOML, XDG paths, the one-time macOS migration). |
| `quickhop-providers` | built | Built-in providers over an explicit `Env` (OS + variables): `apps`, `calc`, `emoji`, `system`, `quicklinks`, `scripts`. `builtin(config, env)` loads the enabled ones; `script_dirs` resolves script directories. |
| `quickhop-store` | built | SQLite (rusqlite, bundled): `frecency.sqlite`, frecency per (query prefix, item), half-life 7 days; `state.sqlite` (`state`), the clipboard history. Grows the snippet, favorites and chat tables. |
| `quickhop` | built | The binary: `query`, `launch`, `run`, `config default\|path\|check`, `doctor`; later `daemon`, `toggle`, `open`. |
| `quickhop-ui` | model built | The launcher window in gpui ([ADR 0001](adr/0001-ui-toolkit.md)). `model` is built: the toolkit-free view model (query and numbered searches, results, wrapping selection, navigation stack, action panel with filtering, toasts). The window and its views are next (plan slice U2); `examples/spike.rs` is the U0 toolkit spike. |
| `quickhop-index` | planned | File index for Linux (and as a Spotlight fallback): walker, inotify/FSEvents watcher, SQLite FTS5. |
| `quickhop-ext` | planned | Extension host: spawns extensions, speaks JSON-RPC over stdio, maps their views to the UI model. |
| `quickhop-platform-macos` | started | The only place for FFI on macOS. Built: `icon` (any file's or app's icon as PNG through `NSWorkspace iconForFile:`, behind the `IconRenderer` trait), `pasteboard` (the `NSPasteboard` as a `ClipboardSource`, secrets skipped, writing text), `keys` (`⌘V` through `CGEventPost`, the Accessibility check and prompt). Planned: AX (window management, selected text), `NSRunningApplication`, `NSStatusItem`, EventKit. |
| `quickhop-platform-linux` | planned | Wayland protocols (data-control, virtual-keyboard, foreign-toplevel) via `wayland-client`, X11 via `x11rb`, D-Bus/portals via `zbus`/`ashpd`. |

Unsafe code is forbidden everywhere except the two platform crates, which must document every
unsafe block (`scripts/check-unsafe.sh`, CONTRIBUTING.md). Platform crates compile on every OS
(OS modules are `cfg`-gated) and expose safe traits the rest of the workspace calls: a
`Clipboard`, a `WindowManager`, a `Paster`, a `HotkeyRegistry`, each with a fake for tests.

## Directories

Configuration is XDG on macOS and Linux alike (Windows keeps `%APPDATA%`):

    ~/.config/quickhop/                 $XDG_CONFIG_HOME/quickhop, or $QUICKHOP_CONFIG's folder
    ├── config.toml                     settings, providers, quicklinks, hotkeys, aliases
    ├── scripts/                        Raycast-compatible script commands, always scanned
    ├── snippets.toml                   (planned) snippets, editable by hand
    ├── themes/                         (planned) custom themes
    └── extensions.toml                 (planned) installed extensions and their preferences

    ~/.local/share/quickhop/            $XDG_DATA_HOME/quickhop, or $QUICKHOP_DATA
    ├── frecency.sqlite                 picks per query prefix
    ├── state.sqlite                    (planned) clipboard history, snippets usage, favorites, AI chats
    ├── index.sqlite                    (planned) file index
    └── extensions/                     (planned) installed extensions

    $XDG_RUNTIME_DIR/quickhop.sock      (planned) daemon socket; $TMPDIR/quickhop-<uid>.sock on macOS

quickhop 0.1 kept both the config and the database in `~/Library/Application Support/quickhop`
on macOS. The first run of a newer binary moves `config.toml` and `frecency.sqlite*` to the
XDG locations when those are still free (`config::migrate_legacy`), and leaves locations
overridden by `QUICKHOP_CONFIG` / `QUICKHOP_DATA` alone.

## Process model

Today every CLI call loads the providers and searches once. The launcher adds a resident
process, started at login (`SMAppService` / LaunchAgent on macOS, XDG autostart or a systemd
user unit on Linux):

1. `quickhop daemon` loads the config and providers once, opens the store, starts the
   clipboard watcher, the inline-script scheduler and the file indexer, registers hotkeys, and
   keeps the hidden gpui window ready.
2. A single-instance lock and a Unix socket accept commands: `toggle`, `open <id>`,
   `run <script>`, `reload`. On Wayland without the GlobalShortcuts portal, the compositor
   binds a key to `quickhop toggle`.
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
changing it; whether to build it is an open question in [plan.md](plan.md).

## Indexing

`quickhop-index` serves file and folder search (FIL-01). On macOS the first source is Spotlight
(`mdfind` scoped to the home folder), with the index as a fallback for excluded volumes. On
Linux the index is primary: a parallel walk of configured roots (home by default, honouring
`.gitignore`, skipping caches and hidden folders unless configured), names and paths stored in
SQLite FTS5 with the trigram tokenizer, kept current by inotify (FSEvents on macOS) and a
nightly rescan for missed events. Queries go through the same fuzzy ranking as every other
provider. The indexer runs inside the daemon at low priority and is cancellable.

## Storage

- `frecency.sqlite` (built): one row per (normalised query prefix, item id) holding a score
  already decayed to its last update.
- `state.sqlite` (clipboard built), migrated by `PRAGMA user_version` with a list of SQL steps
  in `quickhop-store` (`rusqlite_migration` lags behind the `rusqlite` version in use):
  `clipboard` (SHA-256 content hash for dedupe, kind, text or blob path, source app, pinned,
  first and last copy times, copy count, a sequence number for order; images and files stored
  as files beside it), later `snippets_usage`, `favorites`, `aliases`, `hotkeys`,
  `chats`/`messages`.
  Clipboard retention is configurable; encryption at rest uses a key from the OS keyring
  (Keychain, Secret Service).
- Snippets and quicklinks are authored in TOML under the config directory so they sync and
  diff as text; the database only keeps usage data.

## Packaging

| OS | Artefacts | Notes |
|----|-----------|-------|
| macOS | `quickhop.app` (LSUIElement agent app, the CLI inside `Contents/MacOS`), DMG, Homebrew cask; `cargo install` for the CLI | signed with a Developer ID and notarized in CI; universal binary (`aarch64` + `x86_64`) with `lipo`; `quickhop://` URL scheme in Info.plist |
| Linux | `.deb`, `.rpm` (cargo-deb, cargo-generate-rpm), AUR `PKGBUILD`, tarball; `cargo install` | ships `quickhop.desktop` (with `x-scheme-handler/quickhop`), hicolor icons, a systemd user unit and an autostart entry; Flatpak later, since launching host apps needs `flatpak-spawn --host` |

Release builds run on GitHub Actions for both OSes from a tag; the UI test jobs in
[plan.md](plan.md) gate them.

## Testing

See [testing.md](testing.md).
