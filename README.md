<div align="center">

# quickhop

**Press a key. Type a few letters. Be there.**

```sh
curl -fsSL https://yuyudhan.github.io/quickhop/install | sh
```

Installs quickhop (and quickhop.app on macOS) for your user on macOS or Linux, verifies its
SHA-256, no sudo. [Other ways to install](docs/install.md)

A fast, open-source, keyboard-first launcher for **macOS and Linux**, written in Rust.<br>
Apps, files, calculations, clipboard history, snippets and scripts, all from one place.<br>
A free alternative to Raycast that runs your Raycast script commands unchanged.

<img src="docs/media/quickhop-cli.gif" alt="quickhop in a terminal: converting 5 km to miles, finding Lock Screen, a GitHub quicklink, running a script command, listing clipboard history and expanding a snippet" width="760">

[Install](#install) · [First steps](#first-steps) · [Raycast scripts](#raycast-script-commands) · [Configure](#configure) · [Docs](docs/README.md)

</div>

## Install

Two ways in: a prebuilt release, which needs no Rust, or a build from source with Rust 1.98 or
newer ([details](docs/install.md#requirements)).

**Prebuilt** (releases from v0.1.0): macOS on Apple silicon or Intel (one universal build),
and Linux on x86_64 or aarch64 with glibc 2.31 or later (Debian 11, Ubuntu 20.04, Fedora 32,
RHEL 9 or newer; not musl/Alpine), in any shell:

```sh
curl -fsSL https://yuyudhan.github.io/quickhop/install | sh
```

The installer checks your system before downloading anything and says what to do when
quickhop's build cannot run there, verifies the download's SHA-256 against the release's
`SHA256SUMS`, and installs `quickhop` into `~/.local/bin` without `sudo` (on macOS also
`quickhop.app`), working in a temporary folder it always removes. Run it again to update or
reinstall; options go after `sh -s --`, as in `… | sh -s -- --version 0.1.0`. Every failure
is one `quickhop: error:` line saying what happened and what to do.

**From source**: Rust 1.98 or newer from [rustup](https://rustup.rs), a C compiler and about
3 GB of free disk; the first build takes several minutes. Already have Rust? Run
`rustup update stable` first: an older Rust stops at once with `requires rustc 1.98`, and
Rust from a distribution package (`apt install rustc`) is usually older still.

macOS:

```sh
xcode-select --install          # C compiler; wait until its installer finishes
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh
sh rustup-init.sh               # skip both lines if rustup is already installed
. "$HOME/.cargo/env"            # or open a new terminal
rustup update stable            # quickhop needs Rust 1.98 or newer
cargo install --locked --git https://github.com/yuyudhan/quickhop quickhop
```

Linux (Debian and Ubuntu shown; Fedora and Arch in [docs/install.md](docs/install.md#from-source)):

```sh
sudo apt install build-essential xdg-utils wl-clipboard xclip
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh
sh rustup-init.sh               # skip both lines if rustup is already installed
. "$HOME/.cargo/env"            # or open a new terminal
rustup update stable            # quickhop needs Rust 1.98 or newer
cargo install --locked --git https://github.com/yuyudhan/quickhop quickhop
```

To have every requirement checked first, with the command that fixes whatever is missing, run
`scripts/install-from-source.sh` from a clone ([troubleshooting](docs/troubleshooting.md#installing)).

Then check the setup and start the background daemon, now and at every login:

```sh
quickhop doctor
quickhop autostart on
```

`autostart on` starts the daemon at once, so the hotkey and clipboard history work right away.
On sway, i3, Hyprland and river, which do not run XDG autostart entries, it starts the daemon
for this session and prints the exec line to add to the compositor's config instead; add it
there so the next login starts quickhop too.

The `.deb`, the macOS app (`quickhop.app`, from `just bundle-macos-release` in a clone), every
release asset, updating and uninstalling: [docs/install.md](docs/install.md).

## First steps

```sh
quickhop query 5 km to miles               # calculator and units
quickhop query lock screen                 # apps, system commands, emoji, files, ...
quickhop run hello Ada                     # run a Raycast script command
quickhop clipboard list                    # clipboard history, newest first
quickhop launch snippets:reply --arg Ada   # expand a snippet
```

Every result line ends with its id, such as `[system:lock]`; `quickhop launch <id>` runs it,
and `--paste` sends text results straight into the focused app. The daemon records your
clipboard and keeps the file index current. The full tour, with every command:
[docs/usage.md](docs/usage.md).

On macOS the daemon also shows the launcher window when you press the global hotkey
(`Alt+Shift+Space`, so Raycast keeps its `Alt+Space`) or run `quickhop toggle`; opening
`quickhop.app` starts it, and until quickhop may paste into other apps its onboarding window
shows where to allow that: System Settings › Privacy & Security › Accessibility (Device
Control and Data Access on macOS 27 and later).
The Linux window is being built ([plan](docs/plan.md)). Everything else on this page works
from the command line on both.

## What works today

| | macOS | Linux |
|---|:---:|:---:|
| Fuzzy search that learns from what you pick | ✅ | ✅ |
| Applications, with icons | ✅ | ✅ |
| Calculator and unit conversion | ✅ | ✅ |
| Emoji search | ✅ | ✅ |
| System commands: lock, log out, sleep, restart, shut down | ✅ | ✅ |
| Quicklinks with `{query}` and aliases | ✅ | ✅ |
| Raycast script commands: all metadata, modes and arguments | ✅ | ✅ |
| File and folder search, with reveal, copy path and trash | ✅ Spotlight or own index | ✅ own index |
| Clipboard history, deduplicated, password-manager copies skipped | ✅ | ✅ X11, wlroots, KDE, COSMIC |
| Paste into the focused app | ✅ | ✅ X11 |
| Snippets with Raycast's dynamic placeholders | ✅ | ✅ |
| Aliases, ordering and on/off switches per result | ✅ | ✅ |
| Background daemon and launch at login | ✅ | ✅ |
| Launcher window with a global hotkey and an action panel | ✅ | next |

What each feature uses on each platform: [docs/platform-support.md](docs/platform-support.md).
All 95 Raycast features and their status: [docs/features.md](docs/features.md).

## Raycast script commands

Any executable with `@raycast.*` comments is a quickhop command. Drop it in
`~/.config/quickhop/scripts`:

```bash
#!/usr/bin/env bash
# @raycast.schemaVersion 1
# @raycast.title Hello
# @raycast.mode compact
# @raycast.icon 👋
# @raycast.argument1 { "type": "text", "placeholder": "Name", "optional": true }

echo "Hello, ${1:-world}!"
```

```sh
chmod +x ~/.config/quickhop/scripts/hello.sh
quickhop run hello Ada        # Hello, Ada!
```

Every field Raycast documents is supported: `title`, `mode` (`fullOutput`, `compact`,
`silent`, `inline`), `packageName`, `icon`, `iconDark`, `description`, `author`, `authorURL`,
`currentDirectoryPath`, `needsConfirmation`, `refreshTime` and up to three `text`, `password` or
`dropdown` arguments. The field-by-field table and the few differences:
[docs/script-commands.md](docs/script-commands.md).

## Why quickhop

| | quickhop | Raycast |
|---|---|---|
| Platforms | macOS and Linux (X11 and Wayland) | macOS and Windows |
| Source | Open source, MIT or Apache-2.0 | Proprietary |
| Settings | Plain TOML in `~/.config/quickhop`, easy to keep in git | In-app settings; cloud sync with a Raycast account |
| Account | None, and no telemetry | A Raycast account for sync and Pro features |
| Script commands | Raycast's format, unchanged | Raycast's format |
| AI (designed) | Local models through Ollama by default, or your own OpenAI, Anthropic or compatible keys ([docs/ai.md](docs/ai.md)) | Raycast AI, part of Raycast Pro |
| Extensions (designed) | Any language, JSON-RPC over stdio ([docs/extensions.md](docs/extensions.md)) | TypeScript and React |

## Configure

Everything is optional plain text, in the same place on macOS and Linux:

    ~/.config/quickhop/
    ├── config.toml       # settings, providers, quicklinks, per-result settings
    ├── snippets.toml     # snippets
    └── scripts/          # Raycast-compatible script commands

```toml
# ~/.config/quickhop/config.toml
max_results = 8

[providers]
emoji = false                       # turn any source of results off

[files]
roots = ["~/code", "~/Documents"]   # what the file index covers (default: your home)

[[quicklinks]]
name = "GitHub search"
url = "https://github.com/search?q={query}"
alias = "gh"                        # `gh tokio` searches GitHub

[[commands]]
id = "system:lock"                  # any result, by its id
alias = "lk"
```

```toml
# ~/.config/quickhop/snippets.toml
[[snippets]]
name = "Reply"
keyword = ";re"
text = "Hi {argument name=\"name\"}, thanks for the report. Fixed on {date format=\"EEEE, MMM d\"}."
```

`quickhop config default` prints every setting with its default, and `quickhop config check`
validates your file. Every key, every placeholder: [docs/configuration.md](docs/configuration.md).

## Uninstall

```sh
quickhop autostart off          # first, so no login entry is left behind
quickhop daemon stop
cargo uninstall quickhop        # or: curl -fsSL https://yuyudhan.github.io/quickhop/install | sh -s -- --uninstall
tccutil reset Accessibility io.github.yuyudhan.quickhop   # macOS
```

Your settings stay in `~/.config/quickhop` and your history in `~/.local/share/quickhop`
until you delete them. A login entry left by a removed quickhop shows up as a `warn:` line in
`quickhop doctor`. For the `.deb` and quickhop.app, see
[docs/install.md](docs/install.md#uninstall).

## Troubleshooting

- **`command not found`**: add `~/.cargo/bin` (or `~/.local/bin` for the installer) to your
  `PATH`.
- **Clipboard history stays empty**: run `quickhop autostart on`, then `quickhop daemon status`
  says what is watched and why not. GNOME on Wayland gives no app the clipboard.
- **`copied, not pasted`**: on macOS, allow the app running quickhop in System Settings ›
  Privacy & Security › Accessibility (Device Control and Data Access on macOS 27 and later);
  `quickhop onboarding` leads there. On Wayland, press Ctrl+V yourself.
- **A setting is ignored**: `quickhop config check`, then `quickhop reload` for a running
  daemon.

`quickhop doctor` shows every path, tool and provider. Every error message and its fix:
[docs/troubleshooting.md](docs/troubleshooting.md).

## Documentation

[Install](docs/install.md) · [Usage](docs/usage.md) · [Configuration](docs/configuration.md) ·
[Script commands](docs/script-commands.md) · [Platform support](docs/platform-support.md) ·
[Troubleshooting](docs/troubleshooting.md) · [Extensions](docs/extensions.md) · [AI](docs/ai.md) ·
[Architecture](docs/architecture.md) · [Features](docs/features.md) · [Plan](docs/plan.md) ·
[Changelog](CHANGELOG.md) · [All docs](docs/README.md)

## Contributing

Issues and pull requests are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md) (setup,
the `just check` gate, the rules), [docs/architecture.md](docs/architecture.md) and
[docs/testing.md](docs/testing.md). Coding agents: [AGENTS.md](AGENTS.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.

Raycast's example scripts used as test fixtures are MIT-licensed by Raycast Technologies Ltd.
