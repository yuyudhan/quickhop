//! Property tests for the fuzzy matcher and the ranking invariants.

use std::collections::HashSet;

use proptest::prelude::*;
use quickhop_core::{Action, Boosts, Item, ItemId, Query, Ranked, Tier, rank, score};

fn is_subsequence(query: &str, candidate: &str) -> bool {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let mut hay = candidate.chars().map(fold);
    query.chars().map(fold).all(|q| hay.any(|h| h == q))
}

fn item(index: usize, title: &str, aliases: &[String]) -> Item {
    Item::new(
        ItemId::new("p", format!("{index}")),
        title,
        Action::Copy { text: title.into() },
    )
    .with_aliases(aliases.iter().cloned())
}

fn candidates(titles: &[String], computed: &[bool]) -> Vec<(Item, bool)> {
    titles
        .iter()
        .enumerate()
        .map(|(i, t)| (item(i, t, &[]), computed.get(i).copied().unwrap_or(false)))
        .collect()
}

fn position(ranked: &[Ranked], id: &ItemId) -> Option<usize> {
    ranked.iter().position(|r| &r.item.id == id)
}

fn titles() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec("[a-eA-E ]{1,16}", 0..24)
}

proptest! {
    #[test]
    fn single_words_match_exactly_subsequences(query in "[a-dA-D]{1,6}", candidate in "[a-dA-D _-]{0,24}") {
        prop_assert_eq!(score(&query, &candidate).is_some(), is_subsequence(&query, &candidate));
    }

    #[test]
    fn positions_are_increasing_and_match(query in "[a-dA-D]{1,6}", candidate in "[a-dA-D _-]{0,24}") {
        if let Some(m) = score(&query, &candidate) {
            let chars: Vec<char> = candidate.chars().collect();
            prop_assert_eq!(m.positions.len(), query.chars().count());
            prop_assert!(m.positions.windows(2).all(|w| w[0] < w[1]));
            for (q, &p) in query.chars().zip(&m.positions) {
                prop_assert!(chars[p].eq_ignore_ascii_case(&q));
            }
        }
    }

    #[test]
    fn candidate_matches_itself(candidate in "[a-zA-Z][a-zA-Z _-]{0,24}") {
        prop_assert!(score(&candidate, &candidate).is_some());
    }

    #[test]
    fn results_are_sorted_unique_and_limited(
        titles in titles(),
        computed in prop::collection::vec(any::<bool>(), 0..24),
        query in "[a-e ]{0,4}",
        limit in 0usize..30,
    ) {
        let ranked = rank(&Query::new(&query), candidates(&titles, &computed), &Boosts::new(), limit);
        prop_assert!(ranked.len() <= limit);
        prop_assert!(ranked.windows(2).all(|w| (w[0].tier, w[0].score) >= (w[1].tier, w[1].score)));
        let ids: HashSet<_> = ranked.iter().map(|r| &r.item.id).collect();
        prop_assert_eq!(ids.len(), ranked.len());
    }

    #[test]
    fn matched_results_really_match(titles in titles(), query in "[a-e]{1,4}") {
        let query = Query::new(&query);
        for r in rank(&query, candidates(&titles, &[]), &Boosts::new(), usize::MAX) {
            prop_assert_eq!(r.tier, Tier::Matched);
            prop_assert!(score(query.text(), &r.item.title).is_some());
        }
    }

    #[test]
    fn empty_query_keeps_everything(titles in titles()) {
        let all = rank(&Query::new("  "), candidates(&titles, &[]), &Boosts::new(), usize::MAX);
        prop_assert_eq!(all.len(), titles.len());
    }

    #[test]
    fn exact_alias_is_always_first(titles in titles(), query in "[a-e]{1,4}", at in 0usize..24) {
        let mut list = candidates(&titles, &[]);
        let at = at.min(list.len());
        list.insert(at, (item(999, "zzz", std::slice::from_ref(&query)), false));
        let ranked = rank(&Query::new(&query), list, &Boosts::new(), 5);
        prop_assert_eq!(&ranked[0].item.id, &ItemId::new("p", "999"));
        prop_assert_eq!(ranked[0].tier, Tier::Pinned);
    }

    #[test]
    fn frecency_never_lowers_an_item(
        titles in titles(),
        query in "[a-e]{0,3}",
        pick in 0usize..24,
        boost in 0.0f64..1000.0,
    ) {
        prop_assume!(!titles.is_empty());
        let id = ItemId::new("p", format!("{}", pick % titles.len()));
        let query = Query::new(&query);
        let before = rank(&query, candidates(&titles, &[]), &Boosts::new(), usize::MAX);
        let mut boosts = Boosts::new();
        boosts.insert(id.clone(), boost);
        let after = rank(&query, candidates(&titles, &[]), &boosts, usize::MAX);
        prop_assert_eq!(before.len(), after.len());
        if let (Some(b), Some(a)) = (position(&before, &id), position(&after, &id)) {
            prop_assert!(a <= b, "moved from {} to {}", b, a);
        }
    }

    #[test]
    fn ranking_is_deterministic(titles in titles(), query in "[a-e ]{0,4}") {
        let query = Query::new(&query);
        let a = rank(&query, candidates(&titles, &[]), &Boosts::new(), 10);
        let b = rank(&query, candidates(&titles, &[]), &Boosts::new(), 10);
        prop_assert_eq!(a, b);
    }
}
