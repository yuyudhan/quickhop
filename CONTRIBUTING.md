# Contributing

Work lands on `nightly` (the default branch). CI runs on Ubuntu, macOS and Windows for every push
and pull request to it.

## Setup

    rustup show                       # installs the pinned toolchain and targets
    cargo install just taplo-cli typos-cli cargo-deny --locked
    just install-hooks                # pre-commit: `just check`, pre-push: `just verify`

`just check` cross-checks clippy for the other two targets; the bundled SQLite is C, so that
needs [zig](https://ziglang.org/download/) on `PATH` (`scripts/zig-tool.sh` wraps `zig cc`).

## Commands

| Command | Does |
|---------|------|
| `just fmt` | `cargo fmt` and `taplo fmt` in place |
| `just check` | format check, guards, typos, clippy `-D warnings` for `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`, `aarch64-apple-darwin` |
| `just test` | all unit, integration, property, snapshot and CLI e2e tests ([docs/testing.md](docs/testing.md)) |
| `just deny` | cargo-deny: advisories, licences, duplicate versions, sources |
| `just verify` | check, test, deny |

## Hooks

Both hooks gate exactly what is being recorded, not the working tree (`scripts/hook-tree.sh`):
pre-commit exports the index with `git checkout-index`, pre-push exports each pushed commit
with `git archive`, into `target/hook-tree/` (ignored by git), and runs the gate there with
the shared `target/` as `CARGO_TARGET_DIR`. Unstaged edits and untracked files, yours or
another contributor's in a shared clone, neither fail the hooks nor get checked by mistake.
Never bypass them with `--no-verify`; fix the failure.

## Rules

- **No unsafe code.** Every crate root has `#![forbid(unsafe_code)]` and the workspace sets
  `unsafe_code = "forbid"`; `scripts/check-unsafe.sh` enforces both. Should a platform API ever
  need FFI, it goes in a dedicated `quickhop-sys-*` crate listed as the only exception in that
  script, with a `// SAFETY:` comment on every block.
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
