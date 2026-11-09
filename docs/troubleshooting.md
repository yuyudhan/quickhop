# Troubleshooting

Start with `quickhop doctor`. It prints the version, the platform, every path quickhop uses and
whether it exists, the graphical session, the helper tools it found, the hotkey and each
provider with its item count. This is its output for a fresh account on Ubuntu 22.04, run over
SSH (hence no session):

    $ quickhop doctor
    quickhop 0.1.0
    platform: linux-x86_64
    config: /home/kalki3/.config/quickhop/config.toml (absent, using defaults)
    data: /home/kalki3/.local/share/quickhop
    index: /home/kalki3/.local/share/quickhop/index.sqlite (absent: run `quickhop index`)
    autostart: off (`quickhop autostart on` writes /home/kalki3/.config/autostart/quickhop.desktop)
    scripts: none (create /home/kalki3/.config/quickhop/scripts)
    session: none (no WAYLAND_DISPLAY or DISPLAY)
    tool xdg-open: found
    tool wl-copy: found
    tool wl-paste: found
    tool xclip: found
    tool loginctl: found
    tool systemctl: found
    hotkey: Alt+Space (not registered yet: the Linux window is still being built)
    provider apps: enabled (49 items)
    provider calc: enabled (0 items)
    provider emoji: enabled (1923 items)
    provider system: enabled (5 items)
    provider quicklinks: enabled (0 items)
    provider scripts: enabled (0 items)
    provider snippets: enabled (0 items)
    provider files: disabled
    ui: launcher window not built yet (gpui, see docs/adr/0001-ui-toolkit.md)

Every error quickhop prints starts with `quickhop:` and names the file, key or id involved.
Find the message below.

- [Installing](#installing)
- [Configuration](#configuration)
- [Search and results](#search-and-results)
- [Script commands](#script-commands)
- [Clipboard](#clipboard)
- [The daemon](#the-daemon)

## Installing

**`quickhop: command not found`.** The binary's folder is not on your `PATH`. `cargo install`
uses `~/.cargo/bin`; the installer uses `~/.local/bin`. Add it in `~/.profile` (`~/.zprofile`
on macOS), then open a new terminal:

    export PATH="$HOME/.local/bin:$PATH"

**`cargo install` fails compiling `libsqlite3-sys`.** SQLite is bundled and compiled from C.
Install a C compiler: `xcode-select --install` on macOS, `sudo apt install build-essential` on
Debian and Ubuntu.

**The installer stops with `checksum mismatch`.** The download is incomplete or altered, and
nothing was installed. Run it again; if it persists, report it with the two hashes it printed.

**macOS refuses to open a downloaded binary.** Files downloaded by a browser carry the
quarantine flag. The installer clears it; for a file you fetched yourself, run
`xattr -d com.apple.quarantine /path/to/quickhop`.

## Configuration

**`invalid config: unknown field ...`.** A key is misspelled or in the wrong table; the message
gives the line. Every key is listed in [configuration.md](configuration.md), and
`quickhop config check FILE` tests a file without using it.

**`invalid config: unknown command id ...`.** A `[[commands]]` entry names a result that no
longer exists, such as an uninstalled app or a renamed script. Find the current id with
`quickhop query NAME --json`, or delete the entry.

**A change in `config.toml` has no effect.** Check which file is read with
`quickhop config path`. A running daemon reads the config when it starts; `quickhop reload`
makes it read it again.

## Search and results

**An application is missing.** quickhop reads app bundles from the Applications folders on
macOS, and desktop entries from every XDG `applications` folder on Linux. Entries marked
`NoDisplay` or `Hidden`, limited to other desktops by `OnlyShowIn`/`NotShowIn`, or whose
`TryExec` program is missing are skipped, as in your desktop's own menu. Add other folders with
`[apps] dirs`.

**No files are found on Linux.** The index has not been built: run `quickhop index`, then
`quickhop autostart on` so the daemon keeps it current. `quickhop doctor` shows whether
`index.sqlite` exists.

**No files are found on macOS.** Without an index, Spotlight answers from the third character
typed and skips folders excluded in System Settings → Spotlight. `quickhop index` builds
quickhop's own index, which is used from then on.

**Ranking keeps preferring an old pick.** Picks are remembered per query in `frecency.sqlite`
in the data folder and fade over time. Delete that file to start fresh.

## Script commands

**`no script command "NAME" in ...`.** The message lists the folders searched. The file must
sit directly in one of them (folders are not searched recursively), carry
`@raycast.schemaVersion 1`, `@raycast.title` and `@raycast.mode`, and be neither hidden nor a
`*.template.*` file. `quickhop query NAME` shows whether quickhop sees it.

**`invalid script command PATH: ...`.** A header has a wrong value: an unknown `mode`, a
`refreshTime` not written like `10s`, `5m`, `1h` or `1d`, argument JSON that does not parse, a
dropdown without `data`, or arguments on an `inline` script. The message names the header.

**`cannot run PATH: Permission denied`.** The file is not executable: `chmod +x PATH`.

**`NAME needs confirmation: run again with --yes`.** The script sets
`@raycast.needsConfirmation true`.

**`script failed: ...` or `... timed out`.** The script exited non-zero (the message is its
last line of output) or ran past the timeout. Run the file directly to see all of its output,
or allow more time with `--timeout SECONDS`.

## Clipboard

**`quickhop clipboard list` stays empty.** Something must watch the clipboard:
`quickhop autostart on` starts the daemon at login, or run `quickhop clipboard watch` in a
terminal. `quickhop daemon status` says whether the clipboard is watched and, if not, why.

**`daemon status` says `clipboard: off (no graphical session ...)`.** The daemon was started
outside your desktop session (over SSH, or by a systemd unit without the session's
variables), so it cannot reach the display. Start it from the desktop with autostart, or hand
the variables to systemd and restart the unit:

    systemctl --user import-environment DISPLAY WAYLAND_DISPLAY
    systemctl --user restart quickhop

**`cannot start wl-paste (install wl-clipboard)`.** Wayland watching runs `wl-paste --watch`;
install the `wl-clipboard` package.

**`wl-paste stopped ...: the compositor may lack the data-control protocol; GNOME has none`.**
GNOME on Wayland gives no other program the clipboard. Use an X11 session, or add entries with
`quickhop clipboard add`. See [platform-support.md](platform-support.md).

**`copied, not pasted: ...`.** The text is on the clipboard but no paste key was sent; the
reason follows. On macOS, grant Accessibility to the app running quickhop in System Settings →
Privacy & Security → Accessibility. On Wayland, synthetic paste is not available: press Ctrl+V.

## The daemon

**`quickhop daemon is already running (pid N)`.** One daemon runs per user.
`quickhop daemon status` shows what it does; `quickhop daemon stop` ends it.

**The daemon does not start at login.** `quickhop autostart status` shows whether the login
entry exists and where. After moving or reinstalling the binary, run `quickhop autostart on`
again so the entry starts the new one.

**The hotkey does nothing, or opens another launcher too (macOS).** `quickhop daemon status`
ends with the hotkey the daemon holds, or `hotkey: off (...)` and why: `Alt+Space is taken by
another app` means another app holds that combination for itself. macOS otherwise lets
several apps register the same combination, so another launcher bound to it may open as
well. Either way, set another `hotkey` in `config.toml` and run `quickhop reload`. With no
daemon running there is no hotkey: `quickhop autostart on`, or run `quickhop toggle` once.

## Reporting a problem

Open an issue at <https://github.com/yuyudhan/quickhop/issues> with the output of
`quickhop doctor`, the exact command, and what it printed.
