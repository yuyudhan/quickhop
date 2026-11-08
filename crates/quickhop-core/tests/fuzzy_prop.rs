//! Property tests for the fuzzy matcher.

use proptest::prelude::*;
use quickhop_core::score;

fn is_subsequence(query: &str, candidate: &str) -> bool {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let mut hay = candidate.chars().map(fold);
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(fold)
        .all(|q| hay.any(|h| h == q))
}

proptest! {
    #[test]
    fn matches_exactly_subsequences(query in "[a-dA-D ]{0,6}", candidate in "[a-dA-D _-]{0,24}") {
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
        let m = score(&candidate, &candidate);
        prop_assert!(m.is_some());
    }
}
