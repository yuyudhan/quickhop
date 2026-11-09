# Contributing

Work lands on `nightly` (the default branch). CI runs on Ubuntu, macOS and Windows for every push
and pull request to it.

## Setup

    rustup show                       # installs the pinned toolchain and targets
    cargo install just taplo-cli typos-cli cargo-deny --locked
    brew install shellcheck           # or: sudo apt install shellcheck
    just install-hooks                # pre-commit: `just check`, pre-push: `just verify`

`just verify` runs clippy for every target this machine can check and names the rest:

- macOS checks `aarch64-apple-darwin` and cross-checks `x86_64-pc-windows-gnu`;
  `x86_64-unknown-linux-gnu` is deferred, since gpui's font libraries need a Linux sysroot.
- Linux checks `x86_64-unknown-linux-gnu` and cross-checks `x86_64-pc-windows-gnu`;
  `aarch64-apple-darwin` is deferred, since gpui's Metal shaders need the macOS SDK.

Run `just verify` on both before a push; CI lints every OS natively on every push. The Windows
cross-check needs [zig](https://ziglang.org/download/) on `PATH` (the bundled SQLite is C;
`scripts/zig-tool.sh` wraps `zig cc`) and mingw-w64's `windres` (`brew install mingw-w64`,
`apt install mingw-w64`). A missing tool fails the gate instead of skipping the target.

## Commands

| Command | Does |
|---------|------|
| `just fmt` | `cargo fmt` and `taplo fmt` in place |
| `just check` | format check, guards, typos, shell, clippy `-D warnings` for the host, all tests: the everyday gate (pre-commit) |
| `just shell` | shellcheck on `scripts/` and the hooks, then the installer's end-to-end test (`scripts/test-install.sh`) |
| `just clippy-all` | clippy `-D warnings` for `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`, `aarch64-apple-darwin` |
| `just test` | all unit, integration, property, golden, snapshot and CLI e2e tests ([docs/testing.md](docs/testing.md)) |
| `just deny` | cargo-deny: advisories, licences, duplicate versions, sources |
| `just verify` | check, clippy-all, deny (pre-push) |

## Hooks

Both hooks gate exactly what is being recorded, not the working tree (`scripts/hook-tree.sh`):
pre-commit exports the index with `git checkout-index`, pre-push exports each pushed commit
with `git archive`, into `target/hook-tree/` (ignored by git), and runs the gate there with
the shared `target/` as `CARGO_TARGET_DIR`. Unstaged edits and untracked files, yours or
another contributor's in a shared clone, neither fail the hooks nor get checked by mistake.
Never bypass them with `--no-verify`; fix the failure.

## Rules

- **Unsafe code only in platform crates.** Every crate root has `#![forbid(unsafe_code)]` and
  the workspace sets `unsafe_code = "forbid"`. FFI to AppKit/Accessibility, Wayland or X11
  lives in `crates/quickhop-platform-<os>` crates, which declare their own `[lints]` without
  the forbid and whose roots carry `#![deny(unsafe_op_in_unsafe_fn)]` and
  `#![deny(clippy::undocumented_unsafe_blocks)]`: one `unsafe` block per operation, each with
  a `// SAFETY:` comment. `scripts/check-unsafe.sh` enforces all of it.
- **Files stay under 3000 lines** (`scripts/check-line-count.sh`). Split modules before then.
- **4-space indentation everywhere**, no tabs: Rust (rustfmt.toml), TOML (taplo), YAML, Markdown
  and scripts (.editorconfig).
- **Lints** (workspace `[lints]`): clippy pedantic; `unwrap_used`, `expect_used` and `panic` denied
  outside tests; `undocumented_unsafe_blocks`; `missing_docs` on every public item; `todo!` and
  `unimplemented!` denied. Fix the code instead of adding `#[allow]`.
- **Tests** accompany every behaviour change; property tests (proptest) for invariants; every
  provider passes the contract in `crates/quickhop-providers/tests/providers/contract.rs`.
  See [docs/testing.md](docs/testing.md).
- **Platform code** lives in its platform crate and compiles on every OS (OS-specific modules are
  `cfg`-gated), so `cargo clippy --target` works from any machine.
- **Dependencies** must pass cargo-deny: permissive licences only, one version per crate. The
  one exception, nucleo-matcher (MPL-2.0), is allowed for that crate alone in `deny.toml`.
