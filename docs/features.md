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
| LCH-01 | Global hotkey opens/closes the launcher (default `Alt+Shift+Space`, free of Raycast's `Alt+Space`) | partial | P0 | built: `RegisterEventHotKey` (Carbon) in `quickhop-platform-macos::hotkey`, no permission needed; `quickhop daemon` registers `hotkey` from `config.toml` (parsed and checked by `quickhop_core::hotkey`, key codes from the current keyboard layout), `daemon status` says whether it holds it | X11: `XGrabKey`. Wayland: `GlobalShortcuts` portal (KDE 5.27+, GNOME 48+, Hyprland); else a compositor binding to `quickhop toggle` (sway `bindsym`, Hyprland `bind`, GNOME custom shortcut via `gsettings`), set up by `quickhop doctor --fix` |
| LCH-02 | Root search: fuzzy match over every provider | done | P0 | nucleo matcher in `quickhop-core`, shared | same |
| LCH-03 | Frecency ranking per query prefix (7-day half-life) | done | P0 | SQLite store | same |
| LCH-04 | Aliases for any command (`g rust`, `:smile`) | done | P0 | quicklinks and emoji built in; `[[commands]] alias` for every item | same |
| LCH-05 | Hotkey per command (open an app or run a script without the launcher) | planned | P1 | `global-hotkey` registrations from `[[commands]] hotkey` | X11 grabs; Wayland portal shortcuts (one per command) or compositor bindings to `quickhop run`/`quickhop open <id>` |
| LCH-06 | Favorites pinned at the top of the empty root | planned | P1 | store table `favorites` | same |
| LCH-07 | Action panel (`⌘K` / `Ctrl+K`) with every action of the selected item | partial | P0 | built: `⌘K` lists every action of the selected item, typing filters them, `Return` runs one, `Esc` closes the panel | `Ctrl+K`, with the Linux window |
| LCH-08 | Fallback commands when nothing matches (web search, file search, AI) | planned | P1 | config `fallbacks = [...]` of command ids, ranked after results | same |
| LCH-09 | Enable, disable and reorder root commands | done | P1 | `[providers]` switches; `[[commands]] enabled` and `order` (empty root) | same |
| LCH-10 | Navigation stack: commands push views, `Esc` pops, `Esc` at root hides | partial | P0 | built: the view model's stack (push, pop, `Esc` pops, `Esc` at root hides) and the window over it; next: the views commands push (clipboard, snippets, files) | with the Linux window |
| LCH-11 | Launcher window: borderless, rounded, translucent/blurred, on the active screen, hides on focus loss | partial | P0 | built: one gpui `WindowKind::PopUp` window (non-activating, above other apps) with a `Blurred` background, created hidden when the daemon starts, shown and hidden from then on, hidden when it loses the focus; next: the screen of the mouse instead of the main screen | X11: override-redirect-free centered dialog, `_NET_WM_STATE_ABOVE`, ARGB visual; Wayland: `wlr-layer-shell` overlay (sway, Hyprland, KWin, COSMIC) with KDE blur protocol when offered; GNOME (no layer-shell): normal centered undecorated window |
| LCH-12 | Command arguments inline in the search bar (`alias` + space focuses them) | partial | P1 | built: a space after the alias of a script that asks for arguments opens its fields in the search bar, `Tab`/`Shift+Tab` move between them, arrows pick dropdown choices, passwords are masked, `Return` runs it or focuses the first empty required field, `Esc` closes the fields; `Return` on such a script opens them too | same, with the Linux window |
| LCH-13 | Deeplinks: `quickhop://commands/<id>?arguments=...` | planned | P1 | `CFBundleURLTypes` in Info.plist | `x-scheme-handler/quickhop` in the `.desktop` file, `xdg-mime` |
| LCH-14 | Compact window mode and adjustable size/position | planned | P2 | config `[window]` | same |
| LCH-15 | Root search history | planned | P2 | store | same |

## Applications and files

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| APP-01 | Search and open installed applications | done | P0 | `.app` bundles in `/Applications`, `/System/Applications`, `~/Applications` (Info.plist), opened with `open`; also shown in Finder or their path copied | XDG desktop entries (`$XDG_DATA_DIRS/applications`, precedence, `TryExec`, `OnlyShowIn`, field codes), run directly; Flatpak and Snap exports are desktop entries too |
| APP-02 | Application icons | partial | P0 | built: any app's or file's icon rendered to PNG by `NSWorkspace iconForFile` in `quickhop-platform-macos` (asset catalogs included), drawn in the window's rows (loaded off the main thread, cached); a script's emoji icon drawn as text | Icon Theme spec lookup done (theme, `Inherits`, `hicolor`, pixmaps); SVG rendering with `resvg` in the UI |
| APP-03 | Running apps: quit, hide, force quit, show in Finder | planned | P1 | `NSRunningApplication` (platform crate) | `/proc` scan, `kill`; window-level actions via WIN mechanisms |
| APP-04 | Switch windows (search open windows, focus one) | planned | P1 | AX window list + `CGWindowListCopyWindowInfo` (Screen Recording permission for titles on macOS 10.15+) | X11: EWMH `_NET_CLIENT_LIST`, `_NET_ACTIVE_WINDOW`. Wayland: `wlr-foreign-toplevel-management` / `ext-foreign-toplevel-list` (sway, Hyprland, COSMIC), KWin scripting over D-Bus, GNOME Shell extension (no protocol) |
| APP-05 | Watch application folders and refresh without restart | planned | P1 | FSEvents via `notify` | inotify via `notify` on every `applications` dir |
| FIL-01 | File and folder search by name | partial | P0 | built: `files` provider over Spotlight (`mdfind -onlyin ~ -name`), queries of 3+ characters, 200 paths at most, killed after 2 s or on cancel, or over quickhop's index once `quickhop index` built one; in the window, the Search Files command (`views:files`) lists them with their actions; `NSMetadataQuery` later | quickhop's own index, built: `quickhop index [--watch]` (`quickhop-index`: parallel walk, `notify` watcher, gitignore-aware, SQLite FTS5 trigram), the same `files` provider through its `FileSource` trait; `plocate` or GNOME `localsearch`/KDE `baloo` results when present (later) |
| FIL-02 | File actions: open, open with, reveal, copy path, Quick Look, move to trash | partial | P1 | built: open, reveal (`open -R`), copy path, Quick Look (`qlmanage -p`), move to trash (confirmed first); next: open with | built: `xdg-open`, `org.freedesktop.FileManager1.ShowItems` (D-Bus, parent folder as fallback), the home trash per the FreeDesktop.org spec (`gio trash` across filesystems); later `gio launch`, `sushi`/`gnome-sushi`. `quickhop launch <id> --action reveal\|copy\|trash` |
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
| CLP-01 | Text clipboard history, searchable, kept in SQLite | partial | P0 | built: `quickhop clipboard watch\|add\|list\|clear` and the daemon poll `NSPasteboard.changeCount` every 250 ms through `quickhop_core::Watcher`; history deduplicated by SHA-256 in `state.sqlite` with `[clipboard]` retention; the Clipboard History command (`views:clipboard`) lists it in the window, newest first until something is typed, searched over the whole text, with the source app as subtitle | built: `quickhop clipboard watch\|add\|list\|clear`, history in `state.sqlite` with `[clipboard]` retention. X11: XFixes `SelectionNotify` on `CLIPBOARD`. Wayland: `wl-paste --watch` on compositors with `wlr-data-control` / `ext-data-control-v1` (sway, Hyprland, KWin, COSMIC); native `wayland-client` later; GNOME has no data-control protocol: watch only while quickhop has focus, or the optional GNOME Shell extension. The searchable view comes with the launcher window |
| CLP-02 | Images, files and rich text in history | planned | P1 | pasteboard types `public.png`, `public.file-url`, `public.rtf` | MIME targets `image/png`, `text/uri-list`, `text/html` |
| CLP-03 | Paste the chosen entry into the frontmost app | partial | P0 | built: `quickhop clipboard paste [n]`, `launch`/`open --paste` through `quickhop_core::paste::paste_text` (copy, then `⌘V` through `CGEventPost` when Accessibility is granted, else copy only with the reason); `Return` on a clipboard entry hides the window, waits for the previous app to take the focus back, then pastes (`Paste` action; copy-only shows the reason in a HUD); next: run the **[ax]** test that pastes into a text window (written) on an unlocked Mac | built: `quickhop clipboard paste [n]`, `launch`/`open --paste`; sets the clipboard (`xclip` / `wl-copy`), then X11: XTest `Ctrl+V` (done). Wayland: copy only and say so, until `virtual-keyboard-unstable-v1` (wlroots; `wtype` did not paste into GTK in headless sway 1.7) and the `RemoteDesktop` portal keyboard (GNOME, KDE) |
| CLP-04 | Ignore secrets and chosen apps; pin entries; delete | partial | P1 | built: copies marked `org.nspasteboard.ConcealedType`/`TransientType` are never recorded, the frontmost app (`NSWorkspace`) is stored as the source; next: ignored apps, pinning, deleting from the view | built: copies offering `x-kde-passwordManagerHint` are never recorded (X11: asked for `TARGETS` before the text; Wayland: `wl-paste`'s `CLIPBOARD_STATE=sensitive`, or `wl-paste --list-types` before wl-clipboard 2.2); next: X11 `_NET_ACTIVE_WINDOW` class as the source, ignored apps, pinning, deleting from the view |
| CLP-05 | Paste as plain text | planned | P1 | write `public.utf8-plain-text` only | offer `text/plain;charset=utf-8` only |
| CLP-06 | Retention limits and encryption at rest | planned | P2 | key in Keychain (Security framework) | key in Secret Service (`org.freedesktop.secrets`) via `oo7`/`secret-service` |

## Snippets

| ID | Feature | Status | P | macOS | Linux (X11 / Wayland) |
|----|---------|--------|---|-------|------------------------|
| SNP-01 | Snippet library: create, search, copy or paste | partial | P0 | `snippets.toml` in the config dir, `snippets` provider (name, keyword), `quickhop launch snippets:<name>` expands and prints, `--paste` pastes (CLP-03); in the window, the Search Snippets command (`views:snippets`) lists the library and `Return` copies the expanded snippet; next: paste from the window, create and edit | same |
| SNP-02 | Dynamic placeholders: `{clipboard}`, `{date}`/`{time}`/`{datetime}` with formats and offsets, `{cursor}`, `{uuid}`, `{argument name="..." default="..."}`, `{snippet name="..."}` | done | P0 | pure expansion engine in core (`placeholders`, LDML `datefmt`), plus `{day}`, `{selection}`, `options`, `{clipboard offset=N}` and the modifiers; Raycast manual goldens | same |
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
| SCR-02 | Output modes `fullOutput`, `compact`, `silent`, `inline` (inline without `refreshTime` runs compact, as Raycast) | done | P0 | CLI prints per mode; macOS window: fullOutput opens an output page (`Esc` goes back), compact shows a toast and keeps the window, silent hides it and shows a HUD over the app in front, inline updates the subtitle | same, with the Linux window |
| SCR-03 | Arguments: `text`, `password`, `dropdown`, `optional`, `percentEncoded` | done | P0 | CLI positional values; macOS window fields in the search bar (LCH-12) | same |
| SCR-04 | `quickhop run <script> [args]` by path, file stem or title | done | P0 | shared | same |
| SCR-05 | Script directories: `~/.config/quickhop/scripts` plus `[scripts] dirs`; `*.template.*` skipped | done | P0 | XDG on macOS too | XDG |
| SCR-06 | Inline refresh scheduler (`refreshTime`, first 10 refreshed automatically) | partial | P1 | built: the daemon runs the first 10 inline scripts that take no arguments again every `refreshTime` and the window searches again with their new subtitles, keeping the selection | same, with the Linux window |
| SCR-07 | ANSI colours in fullOutput and inline output | partial | P1 | colour kept in fullOutput/inline, stripped from toasts; the output page draws foreground colours (SGR 30-37, 90-97, `38;5;n` below 16) in a 16-colour palette; next: 256 and 24-bit colours, backgrounds, colour in inline subtitles | same |
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
| AI-01 | Quick AI: ask from the root search (`Tab`), streamed answer | planned | P1 | provider-neutral client: local Ollama by default, the user's own OpenAI, Anthropic or OpenAI-compatible keys ([ADR 0002](adr/0002-licence-extensions-ai.md)); keys in Keychain | same; keys in Secret Service |
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
| CFG-04 | Live reload when config, scripts or snippets change | partial | P1 | built: the daemon watches `config.toml` and `snippets.toml` (`notify`, FSEvents) and reloads on save, an invalid file refused as by `quickhop reload`; next: script folders | same (inotify) |
| CFG-05 | Settings window that writes `config.toml` (comments preserved with `toml_edit`) | partial | P1 | built: quickhop Settings in the search (`settings`, `preferences`, `config`), the menu-bar icon's menu and `quickhop settings` open a settings window: hotkey recorder, launch at login, menu-bar icon, theme, results per search, each provider, clipboard history, hidden files, open config file; each change is written at once keeping comments and order, an invalid result refused. Also in the search: Reload quickhop config, About quickhop, Quit quickhop. The menu-bar icon (`NSStatusItem`, template image, `menu_bar_icon`) offers Open quickhop, Settings…, Reload config, Quit | `quickhop settings` and the tray icon (`StatusNotifierItem`, `ksni`) open `config.toml` in the default editor until the Linux window lands |
| CFG-06 | Themes: light, dark, follow system; custom theme TOML in `~/.config/quickhop/themes/` | partial | P1 | built: `theme = "system" \| "dark" \| "light"` for every window, following `NSApp.effectiveAppearance`; next: custom theme files | `Settings` portal `color-scheme` (GNOME, KDE) with the Linux window |
| CFG-07 | Launch at login | partial | P0 | built: `quickhop autostart on\|off\|status` writes a LaunchAgent plist (`io.github.yuyudhan.quickhop`, `RunAtLoad`; `….dev` for a development build) that runs quickhop.app when installed, and `autostart on` starts the daemon at once (`launchctl bootstrap`), replacing one from another program or version; `SMAppService.mainApp` with the `.app` | done: `quickhop autostart on\|off\|status` writes `~/.config/autostart/quickhop.desktop` and starts the daemon at once; on sway, i3, Hyprland and river it prints the compositor's exec line instead; packages also ship a systemd user unit |
| CFG-08 | Settings sync between machines | planned | P2 | plain files: the config dir is git/Syncthing friendly; `quickhop export/import` bundles config, snippets, quicklinks, encrypted with a passphrase (`age`) | same |
| CFG-09 | Import a Raycast export (`.rayconfig`) | planned | P2 | decrypt with the user's export password, map snippets/quicklinks/aliases/hotkeys | same |
| PLT-01 | Background daemon, single instance, IPC (`quickhop toggle`, `quickhop open <id>`) | partial | P0 | built (Unix): `quickhop daemon` with lock file and socket in `$TMPDIR/quickhop-$USER/`, `daemon status\|stop`, `open <id>` (starts a daemon when none runs), `reload`; on macOS the daemon owns the launcher window: `toggle`, `show` (both start a daemon when none runs) and `hide`, `daemon --no-window`, and `daemon --test-socket <path>` answering `snapshot` with what the window shows | same, in `$XDG_RUNTIME_DIR/quickhop/`; the daemon also watches the clipboard and keeps the file index current; `toggle` with the Linux window |
| PLT-02 | Permission onboarding (Accessibility, Input Monitoring, Screen Recording, Calendar) | partial | P1 | built: the onboarding window for Accessibility, which pasting needs: opened by quickhop.app while the permission is missing and by `quickhop onboarding`, it shows the status, its button puts quickhop in the list (`AXIsProcessTrustedWithOptions`) and opens the Accessibility pane of System Settings, and it asks again (`AXIsProcessTrusted`) whenever quickhop comes back to the front; next: the other permissions | portal permission prompts; `input` group guidance for SNP-03 |
| PLT-03 | Packages and install | partial | P1 | built: `quickhop.app` from `just bundle-macos-release` (agent app, `LSUIElement`, icon rendered from `packaging/macos/quickhop.svg`, ad-hoc signed; opened, it runs the daemon with the window and the hotkey) and quickhop Dev.app from `just bundle-macos`; the release workflow (`.github/workflows/release.yml`, on a `vX.Y.Z` tag) builds a universal binary (arm64 + x86_64, `lipo`), which `scripts/dist.sh --app` packs with the app into the macOS archive and `install.sh` installs into `/Applications`; next: the first release, a DMG, a Developer ID signature and notarization (wired, waiting for a certificate) | built: x86_64 and aarch64 release archives and `.deb`s built in `debian:bullseye` (glibc 2.31 floor, enforced), with `.desktop`, icon and systemd user unit (`just bundle-linux`, checked in a clean Debian container by `just bundle-linux-test`); later `.rpm`, AUR `PKGBUILD`; Flatpak later (host app launching needs `flatpak-spawn --host`) |
| PLT-04 | Updates | planned | P2 | update check against GitHub releases, cask upgrade | package managers |
| PLT-05 | No telemetry, works offline (only currency rates, AI and store use the network, each opt-in) | done | P0 | principle | same |

## Totals

| Priority | Features | done | partial | planned |
|----------|---------:|-----:|--------:|--------:|
| P0 | 30 | 19 | 11 | 0 |
| P1 | 36 | 1 | 12 | 23 |
| P2 | 29 | 0 | 0 | 29 |
| All | 95 | 20 | 23 | 52 |

Counted from the tables above; recount when editing them.

## Gap summary

Built (from the CLI): the fuzzy and frecency ranking core, the config with per-command
aliases, switches and root order, applications on macOS and Linux, calculator and units, emoji,
system power commands, quicklinks, Raycast script commands end to end (`quickhop run`, all
metadata, modes and arguments), and snippets with Raycast's dynamic placeholders.

Missing for a first usable release (the P0 gaps): the Linux launcher window and hotkey, and on
both systems the views commands push (LCH-01, LCH-07, LCH-10, LCH-11, PLT-01); icons in the
window's rows (APP-02); the clipboard view and paste on Wayland (CLP-01, CLP-03); pasting
snippets from the window (SNP-01) and launch at login through the `.app` on macOS (CFG-07).
[plan.md](plan.md) orders these into slices.
