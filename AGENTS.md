# AGENTS.md

Guide for coding agents working on quickhop, an open-source Raycast alternative in Rust for
macOS and Linux. Read [docs/architecture.md](docs/architecture.md) before changing structure,
[docs/features.md](docs/features.md) for what exists, and [docs/plan.md](docs/plan.md) for
the slice you are working on.

## Layout

- `crates/quickhop-core`: model, ranking, config and paths. No IO besides config files.
- `crates/quickhop-providers`: built-in providers; every provider takes an explicit `Env`.
- `crates/quickhop-store`: SQLite (frecency, later clipboard/snippets/chats).
- `crates/quickhop`: the CLI binary (`query`, `launch`, `run`, `config`, `doctor`).
- `crates/quickhop-ui`: the gpui window ([ADR 0001](docs/adr/0001-ui-toolkit.md)).
- `crates/quickhop-platform-*` (planned): the only crates allowed to use `unsafe`.

## Commands

    just check     # the gate: fmt check, guards, typos, clippy -D warnings, all tests
    just fmt       # cargo fmt + taplo fmt
    just test      # tests only
    just verify    # check + clippy for this OS's targets and Windows + cargo-deny

Run `just check` before every commit; it must be green on macOS and on Linux. `just verify`
checks Linux clippy only on Linux and macOS clippy only on macOS, so run it on both before a
push. Snapshot changes: `INSTA_UPDATE=always cargo test -p quickhop-providers --test
providers`, then review the `.snap` diff and commit it with the change that caused it.

## Rules

- Config lives in `~/.config/quickhop/` on macOS and Linux (`XDG_CONFIG_HOME` honoured), data
  in `~/.local/share/quickhop/`. Never introduce `~/Library/Application Support` paths.
- Debug builds and `--features dev` are the development flavour
  (`crates/quickhop-core/src/flavour.rs`, `~/.config/quickhop-dev`): never point them at
  `~/.config/quickhop`. The release flavour's paths and identifiers must not change (users
  have no migration); see [CONTRIBUTING.md](CONTRIBUTING.md#development-builds).
- The minimum Rust is `rust-version` in `Cargo.toml`; raising it means updating
  `scripts/install-from-source.sh` and the install docs too
  ([CONTRIBUTING.md](CONTRIBUTING.md#minimum-rust-version)).
- User-facing changes get a line under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md).
- Script commands stay compatible with Raycast's `@raycast.*` format; the goldens in
  `crates/quickhop-providers/tests/fixtures/raycast-goldens/` are Raycast's own files and are
  never edited.
- No `unsafe` outside platform crates; no `unwrap`/`expect`/`panic!` outside tests; every
  public item documented; clippy pedantic clean. Fix the code, do not add `#[allow]`.
- Tests accompany every behaviour change: unit tests next to the code, proptest for invariants,
  the provider contract for providers, `crates/quickhop/tests/cli.rs` for CLI behaviour. Tests
  never read the real home directory: use `Env::new`, `QUICKHOP_CONFIG`, `QUICKHOP_DATA` and
  temp dirs.
- Files stay under 3000 lines; 4-space indentation everywhere.
- Platform behaviour goes behind a trait with a fake, so the logic is tested on every OS.
- When a feature's status changes, update its row in `docs/features.md` (and the totals), and
  the README parity table.

## Machines

macOS work and macOS UI tests run on the MacBook M4; Linux work and Linux UI tests (Xvfb,
headless sway, headless mutter) on itlinux3. Move commits between machines with `git bundle`.

## Commits

Small commits, one concern each, message prefixed with a gitmoji as in the history
(`✨` feature, `🐛` fix, `✅` tests, `📝` docs, `🔧` config/tooling, `♻️` refactor). Commit
only; pushing needs the owner's go-ahead.
