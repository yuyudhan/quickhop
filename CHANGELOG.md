# Changelog

Every user-facing change is listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Nothing has been released yet:
everything below goes into v0.1.0.

## [Unreleased]

### Added

- **Search** over applications (with icons), calculator and unit conversion, emoji, system
  commands (lock, log out, sleep, restart, shut down), quicklinks with `{query}`, Raycast
  script commands, snippets and files, with fuzzy matching and ranking that learns from what
  you pick: `quickhop query`, `launch`, `open`, `run`.
- **Raycast script commands** unchanged: every documented `@raycast.*` field, all four modes,
  up to three text, password or dropdown arguments, `refreshTime`, `needsConfirmation`.
- **Snippets** in `snippets.toml` with Raycast's dynamic placeholders and modifiers.
- **Clipboard history** on macOS, X11 and Wayland compositors with the data-control protocol,
  deduplicated, skipping password-manager copies; `quickhop clipboard watch|add|list|paste|clear`.
- **Paste into the focused app**: ⌘V on macOS with the Accessibility permission, Ctrl+V on
  X11; elsewhere it copies and says why it did not paste.
- **File and folder search** with quickhop's own index (kept current by the daemon) or
  Spotlight on macOS; reveal, copy path, Quick Look and trash.
- **Configuration** in plain TOML under `~/.config/quickhop` on macOS and Linux:
  `[providers]`, `[apps]`, `[scripts]`, `[files]`, `[clipboard]`, `[[quicklinks]]` and
  per-result `[[commands]]` (alias, on/off, order, hotkey check); `quickhop config
  path|default|check` and `quickhop reload`.
- **The daemon**, one per user: clipboard watcher, live file index, `open`, `reload`,
  `daemon status|stop`; `quickhop autostart on|off|status` for launch at login.
- **The macOS launcher window** on the global hotkey (`Alt+Space`) or `quickhop toggle`, with an
  action panel, script-command fields, output pages, toasts and HUDs, and the Clipboard
  History, Search Snippets and Search Files views.
- **quickhop.app** for macOS, an agent app that runs the daemon and the window, with an
  onboarding window for the Accessibility permission.
- **Linux packages**: a `.deb` and a tarball with a desktop entry, icon and systemd user unit.
- **Release pipeline**: a `vX.Y.Z` tag builds a universal macOS archive (arm64 and x86_64) with
  quickhop.app, Linux x86_64 and aarch64 archives and `.deb`s built for glibc 2.31 and later,
  `install.sh`, a `.sha256` per asset and `SHA256SUMS`.
- **`install.sh`**, the no-sudo installer: it refuses a system its build cannot start on
  (musl, glibc older than 2.31) before downloading, fails with the reason on a missing (HTTP
  404), empty or corrupt download and on a binary this system cannot run, links
  `~/.local/bin/quickhop` into quickhop.app on macOS (one program, one Accessibility grant),
  and stops a running daemon before an update and starts the new one the same way.
- **`scripts/install-from-source.sh`** checks Rust, the C compiler, disk space and access to
  the repository before cargo starts, and prints the command that fixes the first one missing.
- **`quickhop doctor`** names the running binary and its other copies, the daemon's pid,
  version and executable, Accessibility and which program to switch on, the program the login
  entry runs, and this session's clipboard tool with your distribution's install command;
  `--strict` fails when anything warned.
- **Development builds** (debug builds, `just bundle-macos`) run beside an installed release:
  `~/.config/quickhop-dev`, `~/.local/share/quickhop-dev`, their own daemon, quickhop Dev.app
  (`io.github.yuyudhan.quickhop.dev`) with its own Accessibility entry, login item and log,
  and the `Alt+Shift+Space` hotkey. `just bundle-macos-release` builds quickhop.app.
- **`just dev-config`** installs a config with every option set and commented into
  `~/.config/quickhop-dev`.

### Changed

- quickhop needs **Rust 1.98 or newer** to build (its window toolkit, gpui). The workspace
  declares it, so an older Rust stops at once with `requires rustc 1.98 or newer`: run
  `rustup update stable`. CI builds with exactly 1.98.
- The docs install by **downloading `install.sh`, then running it**, instead of piping `curl`
  into `bash`, where a failed download exits 0 and looks like success.
- On macOS, config and data moved to `~/.config/quickhop` and `~/.local/share/quickhop`; the
  first run moves them from `~/Library/Application Support/quickhop` and says so.
- `quickhop autostart on` starts the daemon at once (on macOS through `launchctl bootstrap`),
  replacing a daemon from another program or version, and registers quickhop.app when it is
  installed. On sway, i3, Hyprland and river, which ignore XDG autostart, it prints the exec
  line for the compositor's config instead of claiming success.
- Opening quickhop.app replaces a daemon running from another executable or version; daemon
  replies carry `version` and `exe`.
- The onboarding window names the macOS 27 pane (Privacy & Security › Accessibility, under
  Device Control and Data Access) and the program to switch on, re-checks every second after
  its button and comes back to the front once allowed; its height follows its text.
- The Linux app-menu entry starts the daemon and shows a notification saying which.
- Errors from missing helper tools (`xclip`, `wl-clipboard`) name the package to install.

### Fixed

- A stale Accessibility grant after a rebuild or update: the onboarding button clears
  quickhop.app's old entry before macOS adds this build.
- quickhop.app failing silently: failures show an alert and go to `~/Library/Logs/quickhop.log`.
- `[[commands]] order` was ignored for emoji and file results.
- `[clipboard] enabled = false` did not stop `quickhop clipboard watch` or, on Wayland, the
  `wl-paste` watcher's recording.
- `quickhop config check` passed with a broken `snippets.toml`.
- `quickhop daemon stop` left a daemon running whose launcher window was still starting.
- The `.deb` description no longer mentions Windows.
