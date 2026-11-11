# Troubleshooting

Start with `quickhop doctor`. It prints the version and flavour, the binary that runs and any
other `quickhop` copies, the platform, every path quickhop uses and whether it exists, the
daemon (pid, version, executable), on macOS the Accessibility state and which program to
switch on, the login entry, the graphical session, the helper tools this session needs, the
hotkey and each provider with its item count. Each problem is a `warn:` line with its fix,
and the last line counts them; `quickhop doctor --strict` fails when anything warned. This is
its output right after `cargo install` in a fresh Ubuntu 22.04 container (hence no session):

    $ quickhop doctor
    quickhop 0.1.0
    flavour: release (config dir /root/.config/quickhop)
    binary: /root/.cargo/bin/quickhop
    platform: linux-aarch64
    config: /root/.config/quickhop/config.toml (absent, using defaults)
    data: /root/.local/share/quickhop
    index: /root/.local/share/quickhop/index.sqlite (absent: run `quickhop index`)
    daemon: not running (`quickhop autostart on` starts it now and at every login)
    autostart: off: /root/.config/autostart/quickhop.desktop does not exist (`quickhop autostart on` writes it)
    session: none (no WAYLAND_DISPLAY or DISPLAY)
    tool xdg-open: found
    tool wl-copy: found (not used without a graphical session)
    tool wl-paste: found (not used without a graphical session)
    tool xclip: found (not used without a graphical session)
    tool loginctl: found
    tool systemctl: found
    scripts: none (create /root/.config/quickhop/scripts)
    hotkey: Alt+Shift+Space (not registered yet: the Linux window is still being built)
    provider apps: enabled (1 items)
    provider calc: enabled (0 items)
    provider emoji: enabled (1923 items)
    provider system: enabled (5 items)
    provider quicklinks: enabled (0 items)
    provider scripts: enabled (0 items)
    provider snippets: enabled (0 items)
    provider files: disabled
    ui: launcher window not built yet (gpui, see docs/adr/0001-ui-toolkit.md)
    doctor: no problems found

In an X11 session without `xclip`, the tool lines read `tool wl-copy: missing (not used in
this x11 session)` and `warn: xclip missing (needed to copy on X11): sudo apt install xclip`,
and the last line counts the warnings.

Every error quickhop prints starts with `quickhop:` and names the file, key or id involved.
Find the message below.

- [Installing](#installing)
- [Configuration](#configuration)
- [Search and results](#search-and-results)
- [Script commands](#script-commands)
- [Clipboard](#clipboard)
- [The daemon](#the-daemon)

## Installing

`scripts/install-from-source.sh` (in a clone) checks Rust, the C compiler, disk space and access
to the repository before it builds, and prints the fix for the first one missing.

**`cargo: command not found` right after installing Rust.** rustup's installer adds cargo to
`PATH` for new terminals only. Run `. "$HOME/.cargo/env"` (fish: `source
"$HOME/.cargo/env.fish"`), or open a new terminal.

**`quickhop: command not found`.** The binary's folder is not on your `PATH`. `cargo install`
uses `~/.cargo/bin`; the installer uses `~/.local/bin`. Add it in `~/.profile` (`~/.zprofile`
on macOS), then open a new terminal:

    export PATH="$HOME/.local/bin:$PATH"

**`cargo install` stops with `requires rustc 1.98` or `use of unstable library feature`.** Your
Rust is older than quickhop needs (1.98, for its window toolkit gpui). Update it, then install
again:

    rustup update stable
    cargo install --locked --git https://github.com/yuyudhan/quickhop quickhop

`rustup show` prints the version in use; a `rust-toolchain.toml` or `rustup override` in the
folder you run it from wins over `stable`. Rust from a distribution package is usually too old;
install it with rustup instead.

**`this version of Cargo is older than the 2024 edition`.** The same cause as above, from a
distribution's cargo (Debian, older Ubuntu). Install Rust with rustup and run the install from
a new terminal.

**`cargo install` fails compiling `libsqlite3-sys`, or says `cc not found`, `linker cc not
found` or `xcrun: error: invalid active developer path`.** SQLite is bundled and compiled from
C, and macOS builds need the SDK. Install a C compiler: `xcode-select --install` on macOS (wait
until its installer finishes; `xcode-select -p` then prints a path), `sudo apt install
build-essential` on Debian and Ubuntu, `sudo dnf install gcc` on Fedora, `sudo pacman -S
base-devel` on Arch.

**`failed to authenticate when downloading repository` or `failed to fetch into ...`.** A proxy
or a git credential helper is in the way. Run the install as
`CARGO_NET_GIT_FETCH_WITH_CLI=true cargo install --locked --git
https://github.com/yuyudhan/quickhop quickhop`, which fetches with git and its proxy settings.

**The build stops with `signal: 9, SIGKILL` or `No space left on device`.** It ran out of
memory or disk. Close other apps and free space (about 3 GB; `cargo clean` in old Rust
projects frees the most), then run the install again.

**The one-line install printed a `curl: (22)` or `curl: (7)` line and installed nothing.**
curl could not fetch the installer (HTTP 404, or no answer), so `sh` ran nothing; check the
network and run the line again. `-f` in `curl -fsSL` makes curl report a failed download
instead of handing an error page to `sh`.

**`quickhop: error: release v<version> not found (HTTP 404)` or `release not found: <url>`.**
GitHub has no such release (or no such asset in it), from `--version`, `QUICKHOP_VERSION` or
the latest release: check the version at <https://github.com/yuyudhan/quickhop/releases>.

**`quickhop: error: could not reach github.com (…)`.** Nothing answered: curl's own reason
follows in the parentheses (no DNS, a refused or cut connection, a proxy). Fix the network
and run the line again.

**`install.sh`: `this system uses musl libc` or `needs glibc 2.31 or later`.** The Linux
release is built for glibc 2.31 and later (Debian 11, Ubuntu 20.04, Fedora 32, RHEL 9 or
newer), and the installer refuses other systems before downloading. Build from source there:
[install.md](install.md#from-source).

**`version 'GLIBC_2.3x' not found` when running quickhop.** The same cause, for a release
binary copied by hand onto an older system: build from source.

**The installer stops with `checksum mismatch`.** The download is incomplete or altered, and
nothing was installed. Run it again; if it persists, report it with the two hashes it printed.

**macOS says "quickhop.app is damaged" or refuses to open it.** Releases are signed ad hoc,
not notarized, until quickhop has a Developer ID; an app downloaded with a browser carries the
quarantine flag, which Gatekeeper then refuses. `install.sh` clears it; for a copy you
downloaded yourself run `xattr -dr com.apple.quarantine /Applications/quickhop.app` (`-r`:
the whole bundle), or for a bare binary `xattr -d com.apple.quarantine /path/to/quickhop`.

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

**`cannot start wl-paste (it records clipboard history in this Wayland session): …; install
it: sudo apt install wl-clipboard`.** Wayland watching runs `wl-paste --watch`; install the
package the message names (it gives your distribution's command). The same holds for
`cannot start xclip (it copies text on X11)` (package `xclip`) and `cannot start wl-copy (it
copies text in this Wayland session …)` (package `wl-clipboard`). Under XWayland, where
`DISPLAY` and `WAYLAND_DISPLAY` are both set, quickhop uses wl-clipboard, not xclip.

**`wl-paste stopped ...: the compositor may lack the data-control protocol; GNOME has none`.**
GNOME on Wayland gives no other program the clipboard. Use an X11 session, or add entries with
`quickhop clipboard add`. See [platform-support.md](platform-support.md).

**`copied, not pasted: ...`.** The text is on the clipboard but no paste key was sent; the
reason follows. On macOS, allow the app running quickhop in System Settings › Privacy &
Security › Accessibility (Device Control and Data Access on macOS 27 and later); for
quickhop.app, `quickhop onboarding` opens the window that leads there and says when it is
allowed. On Wayland, synthetic paste is not available: press Ctrl+V.

**quickhop is switched on in Accessibility but still says `copied, not pasted` after an
update (macOS).** The grant belongs to one build: an ad-hoc signed quickhop.app gets a new
signature with every build or update, and macOS refuses the new copy while the list still
shows quickhop switched on. Open quickhop.app (or run `quickhop onboarding`) and press its
button: it clears the old entry and adds this build, so the switch you turn on is the one
macOS checks. For a bare `quickhop` binary, remove its entry with − and add it again with +.

## The daemon

**`quickhop daemon is already running (pid N)`.** One daemon runs per user.
`quickhop daemon status` shows what it does; `quickhop daemon stop` ends it.

**The daemon does not start at login.** `quickhop autostart status` shows whether the login
entry exists, where, and what is wrong with it. After moving or reinstalling the binary, run
`quickhop autostart on` again so the entry starts the new one.

**`autostart on` says `sway does not start XDG autostart entries`** (or i3, Hyprland, river).
These compositors ignore XDG autostart, so quickhop writes no entry; it starts the daemon for
this session and prints the line to add, such as `exec /home/you/.cargo/bin/quickhop daemon`
for `~/.config/sway/config` or `exec-once = …` for `~/.config/hypr/hyprland.conf`. Add it to
the config file it names, then log in again. When dex or uwsm runs XDG autostart entries in
your session, `quickhop autostart on --xdg` writes one anyway.

**`warn: autostart runs …, which does not exist (moved or uninstalled)`.** The login entry
points at a binary that is gone, after moving, uninstalling or switching install methods. Run
`quickhop autostart on` again with the quickhop you use now.

**`warn: both the systemd user unit (…) and … start quickhop`.** Two login entries start the
daemon, so every login logs `already running`. Keep one: `quickhop autostart off`, or
`systemctl --user disable quickhop`.

**The hotkey does nothing, or opens another launcher too (macOS).** Run `quickhop doctor`: its
`daemon:` line says whether a daemon runs, which version and from where, and each `warn:` line
gives a fix. With no daemon running there is no hotkey: `quickhop autostart on`, or run
`quickhop toggle` once. `quickhop daemon status` ends with the hotkey the daemon holds, or
`hotkey: off (...)` and why: `Alt+Shift+Space is taken by another app` means another app holds that
combination for itself. macOS otherwise lets several apps register the same combination, so
another launcher bound to it may open as well. Either way, set another `hotkey` in
`config.toml` and run `quickhop reload`. A development build listens on `Ctrl+Alt+Shift+Space`
unless its own config says otherwise ([install.md](install.md#development-builds)).

**quickhop.app does nothing when opened (macOS).** It shows an alert saying why (an invalid
`config.toml`, a daemon that cannot start); the same text is appended to
`~/Library/Logs/quickhop.log` (`quickhop-dev.log` for quickhop Dev.app).

## Reporting a problem

Open an issue at <https://github.com/yuyudhan/quickhop/issues> with the output of
`quickhop doctor`, the exact command, and what it printed.
