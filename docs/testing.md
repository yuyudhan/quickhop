# Testing

`just test` runs everything below on the host; `just check` runs it after the format, guard,
spelling and clippy gates. CI runs `just check`'s steps on Ubuntu, macOS and Windows.

## Layout

| Where | What |
|-------|------|
| `crates/*/src/**` `#[cfg(test)]` | Unit tests next to the code: desktop-entry parsing, `Exec` field codes, icon-theme sizes, plist reading, script headers, frecency maths, config validation, the home trash (records, name clashes, URI escaping). |
| `crates/quickhop-core/tests/ranking_prop.rs` | proptest: fuzzy matches are exactly the subsequences, positions are increasing and correct; ranking is sorted, unique, limited and deterministic; matched results really match; an exact alias is always first; frecency never lowers an item. |
| `crates/quickhop-core/tests/placeholders_prop.rs` | proptest: placeholder expansion never panics on any text or placeholder-shaped noise, and unknown placeholders (any keyword, attributes and modifiers) stay verbatim. The goldens from Raycast's dynamic-placeholder manual (formats, the pattern alphabet, offsets, modifiers, arguments) are unit tests in `placeholders.rs` and `datefmt.rs`. |
| `crates/quickhop-store/tests/frecency_prop.rs` | proptest: decay is monotonic and composes, prefixes nest, more picks never score less. |
| `crates/quickhop-store/tests/clipboard_prop.rs` | proptest: after any sequence of copies and retention runs, `state.sqlite`'s clipboard history equals a plain list model (re-copies move to the front, expired and surplus entries go first, each text once). |
| `crates/quickhop/tests/clipboard_session.rs` | Ignored by default: `quickhop clipboard watch` in Xvfb with `xclip` (including a 300 KB `INCR` transfer) and in headless sway with `wl-copy` lists the copies newest first; in Xvfb a password manager's copy (an owner offering `x-kde-passwordManagerHint` beside the text, served by the test through x11rb) is never recorded; `clipboard paste` types into a focused GTK text window in Xvfb (`tests/fixtures/text_window.py` writes its text to a file) and, on sway, leaves the text on the clipboard and says so. Run it on Linux with `cargo test -p quickhop --test clipboard_session -- --ignored` (needs Xvfb, xclip, sway, wl-clipboard, python3-gi with GTK 3). |
| `crates/quickhop-index/tests/watch_prop.rs` | proptest: with the watcher running, every created file (any names, one or two new folder levels) is indexed once it settles, and deleted ones disappear while the rest stay. Walk, gitignore, search ranking and subtree removal are unit tests in `lib.rs`. |
| `crates/quickhop-index/tests/scale.rs` | Ignored by default: a 200,000-file tree must index in under 30 s and answer queries in under 20 ms. Run it on the Linux machine with `cargo test -p quickhop-index --release --test scale -- --ignored --nocapture`. |
| `scripts/bundle-linux-test.sh` | `just bundle-linux-test`: builds the `.deb` and installs it in a clean `debian:bookworm-slim` container (`QUICKHOP_TEST_IMAGE` picks another), then runs `quickhop doctor`, a query, and checks the desktop entry, icon and systemd user unit are in place. Needs docker. |
| `scripts/test-bundle-macos.sh` | Run by `just check` (the `shell` step) and CI on macOS, skipped elsewhere: builds `quickhop.app` around a stub binary with `scripts/bundle-macos.sh` and checks the `Info.plist` (identifier, executable, the binary's version, `LSUIElement`, icon file, minimum macOS), that the bundle runs the binary, that `AppIcon.icns` holds every size from 16 to 1024 px, the ad-hoc signature and its identifier, that a rebuild replaces the old bundle and that a missing `--bin` is refused, and that a development build (`--version` ending ` (dev)`) becomes quickhop Dev.app with the identifier `io.github.yuyudhan.quickhop.dev`, beside the release's app. |
| `scripts/test-install.sh` | Run by `just shell`: packages a stub binary with `scripts/dist.sh` and installs, runs, updates, uninstalls and purges it in a throwaway HOME, from a directory and over HTTP on 127.0.0.1; checks the refusals (musl, glibc older than 2.31, an HTTP 404, an empty download, a bad checksum, a binary this system cannot start, a non-universal macOS build), the macOS symlink into quickhop.app, and that a running daemon is stopped before an update and started again. It runs the documented one-line install (taken from docs/install.md, the same line as README.md's) from a scratch folder in bash and zsh against the local server, and checks that nothing is left in that folder or in TMPDIR after a success, a 404 for `install.sh`, a missing release asset, a bad checksum, and TERM or INT during a download. |
| `scripts/test-install-from-source.sh` | Run by `just shell`: `scripts/install-from-source.sh` against stub tools (rustc, cargo, rustup, the Command Line Tools, a C compiler, git, df) in an empty environment, checking each refusal, its message and whether cargo started, and that the script's minimum Rust equals the workspace `rust-version`. |
| `scripts/test-dev-config.sh` | Run by `just shell`: `just dev-config` into a fresh folder, a second run keeping the files, `--force` making exactly one backup per file, `XDG_CONFIG_HOME` followed, the release's `~/.config/quickhop` never written, bad arguments refused. |
| `crates/quickhop-providers/tests/providers/` | One test binary over `tests/fixtures/`: `contract.rs` (the `files` provider over a fixture `FileSource` and over quickhop's own index of the fixture folder), `apps.rs`, `scripts.rs`, `raycast_goldens.rs`, `snapshots.rs`, shared builders in `common.rs`. `spotlight.rs` searches the real Spotlight index: its fresh-file test waits for indexing, so it is ignored by default and run on the MacBook with `cargo test -p quickhop-providers --test providers spotlight -- --include-ignored`. |
| `crates/quickhop/tests/cli.rs` | End-to-end: the real binary with a sandboxed config and data dir (`QUICKHOP_CONFIG`, `QUICKHOP_DATA`, `HOME` pointing nowhere) and, on macOS, a private pasteboard (`QUICKHOP_PASTEBOARD`): `query --json` for apps, calculator, units, emoji, quicklinks; text output; `[[commands]]` aliases, switches, root order and unknown ids; `launch` confirmation, frecency recording, script commands, snippets (date format, arguments, options, nested snippets); `index` then `query` finding files by name and folder, `launch --action` copying a file's path and moving it to a sandboxed home trash (Linux); `clipboard add`, `list` (text and JSON), `[clipboard] max_entries` and `clear`, and `add` skipping what `wl-paste` calls `sensitive` (and `nil`, `clear`); on macOS `clipboard watch` over a private pasteboard listing copies newest first and never a concealed one; `paste` refusing a missing entry and a non-text result (before running it), and without a session on Linux (on macOS the private pasteboard is copied to, never pasted); on Unix the daemon over a private `QUICKHOP_RUNTIME` (a second daemon refused with the first one's pid, `status`, `open` through it, a broken config refused by `reload`, `stop` removing the socket, a killed daemon's stale socket replaced by the one `open` starts); `run` by path, stem and title, `--yes`, the default `scripts/` directory; on macOS the move from `~/Library/Application Support/quickhop`. Reveal is checked by hand against a session bus (`dbus-run-session` with a stub `org.freedesktop.FileManager1`). `cargo test` runs the development flavour; `cargo test --release` also tests the release flavour (release paths, the 0.1 migration). |
| `crates/quickhop/tests/first_run.rs` | End-to-end, Unix: `autostart on` registering the program and starting the daemon now (keeping one from the registered program, replacing one from another), the sway/i3/Hyprland/river exec line instead of an entry, `daemon --notify` from the app-menu entry, and what `doctor` says about them (the binary and its copies, the daemon line, a login entry pointing at a missing binary, this session's clipboard tool with the install command). Each test has its own home, config and runtime directory, so none touches the user's login items or daemon. |
| `crates/quickhop/tests/config_options.rs` | Every option of `config.toml` and `snippets.toml` through the real loader, each over a private config, home, data and runtime directory: a wrong type and an out-of-range value fail naming the key and line, unknown keys are refused in every table, the value changes what it controls, and the daemon applies it on `quickhop reload` (hotkey, file index, clipboard watcher and limits, `open`). `config/dev/config.toml` must set every key and pass `config check`. The live hotkey test is ignored by default (macOS). |
| `crates/quickhop-uitest/tests/window.rs` | Ignored by default, macOS (**[ax]**): the real daemon's window, driven by real key events. `quickhop toggle` shows it (created hidden, 600×300 at least, the empty root lists items) and hides it; the global hotkey (`Ctrl+Alt+Cmd+F13`, pressed only once `daemon status` says the daemon holds it) shows and hides it, and after `quickhop reload` with another `hotkey` the new one does and the old one shows nothing; typing searches, `Delete` edits, arrows move and wrap the selection; `⌘K` lists Calculator's actions and `Esc` closes the panel but keeps the window and query; `Esc` at the root hides it; `Return` opens Calculator (the test checks it runs, then quits it), and on `2+2` copies `4` to the private pasteboard; `screencapture -l` of the window has its size and is not one colour, and the middle of Calculator's icon slot has at least 16 colours once the snapshot says its icon is loaded. Run it on the MacBook with `just ui-test` from a terminal with the Accessibility and Screen Recording permissions, in the logged-in session (an ssh session cannot post events; a LaunchAgent with `LimitLoadToSessionType = Aqua` can), on a Mac nobody used in the last 60 s, with the screen unlocked (every test refuses while it is locked: its keys would go to the password field). |
| `crates/quickhop-uitest/tests/scripts.rs` | Ignored by default, macOS (**[ax]**), run like `window.rs`: script commands in the window over the fixture scripts in `tests/fixtures/scripts`. A space after the alias `sd` opens the fields, `Tab` moves to the dropdown and `Return` shows the compact script's toast with both values; `Return` with a required field empty focuses it and runs nothing; a fullOutput script opens its output page and `Esc` goes back to the query; red ANSI output is drawn red (red pixels counted in a capture); an inline script's subtitle changes only once the test socket moves the refresh clock past its `refreshTime`; a silent script hides the window and shows a small HUD that leaves the app in front where it is, then goes. |
| `crates/quickhop-uitest/tests/views.rs` | Ignored by default, macOS (**[ax]**), run like `window.rs`: the lists commands open. Clipboard History lists three copies newest first, typing filters them, and `Return` hides the window and, on the private pasteboard that is never pasted from, copies the entry and says so in the HUD; the paste test runs the daemon on the general pasteboard (it borrows the user's clipboard and puts every item back afterwards), opens a small Swift text window (`tests/fixtures/paste_target.swift`, built with `swiftc`) in front, and checks that the entry chosen in Clipboard History lands in it; Search Snippets lists a `snippets.toml` in file order and `Return` copies a `{date}` snippet expanded; Search Files lists nothing before typing, then finds a file `quickhop index` indexed, and `⌘K` → Copy to Clipboard copies its path. |
| `crates/quickhop-uitest/tests/onboarding.rs` | Ignored by default, macOS (**[ax]**), run like `window.rs`: the onboarding window. The test socket sets what it hears (`permission denied`, `granted`), since the terminal's daemon holds the permission. `quickhop onboarding` opens an ordinary window that says Accessibility is not allowed; granted meanwhile, it says so only once quickhop comes back to the front (another app in front, then the hotkey); its button (`Return`) brings System Settings to the front on the Accessibility pane (read through AX), which the test quits again (it refuses to start while System Settings is open); `Return` (Done) and `Esc` close it. The onboarding model's states and texts are unit tests in `quickhop-ui/src/onboarding.rs`. |
| `crates/quickhop-uitest/tests/app.rs` | Ignored by default, macOS (**[ax]**), run like `window.rs`: `scripts/bundle-macos.sh` bundles the binary under test, `open` opens it with private directories (`--env`), and the test checks the process is known by the bundle's identifier, runs the daemon with the configured hotkey, opens the onboarding window (a bundle signed a moment ago was never allowed Accessibility) and shows its launcher on the hotkey. The binary under test is a debug build, so it is bundled as quickhop Dev.app and its Accessibility grant is the development one. Which launches are the app is a unit test in `crates/quickhop/src/app.rs`. |

## Provider contract

`contract.rs` defines `check(&dyn Provider)` and the `provider_contract!` macro that turns
each `name => constructor` pair into a `#[test]`. Every provider fixture must:

- answer each probe query (empty, one letter, words, maths, unicode, a miss) within the
  100 ms budget (best of three runs);
- return the same ids in the same order for the same query, without duplicates, all under its
  own provider name, with non-empty keys and titles;
- accept an empty query, and return nothing for it when it is a computed provider;
- return `ProviderError::Cancelled` for a cancelled `Cancel`, within the budget;
- give every item at least one action and round-trip every item through JSON: actions are plain
  data, so describing one never runs it.

A new provider adds one line to the `provider_contract!` list, named after the provider (alone or
as `<provider>_<fixture>`); `every_provider_has_a_contract_test` fails while a provider in
`BUILTIN`, or one `builtin()` loads without a switch (the views commands), has no such line, or
while a `BUILTIN` provider is missing from the fixture set.

## Fixtures

`crates/quickhop-providers/tests/fixtures/`:

- `applications/`, `applications-system/`: desktop entries covering precedence (shadowed
  `firefox.desktop`), nested ids, localisation, keywords, field codes, `Terminal`, `NoDisplay`,
  `Hidden`, `OnlyShowIn`, `NotShowIn`, `TryExec`, `Type=Link`, and two broken files.
- `icons/`, `pixmaps/`: a `Fixture` theme inheriting `hicolor` with fixed, threshold and
  scalable directories, plus the unthemed fallback.
- `Applications/`: fake `.app` bundles (display name, nested `Utilities/`, icon file, a
  background agent, one without `Info.plist`); a binary plist is written to a tempdir.
- `StartMenu/`: empty `.lnk` files in folders, with an uninstaller to skip.
- `scripts/` (`/bin/sh`) and `scripts-windows/` (`.cmd` with `::` headers, `.ps1` for the
  argument and timeout cases, CRLF): the same Raycast scripts for every mode, arguments (text,
  dropdown, percent-encoded), `needsConfirmation`, `currentDirectoryPath`, failure, timeout,
  an invalid one, a hidden one and a non-script file.
- `snippets/snippets.toml`: a keyword, multi-line text, arguments, `{cursor}`, a nested
  `{snippet}`, a date format and clipboard modifiers.
- `raycast-goldens/`: Raycast's own script commands, unmodified (MIT,
  `LICENSE-raycast-script-commands`, from raycast/script-commands at `cf612c2`): the eight
  language templates and ten community commands covering every mode, `refreshTime`,
  `needsConfirmation`, `iconDark`, text and dropdown arguments, optional arguments, and an
  absent `packageName`. `raycast_goldens.rs` asserts Raycast's documented semantics and
  snapshots every parsed field; the loader test checks `*.template.*` files are skipped.

Providers take an explicit `Env` (OS + variables), so tests pin `linux` with an empty `PATH`
and never read the machine's apps, themes or locale. Both script sets are parsed on every OS; the
execution tests (and the CLI `launch` e2e) run the set for the host (`common::exec_scripts_dir`),
so Linux, macOS and Windows run the same execution tests.
`crates/quickhop/tests/fixtures/apps/` holds one Firefox for each OS's scanner so the same
e2e test passes on all three.

## Snapshots

`snapshots.rs` searches all providers together, as the CLI does, for 22 queries plus a
frecency case, and compares the rendered lists (tier, score, id, title, subtitle) with
`tests/providers/snapshots/*.snap` via insta. After an intended ranking change:

    INSTA_UPDATE=always cargo test -p quickhop-providers --test providers
    git diff crates/quickhop-providers/tests/providers/snapshots

and commit the reviewed `.snap` files. In CI (`CI` is set) insta never writes snapshots, so a
mismatch fails the run.

## Gates

- pre-commit: `just check` (format, guards, typos, shell, host clippy, tests) on the staged
  tree. `just shell` runs shellcheck on `scripts/` and the hooks, then `test-install.sh`,
  `test-install-from-source.sh`, `test-dev-config.sh` and `test-bundle-macos.sh` (above).
- `crates/quickhop-core/tests/config_docs.rs` fails when
  [configuration.md](configuration.md) misses a config or snippet key, lists one that does not
  exist, or shows a wrong default.
- pre-push: `just verify` (check, clippy for this OS's targets and Windows, deny) on every
  commit being pushed; it prints which targets it deferred to the other OS.
- Both export the tree into `target/hook-tree/` (`scripts/hook-tree.sh`) and run there, so
  other people's uncommitted files in a shared clone never affect them.
