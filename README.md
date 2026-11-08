# quickhop

A keyboard launcher for Linux, macOS and Windows, in Rust: press a hotkey, type a few letters,
hit Enter to open an app, file, calculation or system command. A Raycast alternative.

Status: early. The core (items, actions, providers, fuzzy ranking, config) and the CLI work; the
providers and the launcher window are being built.

## Use

    cargo run -p quickhop -- --version
    cargo run -p quickhop -- config default    # print the default config.toml
    cargo run -p quickhop -- config path       # where the config is read from
    cargo run -p quickhop -- config check      # validate it (or pass a file)
    cargo run -p quickhop -- doctor            # platform, config and provider status

The config lives at `$XDG_CONFIG_HOME/quickhop/config.toml` (Linux),
`~/Library/Application Support/quickhop/config.toml` (macOS) or
`%APPDATA%\quickhop\config.toml` (Windows); `QUICKHOP_CONFIG` overrides it.

## Layout

See [docs/architecture.md](docs/architecture.md); contributing rules in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

Not chosen yet (see LICENSE).
