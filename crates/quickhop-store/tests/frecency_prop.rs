//! Property tests for frecency decay and accumulation.

use proptest::prelude::*;
use quickhop_core::ItemId;
use quickhop_store::{Store, decay, prefixes};

proptest! {
    #[test]
    fn decay_is_monotonic_in_time(value in 0.0f64..1e6, a in 0i64..10_000_000, b in 0i64..10_000_000) {
        let (early, late) = (a.min(b), a.max(b));
        prop_assert!(decay(value, 0, late) <= decay(value, 0, early) + 1e-9);
        prop_assert!(decay(value, 0, early) <= value + 1e-9);
    }

    #[test]
    fn decay_composes(value in 0.0f64..1e6, a in 0i64..5_000_000, b in 0i64..5_000_000) {
        let stepwise = decay(decay(value, 0, a), a, a + b);
        prop_assert!((stepwise - decay(value, 0, a + b)).abs() <= 1e-6 * value.max(1.0));
    }

    #[test]
    fn every_prefix_is_a_prefix(query in "[a-zA-Z ]{0,40}") {
        let all = prefixes(&query);
        prop_assert_eq!(&all[0], "");
        prop_assert!(all.windows(2).all(|w| w[1].starts_with(&w[0]) && w[1].len() > w[0].len()));
    }

    #[test]
    fn more_picks_never_score_less(picks in prop::collection::vec(0i64..30, 1..8)) {
        let mut store = Store::in_memory().map_err(|e| TestCaseError::fail(e.to_string()))?;
        let id = ItemId::new("apps", "x");
        let day = 86_400;
        let mut previous = 0.0;
        let mut times: Vec<i64> = picks.iter().map(|d| d * day).collect();
        times.sort_unstable();
        let end = times.last().copied().unwrap_or(0);
        for t in times {
            store.record("q", &id, t).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let now = store.boosts("q", end).map_err(|e| TestCaseError::fail(e.to_string()))?[&id];
            prop_assert!(now > previous);
            previous = now;
        }
    }
}
