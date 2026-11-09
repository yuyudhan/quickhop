# Install

quickhop is one binary, `quickhop`, for macOS (Apple silicon and Intel) and Linux (x86_64 and
aarch64, X11 and Wayland). No release has been published yet, so today you build it from
source; it takes a couple of minutes and needs nothing beyond Rust and a C compiler.

- [From source](#from-source) (works today)
- [Debian and Ubuntu package](#debian-and-ubuntu-package) (built from source)
- [The installer script](#the-installer-script) (what each release ships)
- [After installing](#after-installing)
- [Update](#update) · [Uninstall](#uninstall)

## From source

**macOS**

    xcode-select --install                          # C compiler for the bundled SQLite
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    cargo install --git https://github.com/yuyudhan/quickhop quickhop

**Linux** (Debian and Ubuntu shown; any distribution with a C compiler works)

    sudo apt install build-essential xdg-utils
    sudo apt install libfontconfig-dev libfreetype-dev libwayland-dev libxkbcommon-x11-dev  # window toolkit
    sudo apt install wl-clipboard                   # Wayland sessions: clipboard history
    sudo apt install xclip                          # X11 sessions: copying results
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    cargo install --git https://github.com/yuyudhan/quickhop quickhop

Fedora: `sudo dnf install gcc xdg-utils wl-clipboard xclip fontconfig-devel freetype-devel
wayland-devel libxkbcommon-x11-devel`. Arch: `sudo pacman -S base-devel xdg-utils wl-clipboard
xclip fontconfig freetype2 wayland libxkbcommon-x11`.

`cargo install` puts the binary in `~/.cargo/bin`, which rustup adds to your `PATH`. quickhop
needs Rust 1.89 or newer; `rustup update` gets it.

From a clone instead:

    git clone https://github.com/yuyudhan/quickhop && cd quickhop
    cargo install --path crates/quickhop

## Debian and Ubuntu package

From a clone, with `dpkg-dev` and `jq` installed:

    just bundle-linux
    sudo apt install ./target/bundle/quickhop_*_$(dpkg --print-architecture).deb

The package installs `/usr/bin/quickhop`, a desktop entry, the icon, both licence texts and a
systemd user unit. The same build writes a tarball, `quickhop-<version>-<arch>-linux.tar.gz`,
with that tree under `quickhop-<version>/`.

## macOS app

From a clone, on macOS 14 or later with the Command Line Tools:

    just bundle-macos
    cp -R target/bundle/quickhop.app /Applications/

`quickhop.app` holds the release binary as its executable, the icon and an ad-hoc signature
(no Developer ID yet). Open it from Finder or Spotlight: it starts the daemon with the
launcher window and the hotkey (`Alt+Space` unless `hotkey` in `config.toml` says otherwise)
and shows the launcher, or the onboarding window while quickhop may not paste yet. It is an
agent app, without a Dock icon or menu bar; `quickhop daemon stop` quits it.

## The installer script

`scripts/install.sh` installs quickhop for one user, without `sudo`:

- picks the archive for your OS and CPU (`x86_64` or `aarch64` Linux; one universal macOS
  build);
- checks its SHA-256 against the `.sha256` published beside it and stops on a mismatch;
- puts `quickhop` in `~/.local/bin` (or `QUICKHOP_INSTALL_DIR`), replacing an older binary
  atomically so a running daemon is never left with a half-written file;
- on Linux, adds the desktop entry and icon under `~/.local/share`; on macOS, installs
  `quickhop.app` into `/Applications` (or `~/Applications`, or `QUICKHOP_APP_DIR`) when the
  archive carries one, and clears the download quarantine flag.

Each GitHub release attaches `install.sh` with its archives, so once one is published this is
the whole installation:

    curl -fsSL https://github.com/yuyudhan/quickhop/releases/latest/download/install.sh | bash

To use the installer today, package your own build and install from the directory:

    cargo build --release -p quickhop
    scripts/dist.sh --target x86_64-unknown-linux-gnu --bin target/release/quickhop
    scripts/install.sh --from target/dist

On macOS use `--target universal-apple-darwin`, and add `--app target/bundle/quickhop.app`
(from `just bundle-macos`) to install the app as well.

| Option | Does |
|--------|------|
| `--version X.Y.Z` | Install that release instead of the latest; refuses an archive holding another version. |
| `--from DIR` | Install from a local directory of release assets, with no network access. |
| `--uninstall` | Stop the daemon, turn autostart off, and remove the binary, app and desktop files. Keeps your config and data. |
| `--purge` | With `--uninstall`, also delete `~/.config/quickhop` and `~/.local/share/quickhop`, naming each before it goes. |
| `-h`, `--help` | Show the options. |

When piping into bash, options go after `bash -s --`:

    curl -fsSL https://github.com/yuyudhan/quickhop/releases/latest/download/install.sh | bash -s -- --uninstall

| Variable | Default | Meaning |
|----------|---------|---------|
| `QUICKHOP_INSTALL_DIR` | `~/.local/bin` | Where the binary goes. |
| `QUICKHOP_APP_DIR` | `/Applications` | Where the macOS app goes; `~/Applications` when `/Applications` is not writable. |
| `QUICKHOP_INSTALL_BASE` | GitHub releases | URL of a directory holding the release assets (a mirror). |

If `~/.local/bin` is not on your `PATH`, the installer prints the line to add to `~/.profile`.

## After installing

    quickhop doctor               # platform, paths, session, helper tools, providers
    quickhop autostart on         # start the daemon at login (LaunchAgent or XDG autostart)
    quickhop daemon status

`quickhop autostart on` registers the binary you ran it with, so run it again after moving the
binary. With the `.deb`, `systemctl --user enable --now quickhop` is an alternative.

Then follow the [30-second tour](usage.md#a-30-second-tour).

## Update

Run the same command you installed with (`cargo install --git
https://github.com/yuyudhan/quickhop quickhop --force`, the newer `.deb`, or `install.sh`), then
restart the daemon so it runs the new binary:

    quickhop daemon stop
    quickhop daemon &          # or log out and in again with autostart on

## Uninstall

| Installed with | Remove with |
|----------------|-------------|
| `cargo install` | `quickhop autostart off; quickhop daemon stop; cargo uninstall quickhop` |
| the `.deb` | `quickhop autostart off; sudo apt remove quickhop` |
| `install.sh` | `scripts/install.sh --uninstall` (add `--purge` to delete config and data) |

Your settings stay in `~/.config/quickhop` and your history in `~/.local/share/quickhop` until
you delete them.
