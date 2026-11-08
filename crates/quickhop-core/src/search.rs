//! Runs a query over providers and ranks the results.

use std::collections::HashMap;

use crate::fuzzy::score;
use crate::item::{Item, ItemId};
use crate::provider::{Provider, ProviderError, Query};

/// Penalty applied when only a keyword, not the title, matched.
const KEYWORD_PENALTY: i64 = 8;

/// An item with its match quality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ranked {
    /// The matched item.
    pub item: Item,
    /// Match score; computed items rank above every fuzzy match.
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

/// A set of providers searched together.
pub struct Search {
    providers: Vec<Box<dyn Provider>>,
    limit: usize,
}

impl Search {
    /// A search returning at most `limit` items.
    pub fn new(limit: usize) -> Self {
        Self {
            providers: Vec::new(),
            limit,
        }
    }

    /// Adds a provider. Earlier providers win ties.
    #[must_use]
    pub fn with_provider(mut self, provider: Box<dyn Provider>) -> Self {
        self.providers.push(provider);
        self
    }

    /// Names of the registered providers, in order.
    pub fn provider_names(&self) -> impl Iterator<Item = &str> {
        self.providers.iter().map(|p| p.name())
    }

    /// Runs `query`. Items with the same [`ItemId`] are merged, keeping the best score.
    pub fn run(&self, query: &Query) -> SearchResults {
        let mut results = SearchResults::default();
        let mut best: HashMap<ItemId, usize> = HashMap::new();
        let mut ranked: Vec<Ranked> = Vec::new();
        for provider in &self.providers {
            let items = match provider.items(query) {
                Ok(items) => items,
                Err(error) => {
                    results.errors.push(error);
                    continue;
                }
            };
            for item in items {
                let Some(candidate) = rank(query, item, provider.is_computed()) else {
                    continue;
                };
                match best.get(&candidate.item.id) {
                    Some(&index) if ranked[index].score >= candidate.score => {}
                    Some(&index) => ranked[index] = candidate,
                    None => {
                        best.insert(candidate.item.id.clone(), ranked.len());
                        ranked.push(candidate);
                    }
                }
            }
        }
        // Stable: equal keys keep provider order.
        ranked.sort_by(|a, b| {
            b.score.cmp(&a.score).then_with(|| {
                a.item
                    .title
                    .chars()
                    .count()
                    .cmp(&b.item.title.chars().count())
            })
        });
        ranked.truncate(self.limit);
        results.ranked = ranked;
        results
    }
}

fn rank(query: &Query, item: Item, computed: bool) -> Option<Ranked> {
    if computed {
        return Some(Ranked {
            item,
            score: i64::MAX,
            positions: Vec::new(),
        });
    }
    let title = score(query.text(), &item.title);
    let keyword = item
        .keywords
        .iter()
        .filter_map(|k| score(query.text(), k))
        .map(|m| m.score - KEYWORD_PENALTY)
        .max();
    match (title, keyword) {
        (Some(t), Some(k)) if k > t.score => Some(Ranked {
            item,
            score: k,
            positions: Vec::new(),
        }),
        (Some(t), _) => Some(Ranked {
            item,
            score: t.score,
            positions: t.positions,
        }),
        (None, Some(k)) => Some(Ranked {
            item,
            score: k,
            positions: Vec::new(),
        }),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Action;

    struct Fixed {
        name: &'static str,
        titles: Vec<(&'static str, &'static [&'static str])>,
        computed: bool,
        fail: bool,
    }

    impl Provider for Fixed {
        fn name(&self) -> &str {
            self.name
        }

        fn items(&self, _query: &Query) -> Result<Vec<Item>, ProviderError> {
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
                })
                .collect())
        }

        fn is_computed(&self) -> bool {
            self.computed
        }
    }

    fn apps() -> Box<dyn Provider> {
        Box::new(Fixed {
            name: "apps",
            titles: vec![
                ("Visual Studio Code", &["vscode", "editor"]),
                ("Firefox", &["browser"]),
                ("Files", &[]),
                ("Terminal", &["console"]),
            ],
            computed: false,
            fail: false,
        })
    }

    fn titles(results: &SearchResults) -> Vec<&str> {
        results
            .ranked
            .iter()
            .map(|r| r.item.title.as_str())
            .collect()
    }

    #[test]
    fn ranks_and_filters() {
        let results = Search::new(10).with_provider(apps()).run(&Query::new("fi"));
        assert_eq!(titles(&results), vec!["Files", "Firefox"]);
        assert_eq!(results.ranked[0].positions, vec![0, 1]);
    }

    #[test]
    fn keywords_match_without_positions() {
        let results = Search::new(10)
            .with_provider(apps())
            .run(&Query::new("browser"));
        assert_eq!(titles(&results), vec!["Firefox"]);
        assert_eq!(results.ranked[0].positions, Vec::<usize>::new());
    }

    #[test]
    fn empty_query_lists_all_up_to_limit() {
        let results = Search::new(2).with_provider(apps()).run(&Query::new(""));
        assert_eq!(results.ranked.len(), 2);
    }

    #[test]
    fn computed_first_and_errors_reported() {
        let calc = Box::new(Fixed {
            name: "calc",
            titles: vec![("= 4", &[])],
            computed: true,
            fail: false,
        });
        let broken = Box::new(Fixed {
            name: "broken",
            titles: vec![],
            computed: false,
            fail: true,
        });
        let results = Search::new(10)
            .with_provider(apps())
            .with_provider(calc)
            .with_provider(broken)
            .run(&Query::new("2+2"));
        assert_eq!(titles(&results), vec!["= 4"]);
        assert_eq!(results.errors.len(), 1);
    }

    #[test]
    fn duplicate_ids_merge() {
        let results = Search::new(10)
            .with_provider(apps())
            .with_provider(apps())
            .run(&Query::new("term"));
        assert_eq!(titles(&results), vec!["Terminal"]);
    }
}
