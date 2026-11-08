# Contributing

Work lands on `nightly` (the default branch). CI runs on Ubuntu, macOS and Windows for every push
and pull request to it.

## Setup

    rustup show                       # installs the pinned toolchain and targets
    cargo install just taplo-cli typos-cli cargo-deny --locked
    just install-hooks                # pre-commit: `just check`, pre-push: `just verify`

## Commands

| Command | Does |
|---------|------|
| `just fmt` | `cargo fmt` and `taplo fmt` in place |
| `just check` | format check, guards, typos, clippy `-D warnings` for `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`, `aarch64-apple-darwin` |
| `just test` | all unit, integration and property tests |
| `just deny` | cargo-deny: advisories, licences, duplicate versions, sources |
| `just verify` | check, test, deny |

## Hooks

Both hooks check the working tree, so they first require it to equal what is being committed or
pushed (`scripts/require-clean-tree.sh`): no unstaged changes, no untracked files, and for pushes
nothing staged but uncommitted. Never bypass them with `--no-verify`; fix the failure.

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
- **Tests** accompany every behaviour change; property tests (proptest) for invariants.
- **Platform code** lives in its platform crate and compiles on every OS (OS-specific modules are
  `cfg`-gated), so `cargo clippy --target` works from any machine.
- **Dependencies** must pass cargo-deny: permissive licences only, one version per crate.
