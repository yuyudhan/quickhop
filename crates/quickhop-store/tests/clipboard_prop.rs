//! Property tests for the clipboard history: after any sequence of copies and retention runs,
//! the store holds exactly what a simple list model says, newest first, each text once.

use proptest::prelude::*;
use quickhop_store::state::State;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone)]
enum Op {
    /// Copy one of a few texts after `hours` hours.
    Copy { text: usize, hours: i64 },
    /// Apply retention.
    Prune { days: u32, max: u32 },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0usize..6, 0i64..72).prop_map(|(text, hours)| Op::Copy { text, hours }),
        1 => (1u32..4, 1u32..6).prop_map(|(days, max)| Op::Prune { days, max }),
    ]
}

const TEXTS: [&str; 6] = ["alpha", "beta", "gamma", "alpha ", " ", "ünïcode ✓"];

proptest! {
    #[test]
    fn history_matches_a_list_model(ops in prop::collection::vec(op(), 0..40)) {
        let mut state = State::in_memory().unwrap();
        // Newest first: (text, last copied).
        let mut model: Vec<(&str, i64)> = Vec::new();
        let mut now = 0;
        for op in &ops {
            match *op {
                Op::Copy { text, hours } => {
                    now += hours * 60 * 60 * 1000;
                    let text = TEXTS[text];
                    state.record_clip(text, None, now).unwrap();
                    model.retain(|(t, _)| *t != text);
                    model.insert(0, (text, now));
                }
                Op::Prune { days, max } => {
                    let cutoff = now - i64::from(days) * DAY_MS;
                    let before = model.len();
                    model.retain(|(_, last)| *last >= cutoff);
                    model.truncate(usize::try_from(max).unwrap());
                    let deleted = state.prune_clips(days, max, now).unwrap();
                    prop_assert_eq!(deleted, before - model.len());
                }
            }
            let stored: Vec<(String, i64)> = state
                .clips(100)
                .unwrap()
                .into_iter()
                .map(|clip| (clip.text, clip.last_copied))
                .collect();
            let expected: Vec<(String, i64)> =
                model.iter().map(|(text, last)| ((*text).to_owned(), *last)).collect();
            prop_assert_eq!(stored, expected);
        }
    }
}
