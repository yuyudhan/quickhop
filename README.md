# quickhop

A keyboard launcher for Linux, macOS and Windows, in Rust: press a hotkey, type a few letters,
hit Enter to open an app, file, calculation or system command. A Raycast alternative.

Status: early. The core (items, actions, fuzzy + frecency ranking, config), the providers (apps,
calculator and units, emoji, system commands, quicklinks, Raycast script commands) and the CLI
work; the launcher window is not built yet.

## Use

    cargo run -p quickhop -- query firefox           # ranked results, one per line
    cargo run -p quickhop -- query '5 km to miles' --json
    cargo run -p quickhop -- launch emoji:1f604 --query smile   # run the default action
    cargo run -p quickhop -- launch system:reboot --dry-run     # print it instead
    cargo run -p quickhop -- --version
    cargo run -p quickhop -- config default    # print the default config.toml
    cargo run -p quickhop -- config path       # where the config is read from
    cargo run -p quickhop -- config check      # validate it (or pass a file)
    cargo run -p quickhop -- doctor            # platform, config and provider status

The config lives at `$XDG_CONFIG_HOME/quickhop/config.toml` (Linux),
`~/Library/Application Support/quickhop/config.toml` (macOS) or
`%APPDATA%\quickhop\config.toml` (Windows); `QUICKHOP_CONFIG` overrides it. For example:

```toml
max_results = 8

[providers]
emoji = true      # apps, calc, emoji, system, quicklinks, scripts

[apps]
dirs = []         # empty: the OS defaults
icon_theme = ""   # empty: the GTK theme, else hicolor

[scripts]
dirs = ["/path/to/raycast-scripts"]   # none by default

[[quicklinks]]
name = "Google"
url = "https://www.google.com/search?q={query}"
alias = "g"       # `g rust` opens the search
```

Picks made with `launch` are remembered (frecency, half-life 7 days) in `frecency.sqlite` in
the data directory (`$XDG_DATA_HOME/quickhop`, `~/Library/Application Support/quickhop`,
`%LOCALAPPDATA%\quickhop`; `QUICKHOP_DATA` overrides). Power commands and scripts marked
`needsConfirmation` only run with `--yes`.

## Layout

See [docs/architecture.md](docs/architecture.md) and [docs/testing.md](docs/testing.md);
contributing rules in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

Not chosen yet (see LICENSE).
