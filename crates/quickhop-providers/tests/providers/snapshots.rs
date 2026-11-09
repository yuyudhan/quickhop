//! Snapshots of ranked result lists: every provider over fixtures, searched together the
//! way the CLI does, for a fixed set of queries. Review changes with `cargo insta review`
//! (or `INSTA_UPDATE=always cargo test` and read the diff).

use crate::common;
use std::fmt::Write as _;

use quickhop_core::{Boosts, Cancel, ItemId, Query, Search};
use quickhop_store::Store;

/// Queries covering every provider, aliases, keywords, computed answers and misses.
const QUERIES: [&str; 23] = [
    "",
    "fire",
    "firefox",
    "files",
    "code",
    "vsc",
    "term",
    "browser",
    "folder",
    "image editor",
    "2+2*3",
    "5 km to miles",
    "smile",
    "heart",
    "lock",
    "reboot",
    "shut",
    "g rust lang",
    "gh",
    "hello",
    "search",
    "qqqqzzzz",
    "budget",
];

fn render(text: &str, boosts: &Boosts) -> String {
    let search = Search::new(8).with_providers(common::all_providers());
    let results = search.run(&Query::new(text), &Cancel::new(), boosts);
    let mut out = format!("query: {text:?}\n");
    for error in &results.errors {
        let _ = writeln!(out, "error: {error}");
    }
    for r in &results.ranked {
        let tier = format!("{:?}", r.tier).to_lowercase();
        let _ = writeln!(
            out,
            "{tier:<8} {:>5}  {:<40} {} | {}",
            r.score,
            r.item.id.to_string(),
            r.item.title,
            r.item.subtitle.as_deref().unwrap_or("-"),
        );
    }
    out
}

fn slug(text: &str) -> String {
    let slug: String = text
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if slug.is_empty() {
        "empty".into()
    } else {
        slug
    }
}

#[test]
fn ranked_results() {
    for (index, text) in QUERIES.iter().enumerate() {
        insta::assert_snapshot!(
            format!("q{index:02}_{}", slug(text)),
            render(text, &Boosts::new())
        );
    }
}

#[test]
fn frecency_reorders_results() -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::in_memory()?;
    let now = 1_700_000_000;
    let terminal = ItemId::new("apps", "terminal.desktop");
    for _ in 0..3 {
        store.record("t", &terminal, now)?;
    }
    insta::assert_snapshot!("frecency_t", render("t", &store.boosts("t", now)?));
    insta::assert_snapshot!("frecency_t_none", render("t", &Boosts::new()));
    Ok(())
}
