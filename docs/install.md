# Install

quickhop is one binary, `quickhop`, for macOS (Apple silicon and Intel) and Linux (x86_64 and
aarch64, X11 and Wayland). Releases, from v0.1.0, ship it prebuilt with an installer script;
you can also build it from source with Rust 1.98 or newer.

- [Requirements](#requirements)
- [The installer script](#the-installer-script) (prebuilt, no Rust needed)
- [Release assets](#release-assets)
- [From source](#from-source)
- [Debian and Ubuntu package](#debian-and-ubuntu-package)
- [macOS app](#macos-app)
- [Development builds](#development-builds)
- [After installing](#after-installing)
- [Update](#update) · [Uninstall](#uninstall)

## Requirements

For a prebuilt release:

- **macOS** on Apple silicon or Intel: one universal build.
- **Linux** on x86_64 or aarch64 with **glibc 2.31 or later**: Debian 11, Ubuntu 20.04,
  Fedora 32, RHEL 9 or newer. musl systems (Alpine, Void musl) cannot run it; build from source
  there, as on older glibc systems.

To build from source:

- **Rust 1.98 or newer, from [rustup](https://rustup.rs).** quickhop's window toolkit (gpui)
  needs it. If you already have Rust, run `rustup update stable` first: an older Rust stops at
  once with `requires rustc 1.98`. Rust from a distribution package (`apt install rustc`,
  `dnf install rust`) is usually older; use rustup instead. `cargo install` does not read the
  repository's `rust-toolchain.toml`; the toolchain you run it with is the one that builds.
  `rustc --version` says which one that is.
- **A C compiler**, for the bundled SQLite: the Xcode Command Line Tools on macOS
  (`xcode-select --install`, then wait until its installer window finishes;
  `xcode-select -p` prints a path once they are in), `build-essential` / `gcc` / `base-devel`
  on Linux.
- **Disk and network**: on macOS cargo downloads the window toolkit's git repository (about
  0.5 GB: 409 MB of history and a 98 MB checkout), and the release build takes 1.1 GB more
  (measured on Apple silicon); have about 3 GB free. The first build takes several minutes,
  longer on small machines; with less than 8 GB of memory, close other apps (an out-of-memory
  build ends in `signal: 9, SIGKILL`).
- **Network access to github.com**, for cargo to fetch the repository. Behind a proxy, run
  cargo with `CARGO_NET_GIT_FETCH_WITH_CLI=true` so it uses git and git's proxy settings.

`scripts/install-from-source.sh` checks every one of these first and stops with the command
that fixes the first one missing, before cargo starts; then it runs the install below. From a
clone: `scripts/install-from-source.sh` (`--check` only checks; `--tag`, `--branch` and
`--force` are passed to cargo).

## The installer script

Every release attaches `install.sh`. This one line downloads it into a new temporary folder,
runs it from there, and removes that folder when it ends, whether it worked, failed or you
pressed Ctrl+C. Nothing is left in the folder you run it from:

    (d="$(mktemp -d)" && trap 'rm -rf "$d"' EXIT INT TERM && curl -fsSL --retry 3 -o "$d/install.sh" https://github.com/yuyudhan/quickhop/releases/latest/download/install.sh && bash "$d/install.sh")

It runs in bash and zsh (macOS's default shell); from fish, run `bash` first. The parentheses
keep the temporary folder and its cleanup to that one command. The script is downloaded before
anything runs rather than piped into `bash`: a failed download piped into bash runs nothing and
exits 0, so it looks like success. Here a missing (HTTP 404) or interrupted download stops the
command with curl's error and a non-zero exit status, and the installer never starts.

It installs quickhop for one user, without `sudo`:

- refuses a system its build cannot start on before downloading anything: Linux needs glibc
  2.31 or later and not musl, and it names the from-source instructions instead;
- picks the archive for your OS and CPU (`x86_64` or `aarch64` Linux; one universal macOS
  build) and fails with the reason when a download is missing (HTTP 404), empty or interrupted;
- checks its SHA-256 against the `.sha256` published beside it and stops on a mismatch;
- runs the downloaded `quickhop --version` before replacing anything, so a build this system
  cannot start fails the install with the loader's message and leaves an installed copy as it
  was;
- puts `quickhop` in `~/.local/bin` (or `QUICKHOP_INSTALL_DIR`), replacing an older binary
  atomically so a running daemon is never left with a half-written file;
- on Linux, adds the desktop entry and icon under `~/.local/share`; on macOS, installs
  `quickhop.app` into `/Applications` (or `~/Applications`, or `QUICKHOP_APP_DIR`), clears the
  download quarantine flag, and makes `~/.local/bin/quickhop` a symlink to
  `quickhop.app/Contents/MacOS/quickhop`, so the command line and the app are one program with
  one Accessibility grant;
- when a quickhop daemon is running, stops it before anything is replaced and starts it again
  the same way afterwards (its LaunchAgent, quickhop.app, its systemd user unit, or
  `quickhop daemon`); if it does not stop, the update aborts with nothing replaced.

| Option | Does |
|--------|------|
| `--version X.Y.Z` | Install that release instead of the latest; refuses an archive holding another version. |
| `--from DIR` | Install from a local directory of release assets, with no network access. |
| `--uninstall` | Stop the daemon, turn autostart off, and remove the binary, app and desktop files. Keeps your config and data. |
| `--purge` | With `--uninstall`, also delete `~/.config/quickhop` and `~/.local/share/quickhop`, naming each before it goes. |
| `-h`, `--help` | Show the options. |

Options go after `bash "$d/install.sh"` inside the parentheses, for example
`… && bash "$d/install.sh" --version 0.1.0)` or `… && bash "$d/install.sh" --uninstall)`.
The installer itself downloads and unpacks the release into its own temporary folder
(`mktemp -d`, under `TMPDIR`) and removes it on every exit, including a failed download or
checksum, Ctrl+C and `kill`.

| Variable | Default | Meaning |
|----------|---------|---------|
| `QUICKHOP_INSTALL_DIR` | `~/.local/bin` | Where the binary goes. |
| `QUICKHOP_APP_DIR` | `/Applications` | Where the macOS app goes; `~/Applications` when `/Applications` is not writable. |
| `QUICKHOP_INSTALL_BASE` | GitHub releases | URL of a directory holding the release assets (a mirror). |

If `~/.local/bin` is not on your `PATH`, the installer prints the line to add to `~/.profile`.

## Release assets

Each release on <https://github.com/yuyudhan/quickhop/releases> carries:

| Asset | For |
|-------|-----|
| `quickhop-universal-apple-darwin.tar.gz` | macOS, Apple silicon and Intel: `quickhop.app` and the binary |
| `quickhop-x86_64-unknown-linux-gnu.tar.gz` | Linux x86_64 |
| `quickhop-aarch64-unknown-linux-gnu.tar.gz` | Linux aarch64 |
| `quickhop_<version>_amd64.deb`, `quickhop_<version>_arm64.deb` | Debian and Ubuntu: `sudo apt install ./quickhop_<version>_<arch>.deb` |
| `install.sh` | the installer above |

Every asset has a `.sha256` beside it, and `SHA256SUMS` lists them all. Offline: download the
archive for your system, its `.sha256` and `install.sh` into one folder, then run
`bash install.sh --from FOLDER`.

The macOS build is signed ad hoc, not notarized, until quickhop has a Developer ID; the
installer clears the quarantine flag that would make Gatekeeper refuse it. For a copy you
unpacked yourself, see [troubleshooting](troubleshooting.md#installing).

## From source

Already have Rust? Start with `rustup update stable`; quickhop needs 1.98 or newer.

**macOS**

    xcode-select --install                          # C compiler; wait until it finishes
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh
    sh rustup-init.sh                               # skip both lines if you have rustup
    . "$HOME/.cargo/env"                            # or open a new terminal
    rustup update stable                            # quickhop needs Rust 1.98 or newer
    cargo install --locked --git https://github.com/yuyudhan/quickhop quickhop

**Linux** (Debian and Ubuntu shown)

    sudo apt install build-essential xdg-utils
    sudo apt install wl-clipboard                   # Wayland sessions: clipboard history
    sudo apt install xclip                          # X11 sessions: copying results
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh
    sh rustup-init.sh                               # skip both lines if you have rustup
    . "$HOME/.cargo/env"                            # or open a new terminal
    rustup update stable                            # quickhop needs Rust 1.98 or newer
    cargo install --locked --git https://github.com/yuyudhan/quickhop quickhop

Fedora: `sudo dnf install gcc xdg-utils wl-clipboard xclip`. Arch: `sudo pacman -S base-devel
xdg-utils wl-clipboard xclip`. On Linux the `quickhop` binary does not use the window toolkit
yet, so it needs no graphics libraries to build.

fish and nushell: instead of `. "$HOME/.cargo/env"`, source `env.fish` or `env.nu` from the
same folder, or open a new terminal.

`cargo install` puts the binary in `~/.cargo/bin`, which rustup adds to your `PATH`.
`--locked` builds the dependency versions in `Cargo.lock`, the ones quickhop is tested with;
without it cargo picks the newest compatible versions, which can break the build.

**Windows**: only the command line works there (no daemon, hotkey or window yet; see
[platform support](platform-support.md)). With Rust from rustup and the Visual Studio build
tools: `cargo install --locked --git https://github.com/yuyudhan/quickhop quickhop`.

From a clone instead (its `rust-toolchain.toml` picks the toolchain quickhop is developed
with):

    git clone https://github.com/yuyudhan/quickhop && cd quickhop
    cargo install --locked --path crates/quickhop

Running the tests on Linux also builds the window toolkit, which needs `pkg-config` and its
libraries: `sudo apt install pkg-config libfontconfig-dev libfreetype-dev libwayland-dev
libxkbcommon-x11-dev` (Fedora: `pkgconf fontconfig-devel freetype-devel wayland-devel
libxkbcommon-x11-devel`; Arch: `pkgconf fontconfig freetype2 wayland libxkbcommon-x11`).

Offline: in a clone, `cargo fetch --locked` while online, then `cargo install --locked
--offline --path crates/quickhop` (or `cargo vendor` to carry the sources elsewhere).

## Debian and Ubuntu package

Each release attaches a `.deb` for amd64 and arm64 ([release assets](#release-assets)). To build
one from a clone, with `dpkg-dev` and `jq` installed:

    just bundle-linux
    sudo apt install ./target/bundle/quickhop_*_$(dpkg --print-architecture).deb

The package installs `/usr/bin/quickhop`, a desktop entry, the icon, both licence texts and a
systemd user unit. The same build writes a tarball, `quickhop-<version>-<arch>-linux.tar.gz`,
with that tree under `quickhop-<version>/`.

The app-menu entry starts the daemon in the background and shows a notification saying
whether it started or was already running. The systemd user unit only starts where the
session reaches `graphical-session.target` (GNOME, KDE); elsewhere use `quickhop autostart on`.

## macOS app

The release archive and the installer script carry `quickhop.app`. To build it from a clone,
on macOS 14 or later with the Command Line Tools and [`just`](https://github.com/casey/just)
(`brew install just`; or run `scripts/bundle-macos.sh` directly):

    just bundle-macos-release
    cp -R target/bundle/quickhop.app /Applications/

`quickhop.app` holds the release binary as its executable, the icon and an ad-hoc signature
(no Developer ID yet). Open it from Finder or Spotlight: it starts the daemon with the
launcher window and the hotkey (`Alt+Space` unless `hotkey` in `config.toml` says otherwise)
and shows the launcher, or the onboarding window while quickhop may not paste yet. It is an
agent app, without a Dock icon or menu bar; `quickhop daemon stop` quits it.

Install the app before running `quickhop autostart on`: the login item then runs the app,
whose Accessibility permission covers pasting.

`just bundle-macos` (without `-release`) builds **quickhop Dev.app**
(`io.github.yuyudhan.quickhop.dev`) instead: a [development build](#development-builds) that
runs beside the release with its own config, data, daemon, login item and Accessibility
entry. Don't install it as your quickhop.

## Development builds

Debug builds (`cargo build`, `cargo run`, `cargo test`) and `just bundle-macos` are the
development flavour, so you can work on quickhop while an installed release keeps running.
Releases (`cargo install`, `just bundle-macos-release`, the release archives) are unchanged.

| | Release | Development |
|---|---|---|
| Config | `~/.config/quickhop/` | `~/.config/quickhop-dev/` |
| Data | `~/.local/share/quickhop/` | `~/.local/share/quickhop-dev/` |
| Daemon socket | `$XDG_RUNTIME_DIR/quickhop`, else `$TMPDIR/quickhop-$USER` | `…/quickhop-dev`, `$TMPDIR/quickhop-dev-$USER` |
| Default hotkey | `Alt+Space` | `Alt+Shift+Space` |
| macOS app | `quickhop.app`, `io.github.yuyudhan.quickhop` | `quickhop Dev.app`, `io.github.yuyudhan.quickhop.dev` |
| Login entry | `io.github.yuyudhan.quickhop.plist`, `quickhop.desktop` | `io.github.yuyudhan.quickhop.dev.plist`, `quickhop-dev.desktop` (`quickhop Dev`) |
| macOS app log | `~/Library/Logs/quickhop.log` | `~/Library/Logs/quickhop-dev.log` |
| `quickhop --version` | `quickhop 0.1.0` | `quickhop 0.1.0 (dev)` |

`quickhop doctor` names the flavour and its config folder on its second line. In a clone,
`just dev-config` installs a config with every option set and commented into
`~/.config/quickhop-dev/` ([configuration](configuration.md#where-files-live)).

## After installing

    quickhop doctor               # what runs, what starts it, what is missing
    quickhop autostart on         # start the daemon now and at every login
    quickhop daemon status

`quickhop autostart on` starts the daemon at once, so the hotkey and clipboard history work
right away, and registers it for every login: a LaunchAgent on macOS (running quickhop.app
when it is installed), an XDG autostart entry on Linux. On sway, i3, Hyprland and river, which
do not run XDG autostart entries, it starts the daemon for this session and prints the line to
add to the compositor's config; add it, then log in again. It registers the program you ran
it with, so run it again after moving the binary or switching install methods. With the
`.deb`, `systemctl --user enable --now quickhop` is an alternative; use one or the other.

Then follow the [30-second tour](usage.md#a-30-second-tour).

## Update

- **`install.sh`**: run the [one-line install](#the-installer-script) again. It stops a
  running daemon first and starts the new one; there is nothing to restart.
- **`cargo install`**: `rustup update stable` (a newer quickhop may need a newer Rust), then
  `cargo install --locked --git https://github.com/yuyudhan/quickhop quickhop --force`.
- **The `.deb`**: `sudo apt install ./quickhop_<version>_<arch>.deb` with the newer file.

After `cargo install` or the `.deb`, restart the daemon so it runs the new binary:

    quickhop daemon stop
    quickhop autostart on      # starts the new one; or log out and in again

## Uninstall

Run `quickhop autostart off` before removing the binary, so no login entry is left pointing at
it.

| Installed with | Remove with |
|----------------|-------------|
| `install.sh` | the [one-line install](#the-installer-script) ending in `bash "$d/install.sh" --uninstall)` (add `--purge` to delete config and data) |
| `cargo install` | `quickhop autostart off; quickhop daemon stop; cargo uninstall quickhop` |
| the `.deb` | `quickhop autostart off; sudo apt remove quickhop` |
| `quickhop.app` copied by hand | `quickhop autostart off; quickhop daemon stop; rm -rf /Applications/quickhop.app` |

On macOS, also remove quickhop's Accessibility entry: `tccutil reset Accessibility
io.github.yuyudhan.quickhop`. If you removed quickhop without `autostart off` first, delete the
login entry it left (a quickhop installed later warns about it in `doctor`):
`~/Library/LaunchAgents/io.github.yuyudhan.quickhop.plist` on macOS,
`~/.config/autostart/quickhop.desktop` on Linux.

Your settings stay in `~/.config/quickhop` and your history in `~/.local/share/quickhop` until
you delete them.
