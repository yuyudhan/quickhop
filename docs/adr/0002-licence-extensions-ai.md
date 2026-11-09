# ADR 0002: MIT OR Apache-2.0, own extension API first, local AI by default

- Status: accepted
- Date: 2026-11-09

## Context

[plan.md](../plan.md) left three questions for the owner: the licence (the repository had none,
`LICENSE` said "to be decided"), whether to build the Raycast TypeScript compatibility shim (X3)
or only quickhop's own extension API (X1), and whether AI should default to local models or to
one hosted provider. The owner answered all three on 2026-11-09.

## Decision

- **Licence**: MIT OR Apache-2.0, at the user's option (answer 55, "MIT + Apache-2.0"). The
  texts are in `LICENSE-MIT` and `LICENSE-APACHE`; the workspace sets
  `license = "MIT OR Apache-2.0"` and every crate inherits it. Contributions are dual licensed
  the same way unless the contributor says otherwise.
- **Extensions**: quickhop's own language-neutral extension API (X1: JSON-RPC over stdio,
  `extension.toml`) comes first; the `@raycast/api` compatibility shim (X3, EXT-02) comes later
  and is built on top of X1 without changing it (answer 59, "own API first").
- **AI**: local models through Ollama are the default; users can add their own keys for
  OpenAI, Anthropic or any OpenAI-compatible endpoint, stored in the OS keyring. There is no
  hosted default (answer 60, "yes").

## Consequences

- The dual licence is the Rust ecosystem norm and is compatible with every dependency licence
  cargo-deny allows, including the MPL-2.0 and CC0-1.0 exceptions in `deny.toml`.
- The `.deb` and tarball from `scripts/bundle-linux.sh` ship both licence texts in
  `/usr/share/doc/quickhop/`.
- X3 stays in the plan, scheduled after X1.
- The AI client (I1) needs no account to work: with Ollama running locally, Quick AI answers
  out of the box; hosted providers are opt-in per key.
