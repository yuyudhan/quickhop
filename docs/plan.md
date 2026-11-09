# Build plan

The path from today's CLI to Raycast parity, in slices. Each slice is one reviewable branch of
work that leaves `just check` green on both OSes, names the [features](features.md) it
delivers, the machine it is built and tested on, and the tests that accept it.

Machines: **m4** is the MacBook M4 (all macOS work and macOS UI tests); **lx3** is itlinux3
(all Linux work, X11 and Wayland UI tests). Pure-Rust slices are written on either and must
pass `just check` on both before they count as done.

## How the UI is tested

Unit and property tests cover the toolkit-free parts (the view model, placeholders, parsers,
ranking). The window itself is tested for real, on real display servers, by one harness with
two drivers, in slice U1:

- **Test hook.** `quickhop daemon --test-socket <path>` exposes a JSON state snapshot (window
  visible, query text, result ids, selection, view stack, last toast) and nothing else. Tests
  drive the app through real OS input and assert on the snapshot plus screenshots, so they do
  not depend on pixel-exact rendering or on accessibility support in gpui.
- **macOS (m4): an AX-driven harness.** A Rust integration-test crate, `quickhop-uitest`,
  launches the built `quickhop.app`, posts real key events with `CGEventPost` (the hotkey,
  typing, arrows, `⌘K`, `Esc`), finds the window and frontmost app through the Accessibility
  API, captures it with `screencapture -l <windowid>`, and checks the snapshot. The terminal
  running the tests needs Accessibility and Screen Recording permission once. XCUITest is not
  used: it needs an Xcode project wrapper for a non-Xcode app and gives nothing the AX harness
  does not.
- **Linux X11 (lx3): Xvfb.** `xvfb-run -s "-screen 0 1920x1080x24"` with a minimal window
  manager (openbox) for EWMH, input with `xdotool`, screenshots with `import -window`.
- **Linux Wayland (lx3): headless compositors.** sway with `WLR_BACKENDS=headless
  WLR_RENDERER=pixman` (layer-shell, data-control, virtual-keyboard, foreign-toplevel), input
  with `wtype`, screenshots with `grim`; and `mutter --headless --wayland --virtual-monitor
  1920x1080` for the GNOME path (no layer-shell, portals via `xdg-desktop-portal-gnome`).
  Vulkan comes from Mesa lavapipe, so no GPU is needed.
- **CI.** The Linux UI jobs run the same harness on `ubuntu-latest` (Xvfb, sway headless).
  macOS UI tests run on m4 before merge (hosted runners cannot be granted Accessibility
  permission non-interactively); `just ui-test` runs whichever drivers the host supports.

Every UI acceptance test below names its driver: **[ax]**, **[x11]**, **[sway]**, **[gnome]**.

## Phase A: first usable launcher (the P0 gaps, plus FIL-02 with file search)

### U0. UI toolkit spike (m4, lx3)

- Features: decision gate for [ADR 0001](adr/0001-ui-toolkit.md).
- Work: a throwaway gpui window with a text field over 5,000 fake items using
  `quickhop-core` ranking; blurred background on macOS; layer-shell on sway; plain window on
  GNOME and X11.
- Accept: show-to-first-frame under 50 ms from an IPC `toggle` (median of 50, m4 and lx3);
  typing under 8 ms per frame at 5,000 items; blur visible on m4 (screenshot differs from the
  same window with `Opaque`); the window is a layer-shell surface on sway (`swaymsg -t
  get_tree` shows no toplevel). Result recorded in ADR 0001 (or ADR 0002: iced).

### U1. Daemon, IPC and the UI test harness (lx3, then m4)

- Features: PLT-01.
- Work: `quickhop daemon` (single instance via lock file, Unix socket), `quickhop toggle`,
  `open <id>`, `reload`; the `--test-socket` snapshot; `quickhop-uitest` with the ax, x11,
  sway and gnome drivers; `just ui-test`; a Linux UI job in CI.
- Accept: a second `quickhop daemon` exits with "already running"; `toggle` with no daemon
  starts one; killing the daemon removes the socket on next start; harness smoke test opens and
  closes an empty window under **[ax] [x11] [sway] [gnome]**.

### U2. Launcher window and root search (m4 for macOS, lx3 for Linux)

- Features: LCH-02 (UI), LCH-07, LCH-10, LCH-11, APP-01 (UI), APP-02.
- Work: `quickhop-ui::model` (query, results, selection, navigation stack, action panel) with
  unit tests; gpui views: search field, virtualized result list with match highlighting,
  icons (icns via the platform crate, PNG/SVG via `resvg`), action panel, toasts; hide on
  focus loss; window on the active screen.
- Accept: model unit tests for selection wrap, stack push/pop, action panel filtering. UI:
  type "fire", the first row is Firefox, `Enter` opens it (the test checks the spawned
  process), `⌘K`/`Ctrl+K` lists its actions, `Esc` closes the panel then the window, under
  **[ax] [x11] [sway] [gnome]**; result icons render (non-blank icon region in the screenshot).

### U3. Global hotkey (m4, lx3)

- Features: LCH-01.
- Work: `global-hotkey` on macOS and X11; GlobalShortcuts portal via `ashpd` on Wayland;
  where neither works, `quickhop doctor --fix` writes a compositor binding (sway, Hyprland,
  GNOME custom shortcut) to `quickhop toggle`; hotkey from `config.toml`, reloaded live.
- Accept: pressing the configured hotkey shows the window and pressing it again hides it under
  **[ax] [x11] [sway]**; under **[gnome]** the portal path binds and fires (portal dialog
  auto-accepted by the test session); `doctor` reports which path is active.

### U4. Launch at login and first packages (m4, lx3)

- Features: CFG-07, PLT-03 (first cut).
- Work: `quickhop autostart on|off` (SMAppService or LaunchAgent; XDG autostart entry or
  systemd user unit); an unsigned `.app` bundle and DMG from `just bundle-macos`; `.deb` and
  tarball from `just bundle-linux` with desktop file and icons.
- Accept: after `autostart on` and a re-login of a test user (lx3: `systemctl --user
  start` in a fresh user session; m4: `launchctl print gui/$UID/...` shows the job) the daemon
  is running; the `.deb` installs in a clean Debian container and `quickhop doctor` passes;
  the `.app` launches from Finder and shows the window **[ax]**.

### U5. Aliases and per-command settings (lx3)

- Features: LCH-04, LCH-09.
- Work: `[[commands]]` table: `id`, `alias`, `enabled`, `hotkey` (used by LCH-05); aliases
  for every provider's items; root order overrides.
- Accept: core tests: an alias pins any item, a disabled command never appears, unknown ids
  are config errors; CLI e2e via `quickhop query`.

### U6. Script commands in the launcher (m4, lx3)

- Features: SCR-02 and SCR-03 in the UI, LCH-12, SCR-06, SCR-07.
- Work: argument fields in the search bar (alias + space focuses the first), password
  masking, dropdowns; fullOutput view with ANSI rendering; compact toast; silent HUD after
  hide; inline subtitles refreshed by the daemon (first 10 automatically, `refreshTime`).
- Accept: the fixture scripts in every mode under **[ax] [sway]**: `search` takes two
  arguments from the bar and its compact toast reads "searching rust on python"; `hello` opens
  the full-output view; `inline` updates its subtitle after its refresh interval (clock faked
  through the test socket); ANSI fixture shows red text (pixel sample).

### C1. Clipboard history (m4, lx3)

- Features: CLP-01.
- Work: watcher per platform (NSPasteboard change count; XFixes; ext/wlr data-control;
  GNOME focused-only fallback); `state.sqlite` with migrations; clipboard view with search
  and preview; retention setting.
- Accept: store property tests (dedupe by hash, retention cut, order); copying three strings
  with `pbcopy` / `xclip` / `wl-copy` in the test session lists them newest first **[ax] [x11]
  [sway]**; on **[gnome]** the documented limitation holds (entries copied while focused only).

### C2. Paste into the frontmost app and permissions (m4, lx3)

- Features: CLP-03, PLT-02.
- Work: `Paster` trait: CGEvent `⌘V`; XTest; virtual-keyboard; RemoteDesktop portal; copy-only
  fallback with a toast; onboarding view that checks and requests Accessibility.
- Accept: with a text editor focused (TextEdit on m4, gedit/foot on lx3), choosing a history
  entry inserts it **[ax] [x11] [sway] [gnome]** (read back via AX or the editor's file);
  without Accessibility on macOS the onboarding view appears.

### S1. Snippets and placeholders (lx3, UI on m4)

- Features: SNP-01, SNP-02.
- Work: `snippets.toml` schema, placeholder engine in core (`{clipboard}`, date/time with
  formats and offsets, `{cursor}`, `{uuid}`, `{argument}`, nested `{snippet}`), snippet view;
  paste via C2.
- Accept: placeholder golden tests from Raycast's documented examples; proptest that
  expansion never panics and leaves unknown placeholders verbatim; pasting a snippet with
  `{date format="yyyy-MM-dd"}` inserts today's date **[ax] [sway]**.

### F1. File and folder search (lx3 index, m4 Spotlight)

- Features: FIL-01, FIL-02.
- Work: `quickhop-index` (walker, watcher, FTS5 trigram, ranking through `fuzzy`); Spotlight
  source on macOS; `files` provider with a minimum query length.
- Accept: index property tests (every created file is found after the watcher settles, deleted
  ones disappear); a 200k-file fixture tree is indexed under 30 s and answers in under 20 ms
  on lx3; on m4 `mdfind`-backed results include a freshly created file; UI search for a file
  name opens it **[ax] [x11]**; reveal, copy path and move to trash act on a fixture file
  (checked on disk and, for reveal, via the file manager's D-Bus call / Finder's AX window).

## Phase B: Raycast parity (P1)

### K1. Calculator: currency and dates (lx3)

- Features: CAL-03, CAL-04.
- Accept: golden tests with a fixed rates file (`100 usd in eur`), offline uses cached rates
  and says their date; `time in tokyo`, `days until dec 25`, `unix 1700000000` with a fixed
  clock.

### W1. Window management on macOS (m4)

- Features: WIN-01, WIN-02 (macOS).
- Accept: **[ax]** opens a TextEdit window, runs each layout command, and reads the AX frame:
  left half equals half of `visibleFrame` (±1 px) for every layout; next-display on a second
  virtual display when present, else skipped with a reason.

### W2. Window management on Linux (lx3)

- Features: WIN-01, WIN-02 (Linux).
- Accept: **[x11]** with openbox: `xdotool getwindowgeometry` matches each layout; **[sway]**:
  `swaymsg -t get_tree` geometry matches; Hyprland and KWin paths unit-tested against
  recorded IPC; GNOME reports "needs the quickhop GNOME extension".

### A1. Running apps and window switcher (m4, lx3)

- Features: APP-03, APP-04, APP-05.
- Accept: **[ax]** quit/hide/focus TextEdit through the launcher; **[x11] [sway]** switch to a
  second test window and see it focused (EWMH active window, sway focus); installing a desktop
  entry or `.app` while the daemon runs makes it searchable within 2 s.

### Y1. More system commands (m4, lx3)

- Features: SYS-02.
- Accept: each command produces the expected platform call in dry-run (snapshot of the
  `Action`); trash and dark-mode commands verified for real in the test sessions **[ax]
  [gnome]**.

### L1. Launcher conveniences (lx3, UI on m4)

- Features: LCH-05, LCH-06, LCH-08, LCH-13, QLK-02, EMO-02.
- Accept: a per-command hotkey opens that command directly **[ax] [x11]**; favorites appear
  first in the empty root; with no match the fallbacks list appears; `xdg-open
  quickhop://commands/scripts/hello.sh` / `open quickhop://...` runs it; quicklink
  `{clipboard}` and `{selection}` expand (selection via AX / PRIMARY); emoji paste uses C2.

### X1. Extension host and API (lx3, UI on m4)

- Features: EXT-01, EXT-04.
- Work: `quickhop-ext`, `extension.toml`, JSON-RPC protocol spec in `docs/extensions.md`, a
  reference extension in Rust and one in Python, preferences UI, keyring-backed secrets.
- Accept: protocol conformance tests with a scripted fake extension (every message type,
  malformed JSON, crash mid-view, 5 s timeout); the two reference extensions render a list,
  a detail and a form **[ax] [sway]**.

### I1. AI (lx3, UI on m4)

- Features: AI-01, AI-02, AI-03.
- Work: provider-neutral streaming client (OpenAI-compatible, Anthropic, Ollama), Quick AI via
  `Tab`, chat window with history, AI commands with placeholders, keys in the keyring.
- Accept: client tests against a local mock server (streaming, errors, cancel); chat history
  persists across daemon restarts; a Quick AI answer streams into the view **[ax] [sway]**
  against the mock.

### G1. Settings, live reload and themes (m4, lx3)

- Features: CFG-04, CFG-05, CFG-06.
- Accept: editing `config.toml` changes behaviour within 1 s without restart; the settings
  window round-trips a commented config without losing comments (`toml_edit` golden);
  switching system appearance flips the theme **[ax] [gnome]**.

### P1. Script-command authoring and portability (lx3)

- Features: SCR-08, SCR-09.
- Accept: "Create Script Command" writes a template that `quickhop run` executes; on Linux,
  `.applescript` and `platform macos` scripts are hidden with a `doctor` note, tested with the
  Raycast goldens.

### B1. Rich clipboard (m4, lx3)

- Features: CLP-02, CLP-04, CLP-05.
- Accept: copying an image and a file shows previews **[ax] [sway]**; a value copied with
  `org.nspasteboard.ConcealedType` (m4) or `x-kde-passwordManagerHint` (lx3) never appears;
  paste-as-plain-text drops formatting (read back from the editor).

### N1. Snippet expansion and import (m4, lx3)

- Features: SNP-03, SNP-04.
- Accept: typing `;sig` in TextEdit / gedit expands it **[ax] [x11]**; with the evdev opt-in
  the same works under **[sway]**; a Raycast snippets export round-trips.

### R1. Signed packages and release (m4, lx3)

- Features: PLT-03 (complete).
- Accept: tag build produces a notarized universal DMG (`spctl -a -vv` accepts it on m4), a
  Homebrew cask that installs it, `.deb`/`.rpm`/AUR artefacts that install in clean
  containers and pass `quickhop doctor`.

## Phase C: long tail (P2)

| Slice | Machine | Features | Acceptance |
|-------|---------|----------|------------|
| X2. Store | lx3 | EXT-03, EXT-05, EXT-07 | install/update/remove from a fixture git index; OAuth PKCE against a mock provider via the deeplink; interval refresh with a fake clock |
| X3. Raycast extension shim (if approved) | lx3, m4 | EXT-02 | three popular Raycast extensions run unmodified on Node or Bun with list/detail/form views **[ax] [sway]** |
| X4. Menu bar | m4, lx3 | EXT-06, CLD-02 | a menu-bar command shows its title in `NSStatusItem` **[ax]** and as a StatusNotifierItem on D-Bus **[sway]** with waybar |
| M1. Notes | m4, lx3 | NTE-01 | a floating note stays above other windows **[ax] [x11] [sway]**, persists across restarts |
| M2. Calendar | m4, lx3 | CLD-01 | EventKit events listed on m4 (test calendar); EDS and ICS sources on lx3 with fixture data; join link opens |
| M3. Focus | m4, lx3 | FOC-01 | a session hides a blocked app on launch **[ax] [sway]** and ends on time |
| M4. Text tools | lx3 | TRN-01, DIC-01 | translate and define against mock services; golden outputs |
| M5. Pickers | m4, lx3 | CLR-01, EMO-03 | picked color equals a known pixel **[ax] [gnome]**; symbol search goldens |
| M6. Menu search | m4, lx3 | MNU-01 | TextEdit's menu items found and triggered **[ax]**; dbusmenu path on KDE recorded |
| M7. Keyboard extras | m4, lx3 | HYP-01, WIN-03, LCH-14, LCH-15 | Hyper key setup verified with `hidutil property --get`; custom layouts geometry tests; history and compact mode model tests |
| M8. System extras | m4, lx3 | SYS-03, SYS-04 | DND toggles verified via `gsettings` / Shortcuts dry-run; quit-all spares quickhop and Finder **[ax]** |
| M9. Data extras | lx3 | CAL-05, FIL-03, FIL-04, QLK-03, CLP-06 | history/import goldens; content search over the fixture tree; encrypted clipboard unreadable without the keyring key |
| M10. AI tools | lx3 | AI-04 | an extension calls the AI capability against the mock server |
| M11. Sync and migration | m4, lx3 | CFG-08, CFG-09, PLT-04 | export on m4 then import on lx3 reproduces config, snippets and quicklinks; a Raycast `.rayconfig` fixture imports; update check against a fixture release feed |

## Coverage

Every feature ID in [features.md](features.md) that is not `done` belongs to a slice above
(WIN-01, WIN-02 and PLT-03 are split between a macOS and a Linux or first/complete slice);
`done` features are kept green by the existing tests. Order within a phase may change, but a
phase starts only when the previous one is complete on both machines.

## Open questions for the owner

1. **Licence.** The repository has no licence yet. MIT OR Apache-2.0 (the Rust norm, compatible
   with every dependency today, including MPL-2.0 nucleo-matcher) or GPL-3.0 (keeps forks open)?
2. **Raycast extensions.** Build the TypeScript compatibility shim (X3: large, and Raycast's
   API changes under it) or only quickhop's own language-neutral extension API (X1)?
3. **AI provider.** Default to local models (Ollama) with bring-your-own keys for OpenAI-
   compatible and Anthropic endpoints, or pick one hosted default?
