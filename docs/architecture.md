# Architecture

## Crates

| Crate | Role |
|-------|------|
| `quickhop-core` | Platform-independent model, no IO: `Item`, `ItemId`, `Action` (plain data: `Open`, `Copy`, `Run`, `Script`), `Query`, `Cancel`, the `Provider` trait, the nucleo fuzzy matcher (`fuzzy`), `Search`/`rank` (tiers, frecency, dedupe, limit) and `Config` (TOML, per-OS config and data paths). |
| `quickhop-providers` | Built-in providers over an explicit `Env` (OS + variables): `apps`, `calc`, `emoji`, `system`, `quicklinks`, `scripts`. `builtin(config, env)` loads the enabled ones. |
| `quickhop-store` | SQLite (rusqlite, bundled): frecency per (query prefix, item), half-life 7 days. |
| `quickhop-ui` | The launcher window. Toolkit not chosen yet (T91); no code until then. |
| `quickhop` | The binary: `query`, `launch`, `config default\|path\|check`, `doctor`. |

Nothing here depends on the UI toolkit or the process model (daemon or not): a UI or daemon
calls `quickhop_providers::builtin` once, keeps the providers, and runs `Search::run` per
keystroke with a fresh `Cancel`.

## Providers

| Provider | Source | Default action |
|----------|--------|----------------|
| `apps` (Linux, BSD) | XDG desktop entries in `$XDG_DATA_HOME/applications` then each `$XDG_DATA_DIRS/applications`; desktop-file-id precedence, localised `Name`/`GenericName`/`Keywords`, `Exec` quoting and field codes, `TryExec`, `NoDisplay`/`Hidden`, `OnlyShowIn`/`NotShowIn` against `$XDG_CURRENT_DESKTOP`, `Terminal=true` via `$TERMINAL`; icons by the Icon Theme spec (configured or GTK theme, its `Inherits`, `hicolor`, `pixmaps`) | `Run` |
| `apps` (macOS) | `.app` bundles in `/Applications`, `/System/Applications`, `~/Applications` (two folder levels), `Info.plist` (XML or binary) name, identifier, executable, icon; background agents skipped | `Open` the bundle |
| `apps` (Windows) | `.lnk` shortcuts in both Start Menu `Programs` folders (uninstallers skipped; the file name is the title); packaged (UWP) apps from PowerShell `Get-StartApps`, no COM | `Open` the shortcut or `shell:AppsFolder\<AUMID>` |
| `calc` | fend-core: arithmetic, units, bases; only queries with a digit; 500 ms limit, cancellable | `Copy` the result |
| `emoji` | `emojis` crate + CLDR English keywords (`data/emoji-keywords.tsv`, `scripts/gen-emoji-keywords.py`); GitHub shortcodes as keywords, `:code` / `:code:` as aliases | `Copy` the emoji |
| `system` | lock, log out, suspend, reboot, shut down: logind (`loginctl`, `systemctl`), macOS `pmset`/`osascript`, Windows `rundll32`/`shutdown.exe` | `Run` with `confirm: true` |
| `quicklinks` | `[[quicklinks]]` with `{query}` templates (percent-encoded); `alias text` (or `name text`) pins the link | `Open` |
| `scripts` | Raycast script commands (`# @raycast.*` headers, all four modes, up to three arguments, `needsConfirmation`) from `[scripts] dirs`, none by default; inline scripts run once at load (2 s limit) for their subtitle | `Script` |

Scanners are plain file parsing and compile on every OS, so their tests run everywhere;
`apps::Sources::for_env` picks the one for the running OS (configured `[apps] dirs` replace the
defaults). Problems while loading (broken files, failing inline scripts) are returned as
warnings, never as a failed provider.

## Search flow

1. The caller turns the typed text into a `Query` and fetches frecency `Boosts` for it from
   the store.
2. `Search::run` asks every provider, in order, for candidates; a failing provider lands in
   `SearchResults::errors`, a set `Cancel` stops the remaining ones.
3. `rank` classifies each candidate into a `Tier`, best first:
   - `Pinned`: an alias equals the query or its first word (quicklinks, `:smile`);
   - `Computed`: from a computing provider (calculator);
   - `Matched`: nucleo fuzzy score of the title (positions kept for highlighting), or of a
     keyword minus `KEYWORD_PENALTY` (waived when the keyword is exactly the query).
4. The score gains `FRECENCY_WEIGHT * ln(1 + frecency)`.
5. Duplicates (same `ItemId`) keep their best rank; results sort by tier, then score, then
   candidate order (provider order, then provider order of items); the list is cut to
   `max_results`.
6. Picking an item runs its first `Action` (the CLI's `launch`; power commands and
   `needsConfirmation` scripts need `--yes`) and records the pick for every prefix of the
   query.

## Frecency

`quickhop-store` keeps one row per (normalised query prefix, item): a score already decayed
to its last update. A pick decays the stored score to now and adds 1; a lookup decays it to
now. The decay halves every 7 days (`HALF_LIFE_SECS`). The database is
`<data dir>/frecency.sqlite`: `$XDG_DATA_HOME/quickhop`, `~/Library/Application
Support/quickhop`, `%LOCALAPPDATA%\quickhop`, or `$QUICKHOP_DATA`.

## Testing

See [testing.md](testing.md).

## Rules

No unsafe code anywhere, 3000-line file limit, 4-space indentation: see CONTRIBUTING.md.
