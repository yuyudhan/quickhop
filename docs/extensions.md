# Extending quickhop

quickhop grows in three tiers, from the simplest to the richest. The first two work today; the
third is designed and scheduled ([plan.md](plan.md), slice X1).

| Tier | What | Language | Status |
|------|------|----------|--------|
| Script commands | Any executable with Raycast `@raycast.*` comments | Any | Works today: [script-commands.md](script-commands.md) |
| Built-in providers | Rust code inside quickhop, implementing `Provider` | Rust | Works today, for contributors |
| Extensions | Separate programs speaking JSON-RPC over stdio, with views | Any | Designed (X1) |

## Script commands: the way to extend quickhop today

For a command that does one thing and prints a result, a script command is all you need: no
SDK, no build step, and the same file works in Raycast. Put it in
`~/.config/quickhop/scripts`, make it executable, and it is searchable and runnable at once.
Arguments, confirmation, icons, working directory and all four output modes are supported;
the details and the full Raycast compatibility table are in
[script-commands.md](script-commands.md).

## Built-in providers

A provider answers searches from inside quickhop: `items(query, cancel)` returns ranked items,
each with actions (`open`, `copy`, `run`, `reveal`, `trash`, …). Applications, the calculator,
emoji, system commands, quicklinks, scripts, snippets and files are all providers in
`crates/quickhop-providers`. Every provider passes the shared contract tests
(`provider_contract!` in `tests/providers/contract.rs`): answers within 100 ms, the same ids
in the same order for the same query, non-empty keys and titles, at least one action per item,
and prompt cancellation. To add one, read [architecture.md](architecture.md),
[testing.md](testing.md) and [CONTRIBUTING.md](../CONTRIBUTING.md).

## Extensions: quickhop's own API

The owner decided to build quickhop's own, language-neutral extension API first, and a
compatibility layer for Raycast's TypeScript extensions after it, on top of it
([ADR 0002](adr/0002-licence-extensions-ai.md)). The design:

- **Out of process.** An extension is a program in any language, installed under
  `~/.local/share/quickhop/extensions/<id>/`. quickhop starts a command's process when it is
  run and supervises it; a crashed or slow extension fails only its own command, never the
  launcher.
- **A manifest, `extension.toml`,** declares the id, the title, the commands with their mode
  (`view`, `no-view` or `menu-bar`), the preferences, the capabilities it uses, and the
  executable with its arguments.
- **Newline-delimited JSON-RPC 2.0 over stdio.** The extension sends a view tree (`List`,
  `Detail`, `Form` or `Grid`, with sections, items, accessories and actions) and updates to
  it; quickhop sends the search text, selections, invoked actions and form values.
- **Declared capabilities.** Clipboard access, opening URLs, local storage, preferences,
  secrets in the OS keyring (Keychain, Secret Service), toasts and AI are host calls an
  extension may use only if its manifest declares them.

What this gives extension authors: write in the language you like, test your program by
piping JSON into it, and ship one folder that runs on macOS and Linux.

## Raycast extensions

Raycast's extensions are TypeScript and React against `@raycast/api`. The compatibility layer
(slice X3) is an `@raycast/api` implementation running on Node or Bun that translates the
React tree into the view tree above, so it changes nothing in quickhop's own API. APIs that
exist only on macOS (AppleScript, Finder selection) will work best-effort on macOS and be
unavailable on Linux. The feature-by-feature mapping is in [features.md](features.md#extensions-and-store).
