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
| `crates/quickhop-index/tests/watch_prop.rs` | proptest: with the watcher running, every created file (any names, one or two new folder levels) is indexed once it settles, and deleted ones disappear while the rest stay. Walk, gitignore, search ranking and subtree removal are unit tests in `lib.rs`. |
| `crates/quickhop-index/tests/scale.rs` | Ignored by default: a 200,000-file tree must index in under 30 s and answer queries in under 20 ms. Run it on the Linux machine with `cargo test -p quickhop-index --release --test scale -- --ignored --nocapture`. |
| `crates/quickhop-providers/tests/providers/` | One test binary over `tests/fixtures/`: `contract.rs` (the `files` provider over an index of the fixture folder itself), `apps.rs`, `scripts.rs`, `raycast_goldens.rs`, `snapshots.rs` (with an empty file index, since paths depend on the checkout), shared builders in `common.rs`. |
| `crates/quickhop/tests/cli.rs` | End-to-end: the real binary with a sandboxed config and data dir (`QUICKHOP_CONFIG`, `QUICKHOP_DATA`, `HOME` pointing nowhere): `query --json` for apps, calculator, units, emoji, quicklinks; text output; `[[commands]]` aliases, switches, root order and unknown ids; `launch` confirmation, frecency recording, script commands, snippets (date format, arguments, options, nested snippets); `index` then `query` finding files by name and folder, `launch --action` copying a file's path and moving it to a sandboxed home trash (Linux); `run` by path, stem and title, `--yes`, the default `scripts/` directory; on macOS the move from `~/Library/Application Support/quickhop`. Reveal is checked by hand against a session bus (`dbus-run-session` with a stub `org.freedesktop.FileManager1`). |

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

A new provider adds one line to the `provider_contract!` list; the
`every_builtin_provider_has_a_contract_test` test fails until `BUILTIN` and the fixture set
agree.

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

- pre-commit: `just check` (format, guards, typos, host clippy, tests) on the staged tree.
- pre-push: `just verify` (check, clippy on three targets via zig, deny) on every commit
  being pushed.
- Both export the tree into `target/hook-tree/` (`scripts/hook-tree.sh`) and run there, so
  other people's uncommitted files in a shared clone never affect them.
