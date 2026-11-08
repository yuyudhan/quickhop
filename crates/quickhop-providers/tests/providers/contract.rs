//! The provider contract: every provider, built over fixtures, must
//! - answer each probe query within [`BUDGET`];
//! - return the same ids, in the same order, for the same query, with no duplicates;
//! - accept an empty query (computed providers return nothing for it);
//! - honour a cancelled [`Cancel`] with [`ProviderError::Cancelled`];
//! - give every item its own id, a non-empty title and at least one action, and make
//!   every item round-trip through JSON (actions are plain data, safe to dry-run).

use crate::common;
use std::time::{Duration, Instant};

use quickhop_core::{Cancel, Item, Provider, ProviderError, Query};

/// Per-query time budget. Generous for unoptimised builds on shared CI runners; the best of
/// three runs is compared, so one scheduling hiccup does not fail it.
const BUDGET: Duration = Duration::from_millis(100);

/// Queries every provider is probed with.
const PROBES: [&str; 9] = [
    "",
    "a",
    "fire",
    "2+2*3",
    "5 km to miles",
    "smile",
    "g rust",
    "日本語 ✓",
    "zzzzzzzzzzzzzzzzzzzz",
];

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn items(provider: &dyn Provider, text: &str) -> Result<Vec<Item>, String> {
    provider
        .items(&Query::new(text), &Cancel::new())
        .map_err(|error| format!("{} failed for {text:?}: {error}", provider.name()))
}

/// Runs every contract check against `provider`.
fn check(provider: &dyn Provider) -> TestResult {
    let name = provider.name();
    assert!(
        !name.is_empty() && !name.contains(':'),
        "bad provider name {name:?}"
    );
    for text in PROBES {
        let mut best = Duration::MAX;
        for _ in 0..3 {
            let start = Instant::now();
            items(provider, text)?;
            best = best.min(start.elapsed());
        }
        assert!(best <= BUDGET, "{name}: {text:?} took {best:?}");

        let first = items(provider, text)?;
        let again = items(provider, text)?;
        let ids: Vec<_> = first.iter().map(|i| &i.id).collect();
        assert_eq!(
            ids,
            again.iter().map(|i| &i.id).collect::<Vec<_>>(),
            "{name}: unstable ids for {text:?}"
        );
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(
            unique.len(),
            ids.len(),
            "{name}: duplicate ids for {text:?}"
        );

        for item in &first {
            assert_eq!(item.id.provider, name, "{name}: foreign id {}", item.id);
            assert!(!item.id.key.is_empty(), "{name}: empty key");
            assert!(
                !item.title.trim().is_empty(),
                "{name}: empty title {}",
                item.id
            );
            assert!(
                !item.actions.is_empty(),
                "{name}: {} has no action",
                item.id
            );
            let json = serde_json::to_string(item)?;
            let back: Item = serde_json::from_str(&json)?;
            assert_eq!(&back, item, "{name}: {} does not round-trip", item.id);
        }
    }

    let empty = items(provider, "")?;
    if provider.is_computed() {
        assert!(
            empty.is_empty(),
            "{name}: computed provider answered an empty query"
        );
    }

    let cancel = Cancel::new();
    cancel.cancel();
    let start = Instant::now();
    let result = provider.items(&Query::new("fire"), &cancel);
    assert!(
        start.elapsed() <= BUDGET,
        "{name}: cancellation took {:?}",
        start.elapsed()
    );
    assert!(
        matches!(&result, Err(ProviderError::Cancelled { provider }) if provider == name),
        "{name}: cancelled query returned {result:?}"
    );
    Ok(())
}

/// One `#[test]` per provider fixture: `provider_contract!(test_name, expression)`.
macro_rules! provider_contract {
    ($($test:ident => $provider:expr),+ $(,)?) => {
        $(
            #[test]
            fn $test() -> TestResult {
                let provider = $provider;
                check(&provider)
            }
        )+
    };
}

provider_contract! {
    apps_desktop_entries => common::desktop_apps(),
    apps_bundles => common::bundle_apps(),
    apps_start_menu => common::start_menu_apps(),
    calc => quickhop_providers::calc::CalcProvider::new(),
    emoji => quickhop_providers::emoji::EmojiProvider::new(),
    system_linux => quickhop_providers::system::SystemProvider::for_os("linux"),
    system_macos => quickhop_providers::system::SystemProvider::for_os("macos"),
    system_windows => quickhop_providers::system::SystemProvider::for_os("windows"),
    quicklinks => common::quicklinks(),
    scripts => common::scripts(),
}

#[test]
fn every_builtin_provider_has_a_contract_test() {
    let tested = ["apps", "calc", "emoji", "system", "quicklinks", "scripts"];
    assert_eq!(quickhop_providers::BUILTIN, tested);
    let names: Vec<String> = common::all_providers()
        .iter()
        .map(|p| p.name().to_owned())
        .collect();
    for name in quickhop_providers::BUILTIN {
        assert!(
            names.iter().any(|n| n == name),
            "{name} missing from the fixture set"
        );
    }
}
