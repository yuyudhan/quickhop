//! Property tests of the clipboard history against a plain model: deduplication, order and
//! retention.

use proptest::prelude::*;
use quickhop_store::clipboard::{History, Retention};

const DAY: i64 = 24 * 60 * 60;

/// The model: (text, copies, first, last), newest first.
type Model = Vec<(String, u32, i64, i64)>;

fn model_add(model: &mut Model, text: &str, now: i64, retention: Retention) {
    if let Some(at) = model.iter().position(|(t, ..)| t == text) {
        let (t, copies, first, _) = model.remove(at);
        model.insert(0, (t, copies + 1, first, now));
    } else {
        model.insert(0, (text.to_owned(), 1, now, now));
    }
    if retention.max_age_days > 0 {
        let oldest = now - i64::from(retention.max_age_days) * DAY;
        model.retain(|(.., last)| *last >= oldest);
    }
    model.truncate(retention.max_entries);
}

fn copies() -> impl Strategy<Value = Vec<(String, i64)>> {
    // Few distinct texts, so copies repeat; gaps of up to three days between copies.
    prop::collection::vec(("[abc]{1,2}", 0..3 * DAY), 1..60)
}

proptest! {
    #[test]
    fn history_matches_the_model(
        steps in copies(),
        max_entries in 1_usize..8,
        max_age_days in 0_u32..5,
    ) {
        let retention = Retention { max_entries, max_age_days };
        let mut history = History::in_memory().map_err(|e| TestCaseError::fail(e.to_string()))?;
        let mut model = Model::new();
        let mut now = 1_700_000_000;
        for (text, gap) in &steps {
            now += gap;
            let kept = history
                .add(text, now, retention)
                .map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert!(kept);
            model_add(&mut model, text, now, retention);
        }
        let entries = history
            .recent(usize::MAX)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        let got: Model = entries
            .into_iter()
            .map(|e| (e.text, e.copies, e.first_copied, e.last_copied))
            .collect();
        prop_assert_eq!(got, model);
    }

    #[test]
    fn recent_is_a_prefix_of_the_full_history(steps in copies(), limit in 0_usize..10) {
        let retention = Retention { max_entries: 100, max_age_days: 0 };
        let mut history = History::in_memory().map_err(|e| TestCaseError::fail(e.to_string()))?;
        for (i, (text, _)) in steps.iter().enumerate() {
            let now = i64::try_from(i).unwrap_or(i64::MAX);
            history.add(text, now, retention).map_err(|e| TestCaseError::fail(e.to_string()))?;
        }
        let all = history.recent(usize::MAX).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let some = history.recent(limit).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&all[..limit.min(all.len())], &some[..]);
        let mut texts: Vec<&str> = all.iter().map(|e| e.text.as_str()).collect();
        texts.sort_unstable();
        texts.dedup();
        prop_assert_eq!(texts.len(), all.len(), "duplicate entries");
    }
}
