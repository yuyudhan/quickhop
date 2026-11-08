# Architecture

## Crates

| Crate | Role |
|-------|------|
| `quickhop-core` | Platform-independent model: `Item`, `ItemId`, `Action`, `Query`, the `Provider` trait, the fuzzy matcher (`fuzzy::score`), `Search` (runs providers, dedupes, ranks, limits) and `Config` (TOML, per-OS path). |
| `quickhop-providers` | Built-in providers: `apps`, `files`, `calc`, `commands` (to be implemented, T91). |
| `quickhop-ui` | The launcher window. Toolkit not chosen yet; no code until then. |
| `quickhop` | The binary: `--version`, `config default\|path\|check`, `doctor`. |

## Search flow

1. The UI turns the typed text into a `Query`.
2. `Search::run` asks every enabled provider for candidate items. A failing provider is
   reported in `SearchResults::errors`; the others still answer.
3. Each item is scored by `fuzzy::score` against its title (positions kept for highlighting)
   and its keywords (small penalty). Computed providers (calculator) bypass matching and rank
   first.
4. Duplicates (same `ItemId`) keep their best score; results sort by score, then shorter
   title, then provider order; the list is cut to `max_results`.
5. Enter runs the item's first `Action` (`Open`, `Copy`, `Run`).

## Fuzzy scoring

Case-insensitive subsequence match, best alignment by dynamic programming: bonuses for the
first character, word starts and camel humps and consecutive runs; penalties for gaps and
(capped) leading characters. Property tests in `crates/quickhop-core/tests/fuzzy_prop.rs`.

## Rules

No unsafe code anywhere, 3000-line file limit, 4-space indentation: see CONTRIBUTING.md.
