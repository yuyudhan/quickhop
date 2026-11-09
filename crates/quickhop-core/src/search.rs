//! Runs a query over providers and ranks the results.
//!
//! Ranking, best first:
//! 1. [`Tier::Pinned`]: an alias equals the query or its first word.
//! 2. [`Tier::Computed`]: answers from computing providers (the calculator).
//! 3. [`Tier::Matched`]: nucleo fuzzy matches of the title, or of a keyword with
//!    [`KEYWORD_PENALTY`] (waived when the keyword equals the query).
//!
//! Within a tier the score is the fuzzy score plus `FRECENCY_WEIGHT * ln(1 + frecency)`,
//! where the caller supplies frecency per item (see `quickhop-store`). Ties keep candidate
//! order: provider registration order, then the order each provider returned.
use std::cmp::Ordering;
use std::collections::HashMap;

use serde::Serialize;

use crate::config::CommandSettings;
use crate::fuzzy::Matcher;
use crate::item::{Item, ItemId};
use crate::provider::{Cancel, Provider, ProviderError, Query};

/// Points subtracted when only a keyword, not the title, matched (unless the keyword is
/// exactly the query).
pub const KEYWORD_PENALTY: i64 = 16;

/// Points per unit of `ln(1 + frecency)`: one recent pick (frecency 1) adds about 17,
/// roughly one well-placed matched character.
pub const FRECENCY_WEIGHT: f64 = 24.0;

/// Frecency per item for the current query, as computed by the store.
pub type Boosts = HashMap<ItemId, f64>;

/// Rank class of a result; higher classes always rank first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// Fuzzy match of title or keyword.
    Matched,
    /// Computed answer.
    Computed,
    /// Exact alias hit.
    Pinned,
}

/// An item with its match quality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ranked {
    /// The matched item.
    pub item: Item,
    /// Rank class.
    pub tier: Tier,
    /// Score within the tier, frecency included.
    pub score: i64,
    /// Matched character indices in the title, for highlighting (empty for keyword matches).
    pub positions: Vec<usize>,
}

/// The outcome of one search: ranked items plus the providers that failed.
#[derive(Debug, Default)]
pub struct SearchResults {
    /// Best first, at most the search's limit.
    pub ranked: Vec<Ranked>,
    /// Providers that could not answer; their items are missing from `ranked`.
    pub errors: Vec<ProviderError>,
}

/// What a `[[commands]]` entry changes about its item.
#[derive(Debug, Clone)]
struct Override {
    alias: Option<String>,
    enabled: bool,
    order: Option<u32>,
}

/// A set of providers searched together.
pub struct Search {
    providers: Vec<Box<dyn Provider>>,
    limit: usize,
    commands: Vec<(ItemId, Override)>,
}

impl Search {
    /// A search returning at most `limit` items.
    pub fn new(limit: usize) -> Self {
        Self {
            providers: Vec::new(),
            limit,
            commands: Vec::new(),
        }
    }

    /// Applies per-command settings: an alias pins its item like any other alias, a
    /// disabled item never appears, and `order` sorts the empty root search. Entries whose
    /// id does not parse are ignored ([`crate::Config::validate`] rejects them).
    #[must_use]
    pub fn with_commands(mut self, commands: &[CommandSettings]) -> Self {
        self.commands = commands
            .iter()
            .filter_map(|c| {
                let id = ItemId::parse(&c.id)?;
                let settings = Override {
                    alias: c.alias.clone(),
                    enabled: c.enabled,
                    order: c.order,
                };
                Some((id, settings))
            })
            .collect();
        self
    }

    /// Adds a provider. Earlier providers win ties.
    #[must_use]
    pub fn with_provider(mut self, provider: Box<dyn Provider>) -> Self {
        self.providers.push(provider);
        self
    }

    /// Adds several providers, in order.
    #[must_use]
    pub fn with_providers(
        mut self,
        providers: impl IntoIterator<Item = Box<dyn Provider>>,
    ) -> Self {
        self.providers.extend(providers);
        self
    }

    /// Names of the registered providers, in order.
    pub fn provider_names(&self) -> impl Iterator<Item = &str> {
        self.providers.iter().map(|p| p.name())
    }

    /// The registered provider named `name`, if any: the launcher window searches it alone
    /// for a view of its items (all snippets, files).
    pub fn provider(&self, name: &str) -> Option<&dyn Provider> {
        self.providers
            .iter()
            .find(|p| p.name() == name)
            .map(AsRef::as_ref)
    }

    /// The `[[commands]]` ids that name no item: their provider is not registered, is a
    /// computed one, or has no such key ([`Provider::has_key`]). Providers that fail to
    /// answer are not judged. Asks every provider, so call it when commands are loaded, not
    /// per keystroke.
    pub fn unknown_commands(&self) -> Vec<ItemId> {
        self.commands
            .iter()
            .map(|(id, _)| id)
            .filter(|id| {
                self.providers
                    .iter()
                    .find(|p| p.name() == id.provider)
                    .is_none_or(|p| matches!(p.has_key(&id.key), Ok(false)))
            })
            .cloned()
            .collect()
    }

    /// Runs `query`. Stops asking providers once `cancel` is set; items with the same
    /// [`ItemId`] are merged, keeping the best rank.
    pub fn run(&self, query: &Query, cancel: &Cancel, boosts: &Boosts) -> SearchResults {
        let overrides: HashMap<&ItemId, &Override> =
            self.commands.iter().map(|(id, o)| (id, o)).collect();
        let mut errors = Vec::new();
        let mut candidates = Vec::new();
        for provider in &self.providers {
            if cancel.is_cancelled() {
                errors.push(ProviderError::Cancelled {
                    provider: provider.name().into(),
                });
                continue;
            }
            match provider.items(query, cancel) {
                Ok(items) => {
                    let computed = provider.is_computed();
                    candidates.extend(
                        items
                            .into_iter()
                            .filter_map(|item| apply(&overrides, item))
                            .map(|item| (item, computed)),
                    );
                }
                Err(error) => errors.push(error),
            }
        }
        let ranked = if query.is_empty() && overrides.values().any(|o| o.order.is_some()) {
            let mut all = rank(query, candidates, boosts, usize::MAX);
            // Stable: items without an order keep their rank after the ordered ones.
            all.sort_by_key(|r| {
                overrides
                    .get(&r.item.id)
                    .and_then(|o| o.order)
                    .map_or((1, 0), |order| (0, order))
            });
            all.truncate(self.limit);
            all
        } else {
            rank(query, candidates, boosts, self.limit)
        };
        SearchResults { ranked, errors }
    }
}

/// `item` with its `[[commands]]` alias added, or `None` when the command is disabled.
fn apply(overrides: &HashMap<&ItemId, &Override>, mut item: Item) -> Option<Item> {
    let Some(settings) = overrides.get(&item.id) else {
        return Some(item);
    };
    if !settings.enabled {
        return None;
    }
    if let Some(alias) = &settings.alias
        && !item.aliases.iter().any(|a| a.eq_ignore_ascii_case(alias))
    {
        item.aliases.push(alias.clone());
    }
    Some(item)
}

/// Ranks `candidates` (each with its provider's [`Provider::is_computed`]) for `query`
/// and keeps the best `limit`. Candidate order breaks ties.
pub fn rank(
    query: &Query,
    candidates: impl IntoIterator<Item = (Item, bool)>,
    boosts: &Boosts,
    limit: usize,
) -> Vec<Ranked> {
    let mut matcher = Matcher::new(query.text());
    let mut best: HashMap<ItemId, usize> = HashMap::new();
    let mut ranked: Vec<Ranked> = Vec::new();
    for (item, computed) in candidates {
        let Some(mut candidate) = classify(query, &mut matcher, item, computed) else {
            continue;
        };
        candidate.score = candidate
            .score
            .saturating_add(frecency_points(boosts.get(&candidate.item.id)));
        match best.get(&candidate.item.id) {
            Some(&index) if compare(&ranked[index], &candidate) != Ordering::Greater => {}
            Some(&index) => ranked[index] = candidate,
            None => {
                best.insert(candidate.item.id.clone(), ranked.len());
                ranked.push(candidate);
            }
        }
    }
    // Stable: equal keys keep candidate order.
    ranked.sort_by(compare);
    ranked.truncate(limit);
    ranked
}

/// `Less` when `a` ranks before `b`.
fn compare(a: &Ranked, b: &Ranked) -> Ordering {
    b.tier.cmp(&a.tier).then_with(|| b.score.cmp(&a.score))
}

fn frecency_points(frecency: Option<&f64>) -> i64 {
    let frecency = frecency
        .copied()
        .filter(|f| f.is_finite() && *f > 0.0)
        .unwrap_or(0.0);
    // Bounded: ln(1 + f64::MAX) * 24 is about 17000.
    #[allow(clippy::cast_possible_truncation)]
    let points = (FRECENCY_WEIGHT * frecency.ln_1p()).round() as i64;
    points
}

fn is_pinned(query: &Query, item: &Item) -> bool {
    if query.is_empty() {
        return false;
    }
    let whole = query.text().to_lowercase();
    let head = query.head();
    item.aliases.iter().any(|alias| {
        let alias = alias.to_lowercase();
        alias == whole || alias == head
    })
}

fn classify(query: &Query, matcher: &mut Matcher, item: Item, computed: bool) -> Option<Ranked> {
    let (tier, score, positions) = if is_pinned(query, &item) {
        (Tier::Pinned, 0, Vec::new())
    } else if computed {
        (Tier::Computed, 0, Vec::new())
    } else if matcher.is_empty() {
        (Tier::Matched, 0, Vec::new())
    } else {
        let title = matcher.score(&item.title);
        let whole = query.text();
        let keyword = item
            .keywords
            .iter()
            .filter_map(|k| {
                let m = matcher.score(k)?;
                let penalty = if k.eq_ignore_ascii_case(whole) {
                    0
                } else {
                    KEYWORD_PENALTY
                };
                Some(i64::from(m.score) - penalty)
            })
            .max();
        match (title, keyword) {
            (Some(t), Some(k)) if k > i64::from(t.score) => (Tier::Matched, k, Vec::new()),
            (Some(t), _) => (Tier::Matched, i64::from(t.score), t.positions),
            (None, Some(k)) => (Tier::Matched, k, Vec::new()),
            (None, None) => return None,
        }
    };
    Some(Ranked {
        item,
        tier,
        score,
        positions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Action;

    struct Fixed {
        name: &'static str,
        titles: Vec<(&'static str, &'static [&'static str])>,
        aliases: &'static [&'static str],
        computed: bool,
        fail: bool,
    }

    impl Provider for Fixed {
        fn name(&self) -> &str {
            self.name
        }

        fn items(&self, _query: &Query, _cancel: &Cancel) -> Result<Vec<Item>, ProviderError> {
            if self.fail {
                return Err(ProviderError::Other {
                    provider: self.name.into(),
                    message: "broken".into(),
                });
            }
            Ok(self
                .titles
                .iter()
                .map(|(title, keywords)| {
                    Item::new(
                        ItemId::new(self.name, *title),
                        *title,
                        Action::Open {
                            target: (*title).into(),
                        },
                    )
                    .with_keywords(keywords.iter().copied())
                    .with_aliases(self.aliases.iter().copied())
                })
                .collect())
        }

        fn is_computed(&self) -> bool {
            self.computed
        }
    }

    fn fixed(name: &'static str, titles: Vec<(&'static str, &'static [&'static str])>) -> Fixed {
        Fixed {
            name,
            titles,
            aliases: &[],
            computed: false,
            fail: false,
        }
    }

    fn apps() -> Box<dyn Provider> {
        Box::new(fixed(
            "apps",
            vec![
                ("Visual Studio Code", &["vscode", "editor"]),
                ("Firefox", &["browser"]),
                ("Files", &[]),
                ("Terminal", &["console"]),
            ],
        ))
    }

    fn titles(results: &SearchResults) -> Vec<&str> {
        results
            .ranked
            .iter()
            .map(|r| r.item.title.as_str())
            .collect()
    }

    fn run(search: &Search, text: &str) -> SearchResults {
        search.run(&Query::new(text), &Cancel::new(), &Boosts::new())
    }

    #[test]
    fn ranks_and_filters() {
        let results = run(&Search::new(10).with_provider(apps()), "fi");
        // Equal prefix scores: provider order decides.
        assert_eq!(titles(&results), vec!["Firefox", "Files"]);
        assert_eq!(results.ranked[0].score, results.ranked[1].score);
        assert_eq!(results.ranked[1].positions, vec![0, 1]);
    }

    #[test]
    fn keywords_match_without_positions() {
        let results = run(&Search::new(10).with_provider(apps()), "browser");
        assert_eq!(titles(&results), vec!["Firefox"]);
        assert_eq!(results.ranked[0].positions, Vec::<usize>::new());
    }

    #[test]
    fn empty_query_lists_all_up_to_limit() {
        let results = run(&Search::new(2).with_provider(apps()), "");
        assert_eq!(results.ranked.len(), 2);
    }

    #[test]
    fn computed_first_and_errors_reported() {
        let calc = Box::new(Fixed {
            computed: true,
            ..fixed("calc", vec![("= 4", &[])])
        });
        let broken = Box::new(Fixed {
            fail: true,
            ..fixed("broken", vec![])
        });
        let search = Search::new(10)
            .with_provider(apps())
            .with_provider(calc)
            .with_provider(broken);
        let results = run(&search, "2+2");
        assert_eq!(titles(&results), vec!["= 4"]);
        assert_eq!(results.ranked[0].tier, Tier::Computed);
        assert_eq!(results.errors.len(), 1);
    }

    #[test]
    fn exact_keyword_ties_with_a_title_match() {
        let lock = Box::new(fixed("system", vec![("Lock Screen", &[])]));
        let emoji = Box::new(fixed(
            "emoji",
            vec![("closed padlock", &["lock"]), ("lockable", &[])],
        ));
        let results = run(
            &Search::new(10).with_provider(lock).with_provider(emoji),
            "lock",
        );
        assert_eq!(titles(&results)[0], "Lock Screen");
        assert_eq!(results.ranked[0].score, results.ranked[1].score);
        assert_eq!(results.ranked[1].item.title, "closed padlock");
    }

    #[test]
    fn duplicate_ids_merge() {
        let results = run(
            &Search::new(10).with_provider(apps()).with_provider(apps()),
            "term",
        );
        assert_eq!(titles(&results), vec!["Terminal"]);
    }

    #[test]
    fn alias_pins_above_better_matches() {
        let links = Box::new(Fixed {
            aliases: &["g"],
            ..fixed("links", vec![("Search Google", &[])])
        });
        let search = Search::new(10).with_provider(apps()).with_provider(links);
        let results = run(&search, "g rust");
        assert_eq!(results.ranked[0].item.title, "Search Google");
        assert_eq!(results.ranked[0].tier, Tier::Pinned);
    }

    #[test]
    fn frecency_lifts_a_weaker_match() {
        let search = Search::new(10).with_provider(apps());
        assert_eq!(titles(&run(&search, "fi"))[0], "Firefox");
        let mut boosts = Boosts::new();
        boosts.insert(ItemId::new("apps", "Files"), 1.0);
        let results = search.run(&Query::new("fi"), &Cancel::new(), &boosts);
        assert_eq!(titles(&results), vec!["Files", "Firefox"]);
    }

    #[test]
    fn cancelled_search_asks_no_provider() {
        let cancel = Cancel::new();
        cancel.cancel();
        let results =
            Search::new(10)
                .with_provider(apps())
                .run(&Query::new("fi"), &cancel, &Boosts::new());
        assert_eq!(results.ranked.len(), 0);
        assert!(matches!(
            results.errors.as_slice(),
            [ProviderError::Cancelled { .. }]
        ));
    }

    #[test]
    fn non_finite_frecency_is_ignored() {
        assert_eq!(frecency_points(Some(&f64::NAN)), 0);
        assert_eq!(frecency_points(Some(&-3.0)), 0);
        assert_eq!(frecency_points(Some(&f64::INFINITY)), 0);
        assert_eq!(frecency_points(None), 0);
    }

    fn command(id: &str) -> CommandSettings {
        CommandSettings {
            id: id.into(),
            alias: None,
            enabled: true,
            hotkey: None,
            order: None,
        }
    }

    #[test]
    fn command_alias_pins_any_item() {
        let alias = CommandSettings {
            alias: Some("t".into()),
            ..command("apps:Terminal")
        };
        let search = Search::new(10)
            .with_provider(apps())
            .with_commands(&[alias]);
        for text in ["t", "T", "t some argument"] {
            let results = run(&search, text);
            assert_eq!(results.ranked[0].item.title, "Terminal", "{text}");
            assert_eq!(results.ranked[0].tier, Tier::Pinned, "{text}");
        }
        assert_eq!(search.unknown_commands(), Vec::<ItemId>::new());
    }

    #[test]
    fn disabled_command_never_appears() {
        let off = CommandSettings {
            enabled: false,
            alias: Some("ff".into()),
            ..command("apps:Firefox")
        };
        let search = Search::new(10).with_provider(apps()).with_commands(&[off]);
        for text in ["", "fi", "firefox", "browser", "ff"] {
            assert!(!titles(&run(&search, text)).contains(&"Firefox"), "{text}");
        }
        assert_eq!(titles(&run(&search, "fi")), vec!["Files"]);
    }

    #[test]
    fn unknown_command_ids_are_reported() {
        let calc = Box::new(Fixed {
            computed: true,
            ..fixed("calc", vec![("= 4", &[])])
        });
        let broken = Box::new(Fixed {
            fail: true,
            ..fixed("broken", vec![])
        });
        let commands = [
            command("apps:Firefox"),
            command("apps:Nope"),
            command("calc:= 4"),
            command("missing:x"),
            command("broken:x"),
        ];
        let search = Search::new(10)
            .with_provider(apps())
            .with_provider(calc)
            .with_provider(broken)
            .with_commands(&commands);
        let unknown: Vec<String> = search
            .unknown_commands()
            .iter()
            .map(ToString::to_string)
            .collect();
        // A failed provider is not judged.
        assert_eq!(unknown, vec!["apps:Nope", "calc:= 4", "missing:x"]);
    }

    #[test]
    fn command_order_sorts_the_empty_root_only() {
        let commands = [
            CommandSettings {
                order: Some(2),
                ..command("apps:Files")
            },
            CommandSettings {
                order: Some(1),
                ..command("apps:Terminal")
            },
        ];
        let search = Search::new(3)
            .with_provider(apps())
            .with_commands(&commands);
        assert_eq!(
            titles(&run(&search, "")),
            vec!["Terminal", "Files", "Visual Studio Code"]
        );
        assert_eq!(titles(&run(&search, "fi")), vec!["Firefox", "Files"]);
    }
}
