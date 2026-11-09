# AI in quickhop

quickhop's AI features are designed and scheduled ([plan.md](plan.md), slice I1); no quickhop
command calls a model yet, so nothing you type leaves your machine. This page records what was
decided and how the features will behave, so you can plan around them.

## The decision: local first, your own keys

From [ADR 0002](adr/0002-licence-extensions-ai.md), decided by the owner on 2026-11-09:

- **Local models through [Ollama](https://ollama.com) are the default.** With Ollama running
  on your machine, AI works with no account, no key and no network.
- **Hosted models are opt-in, with your own keys.** OpenAI, Anthropic, or any
  OpenAI-compatible endpoint (a self-hosted server, a gateway, another vendor), each enabled
  by adding its key.
- **Keys live in the OS keyring**, Keychain on macOS and Secret Service on Linux, never in
  `config.toml`.
- **There is no hosted default and no quickhop account.** quickhop never sends text to a
  provider you did not set up.

## The features

| Feature | What it does | Raycast equivalent |
|---------|--------------|--------------------|
| Quick AI (AI-01) | Press `Tab` in the root search to ask the typed text; the answer streams in | Quick AI |
| AI Chat (AI-02) | A chat window with history kept across restarts | AI Chat |
| AI Commands (AI-03) | Saved prompt templates using the snippet placeholders `{selection}`, `{clipboard}` and `{argument}` | AI Commands |
| AI in extensions (AI-04) | Extensions ask the model through a declared host capability | AI extensions API |

AI Commands reuse the placeholder engine snippets already use
([configuration.md](configuration.md#placeholders)), so a template like
`Fix the grammar of: {selection}` behaves the same in both. On macOS `{selection}` reads the
selected text through Accessibility; on Linux it reads the X11 `PRIMARY` selection or the
Wayland primary selection.

## Privacy

- Local models see your prompts only on your machine.
- A hosted provider sees exactly the prompt you send and the context you include
  (`{selection}`, `{clipboard}`), under that provider's terms.
- quickhop has no telemetry, with or without AI.
