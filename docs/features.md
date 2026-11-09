# Features: Raycast parity inventory and gap analysis

Every Raycast feature quickhop aims to match, what exists today, and how each one is done on
macOS and on Linux (X11 and Wayland). Sources: raycast.com, the Raycast manual
(manual.raycast.com), the Raycast store categories, the developer docs
(developers.raycast.com) and the script-commands repository
(github.com/raycast/script-commands).

- **Status**: `done` works today from the CLI and is tested; `partial` has the core but not the
  launcher UI or part of the behaviour; `planned` is not started.
- **Priority**: `P0` the launcher is not usable without it (first release); `P1` what people
  reach Raycast for daily (parity release); `P2` long tail.
- IDs are stable: [plan.md](plan.md) slices and tests refer to them.

"AX" is the macOS Accessibility API (`AXUIElement`), which needs the user's Accessibility
permission. "Portal" is an XDG Desktop Portal over D-Bus (`org.freedesktop.portal.*`).

## Launcher core

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| LCH-01 | Global hotkey opens/closes the launcher (default `Alt+Space`) | planned | P0 | `RegisterEventHotKey` (Carbon) via the `global-hotkey` crate; no permission needed | X11: `XGrabKey` (`global-hotkey`). Wayland: `GlobalShortcuts` portal (KDE 5.27+, GNOME 48+, Hyprland); else a compositor binding to `quickhop toggle` (sway `bindsym`, Hyprland `bind`, GNOME custom shortcut via `gsettings`), set up by `quickhop doctor --fix` |
| LCH-02 | Root search: fuzzy match over every provider | done | P0 | nucleo matcher in `quickhop-core`, shared | same |
| LCH-03 | Frecency ranking per query prefix (7-day half-life) | done | P0 | SQLite store | same |
| LCH-04 | Aliases for any command (`g rust`, `:smile`) | partial | P0 | quicklinks and emoji today; per-command `alias` in config for every item | same |
| LCH-05 | Hotkey per command (open an app or run a script without the launcher) | planned | P1 | `global-hotkey` registrations from `[[commands]] hotkey` | X11 grabs; Wayland portal shortcuts (one per command) or compositor bindings to `quickhop run`/`quickhop open <id>` |
| LCH-06 | Favorites pinned at the top of the empty root | planned | P1 | store table `favorites` | same |
| LCH-07 | Action panel (`⌘K` / `Ctrl+K`) with every action of the selected item | planned | P0 | `Item::actions` already carries them; UI list | same |
| LCH-08 | Fallback commands when nothing matches (web search, file search, AI) | planned | P1 | config `fallbacks = [...]` of command ids, ranked after results | same |
| LCH-09 | Enable, disable and reorder root commands | partial | P1 | `[providers]` switches today; per-command `enabled` and order next | same |
| LCH-10 | Navigation stack: commands push views, `Esc` pops, `Esc` at root hides | planned | P0 | UI | same |
| LCH-11 | Launcher window: borderless, rounded, translucent/blurred, on the active screen, hides on focus loss | planned | P0 | gpui `WindowBackgroundAppearance::Blurred` (NSVisualEffectView), `NSPanel`-style non-activating window, screen of the mouse/key window | X11: override-redirect-free centered dialog, `_NET_WM_STATE_ABOVE`, ARGB visual; Wayland: `wlr-layer-shell` overlay (sway, Hyprland, KWin, COSMIC) with KDE blur protocol when offered; GNOME (no layer-shell): normal centered undecorated window |
| LCH-12 | Command arguments inline in the search bar (`alias` + space focuses them) | planned | P1 | UI over `ScriptArgument` | same |
| LCH-13 | Deeplinks: `quickhop://commands/<id>?arguments=...` | planned | P1 | `CFBundleURLTypes` in Info.plist | `x-scheme-handler/quickhop` in the `.desktop` file, `xdg-mime` |
| LCH-14 | Compact window mode and adjustable size/position | planned | P2 | config `[window]` | same |
| LCH-15 | Root search history | planned | P2 | store | same |

## Applications and files

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| APP-01 | Search and open installed applications | done | P0 | `.app` bundles in `/Applications`, `/System/Applications`, `~/Applications` (Info.plist), opened with `open` | XDG desktop entries (`$XDG_DATA_DIRS/applications`, precedence, `TryExec`, `OnlyShowIn`, field codes), run directly; Flatpak and Snap exports are desktop entries too |
| APP-02 | Application icons | partial | P0 | `.icns` path resolved; rendering to PNG in the UI (`NSWorkspace iconForFile` in the platform crate for asset-catalog-only apps) | Icon Theme spec lookup done (theme, `Inherits`, `hicolor`, pixmaps); SVG rendering with `resvg` in the UI |
| APP-03 | Running apps: quit, hide, force quit, show in Finder | planned | P1 | `NSRunningApplication` (platform crate) | `/proc` scan, `kill`; window-level actions via WIN mechanisms |
| APP-04 | Switch windows (search open windows, focus one) | planned | P1 | AX window list + `CGWindowListCopyWindowInfo` (Screen Recording permission for titles on macOS 10.15+) | X11: EWMH `_NET_CLIENT_LIST`, `_NET_ACTIVE_WINDOW`. Wayland: `wlr-foreign-toplevel-management` / `ext-foreign-toplevel-list` (sway, Hyprland, COSMIC), KWin scripting over D-Bus, GNOME Shell extension (no protocol) |
| APP-05 | Watch application folders and refresh without restart | planned | P1 | FSEvents via `notify` | inotify via `notify` on every `applications` dir |
| FIL-01 | File and folder search by name | planned | P0 | Spotlight: `mdfind` (`NSMetadataQuery` later) scoped to home | quickhop's own index (`quickhop-index`: walk + inotify, gitignore-aware, SQLite FTS5 trigram); uses `plocate` or GNOME `localsearch`/KDE `baloo` results when present |
| FIL-02 | File actions: open, open with, reveal, copy path, Quick Look, move to trash | planned | P1 | `open -a`, `open -R`, `qlmanage -p`, `NSFileManager trashItem` | `xdg-open`, `gio launch`, `org.freedesktop.FileManager1.ShowItems` (D-Bus), `sushi`/`gnome-sushi` or `xdg-open`, `gio trash` |
| FIL-03 | File content search | planned | P2 | Spotlight `kMDItemTextContent` | index extension with FTS5 over text files; `localsearch` when present |
| FIL-04 | Recent files and downloads | planned | P2 | Spotlight `kMDItemLastUsedDate`, `~/Downloads` | `recently-used.xbel` (GTK), `~/Downloads` |

## Calculator

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| CAL-01 | Arithmetic, functions, bases, percentages | done | P0 | fend-core, shared | same |
| CAL-02 | Unit conversion (`5 km to miles`) | done | P0 | fend-core | same |
| CAL-03 | Currency conversion with cached rates | planned | P1 | fend custom units fed from a daily ECB rates fetch cached in the data dir; offline uses the last rates | same |
| CAL-04 | Dates and time zones (`time in tokyo`, `days until dec 25`, `unix 1700000000`) | planned | P1 | `jiff` with bundled tzdb, shared | same |
| CAL-05 | Calculator history | planned | P2 | store table | same |

## Clipboard history

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| CLP-01 | Text clipboard history, searchable, kept in SQLite | planned | P0 | daemon polls `NSPasteboard.changeCount` (250 ms) | X11: XFixes `SelectionNotify` on `CLIPBOARD`. Wayland: `ext-data-control-v1` / `wlr-data-control` (sway, Hyprland, KWin, COSMIC) or `wl-paste --watch`; GNOME has no data-control protocol: watch only while quickhop has focus, or the optional GNOME Shell extension |
| CLP-02 | Images, files and rich text in history | planned | P1 | pasteboard types `public.png`, `public.file-url`, `public.rtf` | MIME targets `image/png`, `text/uri-list`, `text/html` |
| CLP-03 | Paste the chosen entry into the frontmost app | planned | P0 | set pasteboard, hide, synthesize `⌘V` with `CGEventPost` (Accessibility) | set clipboard (`wl-copy` / X11 owner), then X11: XTest `Ctrl+V`; Wayland: `virtual-keyboard-unstable-v1` (wlroots), `RemoteDesktop` portal keyboard (GNOME, KDE), or `ydotool`; else copy only and say so |
| CLP-04 | Ignore secrets and chosen apps; pin entries; delete | planned | P1 | skip `org.nspasteboard.ConcealedType`/`TransientType`; frontmost app via `NSWorkspace` | skip `x-kde-passwordManagerHint=secret`; X11 `_NET_ACTIVE_WINDOW` class; Wayland: per-target MIME hints only |
| CLP-05 | Paste as plain text | planned | P1 | write `public.utf8-plain-text` only | offer `text/plain;charset=utf-8` only |
| CLP-06 | Retention limits and encryption at rest | planned | P2 | key in Keychain (Security framework) | key in Secret Service (`org.freedesktop.secrets`) via `oo7`/`secret-service` |

## Snippets

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| SNP-01 | Snippet library: create, search, copy or paste | planned | P0 | store + `snippets.toml` in the config dir; paste as CLP-03 | same |
| SNP-02 | Dynamic placeholders: `{clipboard}`, `{date}`/`{time}`/`{datetime}` with formats and offsets, `{cursor}`, `{uuid}`, `{argument name="..." default="..."}`, `{snippet name="..."}` | planned | P0 | pure expansion engine in core, shared | same |
| SNP-03 | Keyword auto-expansion while typing in any app | planned | P1 | `CGEventTap` listen-only (Input Monitoring permission), then delete + paste | X11: XRecord or XInput2 raw events. Wayland: no global key monitoring; `evdev` keyboard read with the user in `input` (opt-in), or an IBus/Fcitx5 input-method engine |
| SNP-04 | Import and export (Raycast snippets JSON) | planned | P1 | shared | same |

## Quicklinks

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| QLK-01 | URL/path templates with `{query}` and an alias | done | P0 | `open` | `xdg-open` |
| QLK-02 | Raycast placeholders `{argument}`, `{clipboard}`, `{selection}`, `{date}`; open with a chosen app; create from the UI | partial | P1 | `open -a <app>` | `gio launch` / desktop entry `Exec` |
| QLK-03 | Import Raycast quicklinks export | planned | P2 | shared | same |

## Window management

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| WIN-01 | Halves, thirds, quarters, maximize, almost-maximize, center, restore | planned | P1 | AX `kAXPositionAttribute`/`kAXSizeAttribute` on the focused window, `NSScreen.visibleFrame` | X11: EWMH `_NET_MOVERESIZE_WINDOW`, `_NET_WORKAREA`. Wayland: no client protocol, so compositor IPC: `swaymsg`, `hyprctl dispatch`, KWin scripting (D-Bus `org.kde.kwin.Scripting`), GNOME Shell extension; unsupported compositors report it |
| WIN-02 | Move to next/previous display and desktop | planned | P1 | AX + `NSScreen`; Spaces via `CGSPrivate` is not used (no private API), desktop moves are P2 | X11: `_NET_WM_DESKTOP`; Wayland: compositor IPC |
| WIN-03 | Custom layouts and hotkeys per layout | planned | P2 | config `[[window.layouts]]` | same |

## System commands

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| SYS-01 | Lock, log out, sleep, restart, shut down, with confirmation | done | P0 | `pmset`, `osascript` (System Events) | logind: `loginctl lock-session`, `systemctl suspend/reboot/poweroff`, `loginctl terminate-session` |
| SYS-02 | Empty trash, eject disks, toggle dark mode, volume and mute, sleep displays, hide all apps | planned | P1 | `osascript`/Finder, `diskutil eject`, AppleScript appearance, CoreAudio (platform crate), `pmset displaysleepnow` | `gio trash --empty`, `udisksctl unmount/power-off`, portal `Settings` + `gsettings org.gnome.desktop.interface color-scheme` / KDE `plasma-apply-colorscheme`, `wpctl`/`pactl`, DPMS (`xset dpms force off` / compositor IPC) |
| SYS-03 | Do Not Disturb toggle | planned | P2 | Focus modes have no public API: Shortcuts (`shortcuts run`) | GNOME `gsettings org.gnome.desktop.notifications show-banners`; KDE D-Bus `org.freedesktop.Notifications` inhibit |
| SYS-04 | Quit all applications | planned | P2 | `NSRunningApplication.terminate` | compositor IPC / EWMH `_NET_CLOSE_WINDOW` |

## Emoji, symbols and characters

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| EMO-01 | Emoji search by CLDR keywords and shortcodes, copy | done | P0 | shared | same |
| EMO-02 | Paste directly, skin tones, recently used first | partial | P1 | paste via CLP-03; frecency done | same |
| EMO-03 | Unicode symbol and character search, kaomoji | planned | P2 | Unicode names table, shared | same |

## Script commands (Raycast compatible)

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| SCR-01 | `@raycast.*` metadata: `schemaVersion`, `title`, `mode`, `packageName` (folder-name default), `icon`, `iconDark`, `description`, `author`, `authorURL`, `platform`, `currentDirectoryPath` (`~` expanded), `needsConfirmation`, `refreshTime` (10 s floor), `argument1..3` | done | P0 | shared parser, Raycast's own scripts as goldens | same |
| SCR-02 | Output modes `fullOutput`, `compact`, `silent`, `inline` (inline without `refreshTime` runs compact, as Raycast) | done | P0 | CLI prints per mode; UI shows view / toast / HUD / subtitle | same |
| SCR-03 | Arguments: `text`, `password`, `dropdown`, `optional`, `percentEncoded` | done | P0 | CLI positional values; UI fields in the search bar (LCH-12) | same |
| SCR-04 | `quickhop run <script> [args]` by path, file stem or title | done | P0 | shared | same |
| SCR-05 | Script directories: `~/.config/quickhop/scripts` plus `[scripts] dirs`; `*.template.*` skipped | done | P0 | XDG on macOS too | XDG |
| SCR-06 | Inline refresh scheduler (`refreshTime`, first 10 refreshed automatically) | planned | P1 | daemon timer | same |
| SCR-07 | ANSI colours in fullOutput and inline output | partial | P1 | colour kept in fullOutput/inline, stripped from toasts; UI renders SGR 30-37/40-47/90-107, 8- and 24-bit | same |
| SCR-08 | Create Script Command from templates (bash, python, node, ruby, swift, applescript) | planned | P1 | writes into the scripts dir | same, applescript/swift templates hidden |
| SCR-09 | Platform awareness: hide or warn for macOS-only scripts (`osascript`, `.applescript`, `platform macos`) on Linux | planned | P1 | n/a | `platform` header and interpreter check |

## Extensions and store

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| EXT-01 | Extension API: out-of-process extensions speaking JSON-RPC over stdio, manifest `extension.toml`, views List/Detail/Form/Grid, actions, preferences, local storage, toasts | planned | P1 | any language; quickhop spawns and supervises | same |
| EXT-02 | Run Raycast TypeScript extensions through an `@raycast/api` compatibility shim on Node/Bun | planned | P2 | shim maps React tree to EXT-01 views; macOS-only APIs (AppleScript, `getSelectedFinderItems`) best effort | same, macOS-only APIs unavailable |
| EXT-03 | Store: browse, install, update and remove extensions from a git index | planned | P2 | `~/.local/share/quickhop/extensions` | same |
| EXT-04 | Per-extension and per-command preferences UI, secrets in the OS keyring | planned | P1 | Keychain | Secret Service |
| EXT-05 | OAuth PKCE helper for extensions | planned | P2 | `quickhop://oauth` deeplink (LCH-13) | same |
| EXT-06 | Menu bar commands | planned | P2 | `NSStatusItem` (platform crate) | `StatusNotifierItem` over D-Bus (KDE, GNOME with AppIndicator extension, waybar) |
| EXT-07 | Background refresh of commands on an interval | planned | P2 | daemon scheduler | same |

## AI

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| AI-01 | Quick AI: ask from the root search (`Tab`), streamed answer | planned | P1 | provider-neutral client (OpenAI-compatible, Anthropic, Ollama); keys in Keychain | same; keys in Secret Service |
| AI-02 | AI Chat window with history | planned | P1 | store table `chats` | same |
| AI-03 | AI Commands: prompt templates with `{selection}`, `{clipboard}`, `{argument}` | planned | P1 | selected text via AX `kAXSelectedTextAttribute`, or `⌘C` round trip | X11 `PRIMARY` selection; Wayland `primary-selection` via data-control, else clipboard |
| AI-04 | AI in extensions and tool use | planned | P2 | EXT-01 capability | same |

## Notes, calendar, focus, translate and other built-ins

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| NTE-01 | Floating notes: always-on-top markdown notes, several notes, search | planned | P2 | gpui window level floating | X11 `_NET_WM_STATE_ABOVE`; Wayland layer-shell `top` layer or normal window |
| CLD-01 | My schedule: upcoming events, join meeting links | planned | P2 | EventKit (platform crate, Calendar permission) | Evolution Data Server over D-Bus (GNOME), Akonadi (KDE), or CalDAV/ICS URLs from config |
| CLD-02 | Next meeting in the menu bar | planned | P2 | EXT-06 | EXT-06 |
| FOC-01 | Focus sessions: block chosen apps and sites for a duration | planned | P2 | hide/terminate via `NSRunningApplication`; sites via `/etc/hosts` helper (admin prompt) or browser extension | kill/minimize via compositor IPC; sites as macOS |
| TRN-01 | Translate selected or typed text | planned | P2 | provider: DeepL / LibreTranslate / AI-01 | same |
| CLR-01 | Color picker and color format conversion | planned | P2 | `NSColorSampler` (platform crate) | `Screenshot` portal `PickColor`; X11 `XGetImage` |
| MNU-01 | Search menu items of the frontmost app | planned | P2 | AX menu bar tree | D-Bus `com.canonical.dbusmenu` where exported (KDE, Unity-style); else unsupported |
| DIC-01 | Define a word | planned | P2 | Dictionary Services (`DCSCopyTextDefinition`) | `dictd`/`dict` or an online API |
| HYP-01 | Hyper key (Caps Lock as `⌃⌥⇧⌘`) | planned | P2 | `hidutil` key remap + event tap | `keyd` / XKB `caps:hyper` option guidance |

## Settings, themes, sync and platform

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| CFG-01 | `~/.config/quickhop/config.toml` (honours `XDG_CONFIG_HOME`), validated, `config default/path/check` | done | P0 | XDG on macOS too | XDG |
| CFG-02 | One-time move from the 0.1 macOS location `~/Library/Application Support/quickhop` | done | P0 | config and frecency database | n/a |
| CFG-03 | `quickhop doctor`: paths, session, helper tools, providers | done | P0 | shared | adds X11/Wayland session |
| CFG-04 | Live reload when config, scripts or snippets change | planned | P1 | FSEvents (`notify`) | inotify (`notify`) |
| CFG-05 | Settings window that writes `config.toml` (comments preserved with `toml_edit`) | planned | P1 | UI | same |
| CFG-06 | Themes: light, dark, follow system; custom theme TOML in `~/.config/quickhop/themes/` | planned | P1 | `NSApp.effectiveAppearance` | `Settings` portal `color-scheme` (GNOME, KDE) |
| CFG-07 | Launch at login | planned | P0 | `SMAppService.mainApp` (macOS 13+) or a LaunchAgent plist | XDG autostart `~/.config/autostart/quickhop.desktop`, or a systemd user unit |
| CFG-08 | Settings sync between machines | planned | P2 | plain files: the config dir is git/Syncthing friendly; `quickhop export/import` bundles config, snippets, quicklinks, encrypted with a passphrase (`age`) | same |
| CFG-09 | Import a Raycast export (`.rayconfig`) | planned | P2 | decrypt with the user's export password, map snippets/quicklinks/aliases/hotkeys | same |
| PLT-01 | Background daemon, single instance, IPC (`quickhop toggle`, `quickhop open <id>`) | planned | P0 | Unix socket in `$TMPDIR/quickhop-<uid>` | Unix socket in `$XDG_RUNTIME_DIR/quickhop.sock` |
| PLT-02 | Permission onboarding (Accessibility, Input Monitoring, Screen Recording, Calendar) | planned | P1 | `AXIsProcessTrustedWithOptions`, deep links to System Settings panes | portal permission prompts; `input` group guidance for SNP-03 |
| PLT-03 | Packages and install | partial | P1 | `cargo install` today; signed and notarized `.app` in a DMG, Homebrew cask | `cargo install` today; `.deb`, `.rpm`, AUR `PKGBUILD`, tarball with `.desktop` + icons; Flatpak later (host app launching needs `flatpak-spawn --host`) |
| PLT-04 | Updates | planned | P2 | update check against GitHub releases, cask upgrade | package managers |
| PLT-05 | No telemetry, works offline (only currency rates, AI and store use the network, each opt-in) | done | P0 | principle | same |

## Totals

| Priority | Features | done | partial | planned |
|----------|---------:|-----:|--------:|--------:|
| P0 | 30 | 17 | 2 | 11 |
| P1 | 36 | 0 | 5 | 31 |
| P2 | 29 | 0 | 0 | 29 |
| All | 95 | 17 | 7 | 71 |

Counted from the tables above; recount when editing them.

## Gap summary

Built (from the CLI): the fuzzy and frecency ranking core, the config, applications on
macOS and Linux, calculator and units, emoji, system power commands, quicklinks, and Raycast
script commands end to end (`quickhop run`, all metadata, modes and arguments).

Missing for a first usable release (the P0 gaps): the launcher window and its global hotkey
(LCH-01, LCH-07, LCH-10, LCH-11), the daemon and IPC (PLT-01), per-command aliases (LCH-04),
file search (FIL-01), clipboard history with paste (CLP-01, CLP-03), snippets with placeholders
(SNP-01, SNP-02), application icon rendering (APP-02) and launch at login (CFG-07).
[plan.md](plan.md) orders these into slices.
