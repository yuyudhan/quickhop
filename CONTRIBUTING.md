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
| `just shell` | shellcheck on `scripts/` and the hooks, then the installer's, install-from-source's, dev config's and macOS bundle's end-to-end tests |
| `just clippy-all` | clippy `-D warnings` for `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`, `aarch64-apple-darwin` |
| `just test` | all unit, integration, property, golden, snapshot and CLI e2e tests ([docs/testing.md](docs/testing.md)) |
| `just deny` | cargo-deny: advisories, licences, duplicate versions, sources |
| `just verify` | check, clippy-all, deny (pre-push) |
| `just ui-test` | the launcher window driven by real key events (macOS, idle Mac, Accessibility and Screen Recording granted) |
| `just bundle-macos-release` | `target/bundle/quickhop.app`, the release flavour |
| `just bundle-macos` | `target/bundle/quickhop Dev.app`, the development flavour |
| `just bundle-linux` | `.deb` and tarball in `target/bundle/` (needs `dpkg-dev`) |
| `just dev-config` | `config/dev/` into `~/.config/quickhop-dev/`; `--force` replaces files, backing each up |

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

## Minimum Rust version

`rust-version` in the workspace `Cargo.toml` (1.98 today) is the oldest Rust that builds the
tree; `cargo install --git` ignores `rust-toolchain.toml`, so this is what makes an older Rust
stop with `requires rustc 1.98 or newer` instead of failing deep inside gpui. The pinned gpui
(zed) revision sets the floor: moving the pin can raise it. When it changes, change together:

- `rust-version` in `Cargo.toml`;
- `MIN_RUST` in `scripts/install-from-source.sh` (`scripts/test-install-from-source.sh` fails
  when the two differ);
- every "Rust 1.98" and `requires rustc 1.98` in README.md, docs/install.md and
  docs/troubleshooting.md.

CI's MSRV job builds the workspace with exactly `rust-version` and fails when it is below the
toolchain of the pinned zed revision. `rust-toolchain.toml` pins the newer toolchain quickhop is
developed with; it is not the minimum.

## Development builds

Debug builds and `--features dev` are the development flavour
(`crates/quickhop-core/src/flavour.rs`): `cargo run`, `cargo test`, `just test`, `just ui-test`
and `just bundle-macos` (quickhop Dev.app) use `~/.config/quickhop-dev`,
`~/.local/share/quickhop-dev`, their own daemon socket, login entry, log, bundle identifier
(`io.github.yuyudhan.quickhop.dev`) and `Ctrl+Alt+Shift+Space`, so they run beside an installed
release. `just dev-config` installs `config/dev/` (every option set and commented) into
`~/.config/quickhop-dev`. Never point a development build at `~/.config/quickhop`.

**The release flavour's paths and identifiers never change**: `~/.config/quickhop`,
`~/.local/share/quickhop`, the runtime directory `quickhop`, `quickhop.app` and
`io.github.yuyudhan.quickhop` (bundle id, LaunchAgent label and Accessibility entry),
`quickhop.desktop` and `quickhop.service`. Users have no migration: changing one loses their
settings, history, login item or permission. `cargo test --release` checks the release
flavour's paths.

## Releases

A pushed tag `vX.Y.Z` runs `.github/workflows/release.yml`; the tag must equal
`[workspace.package] version` in `Cargo.toml`, or the run fails. It builds the universal macOS
archive with quickhop.app, the Linux x86_64 and aarch64 archives and `.deb`s (in
`debian:bullseye`: glibc 2.31 floor), `install.sh`, a Homebrew formula (Linux) and cask
(macOS), a `.sha256` per asset and `SHA256SUMS`, and publishes the GitHub release, whose notes
(`scripts/release-notes.sh`) carry the one-line install from docs/install.md and the version's
CHANGELOG.md section; a tag run fails early when that section is missing. Running the
workflow by hand (`workflow_dispatch`) is a dry run: everything is built and tested and
uploaded as a workflow artifact, with no release.

To release: rename `[Unreleased]` in CHANGELOG.md to `[X.Y.Z] - YYYY-MM-DD` (with its link at the
bottom), push that to `nightly`, then push the tag: `git tag vX.Y.Z && git push origin vX.Y.Z`.

Optional repository secrets:

| Secret | For |
|--------|-----|
| `MACOS_CERTIFICATE`, `MACOS_CERTIFICATE_PASSWORD`, `MACOS_SIGN_IDENTITY` | Developer ID signing of quickhop.app and the binary (hardened runtime). Without them both stay ad hoc; a partial set is an error. |
| `NOTARY_KEY`, `NOTARY_KEY_ID`, `NOTARY_ISSUER` | Notarization and stapling after signing (App Store Connect API key). |
| `HOMEBREW_TAP_TOKEN` | Fine-grained token with contents: write on `yuyudhan/homebrew-tap`; the tag run pushes `Formula/quickhop.rb` and `Casks/quickhop.rb` there. |

Homebrew stays out of the user docs until the tap is published. Once it is:

    brew install --cask yuyudhan/tap/quickhop     # macOS: quickhop.app and the quickhop command
    brew install yuyudhan/tap/quickhop            # Linux

To try the installer against a local build before tagging (macOS shown; on Linux pass
`--target x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu` and no `--app`):

    arm="$(scripts/release-build.sh aarch64-apple-darwin)"
    intel="$(scripts/release-build.sh x86_64-apple-darwin)"
    mkdir -p target/universal-apple-darwin
    lipo -create -output target/universal-apple-darwin/quickhop "$arm" "$intel"
    app="$(scripts/bundle-macos.sh --bin target/universal-apple-darwin/quickhop --out target/bundle)"
    scripts/dist.sh --target universal-apple-darwin --bin target/universal-apple-darwin/quickhop --app "$app"
    home="$(mktemp -d)"   # throwaway home and runtime dir: your install and daemon stay untouched
    HOME="$home" TMPDIR="$home/" XDG_RUNTIME_DIR="$home/run" QUICKHOP_APP_DIR="$home/Applications" \
        bash target/dist/install.sh --from target/dist

`scripts/dist.sh` refuses a macOS binary or app executable that is not universal. Every
user-facing change gets a line in [CHANGELOG.md](CHANGELOG.md) under `[Unreleased]`; at a
release that section becomes the version's.
