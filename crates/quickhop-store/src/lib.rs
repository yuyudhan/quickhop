//! SQLite store for quickhop: remembers which items the user picks for which query, and
//! turns those picks into frecency boosts.
//!
//! Frecency is kept per (query prefix, item), as in Firefox's location bar: picking an item
//! after typing `fir` credits the empty prefix and `f`, `fi` and `fir`. Each credit is worth 1
//! at the moment of the pick and halves every [`HALF_LIFE_SECS`] (7 days). Scores are
//! stored already decayed to their last update, so a pick is one upsert per prefix.
//!
//! [`state`] is the second database, `state.sqlite`: the clipboard history. [`clipboard`] is
//! the command line's clipboard history in the same file until it moves onto [`state`].
#![forbid(unsafe_code)]

pub mod clipboard;
pub mod state;

use std::path::Path;

use quickhop_core::{Boosts, ItemId};
use rusqlite::{Connection, OptionalExtension, params};

/// Half-life of one pick, in seconds: 7 days.
pub const HALF_LIFE_SECS: f64 = 7.0 * 24.0 * 60.0 * 60.0;

/// Longest query prefix remembered, in characters; longer queries share its credit.
pub const MAX_PREFIX_CHARS: usize = 32;

/// Credits that decayed below this are deleted by [`Store::prune`].
pub const PRUNE_BELOW: f64 = 0.01;

const SCHEMA_VERSION: i64 = 1;

/// Why the store failed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite failed.
    #[error("frecency store: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The file was written by a newer quickhop.
    #[error("frecency store: schema version {0} is newer than this quickhop ({SCHEMA_VERSION})")]
    TooNew(i64),
}

/// An open frecency database.
pub struct Store {
    conn: Connection,
}

/// `value` decayed from `since` to `now` (Unix seconds). Clocks that went backwards do not
/// grow it.
pub fn decay(value: f64, since: i64, now: i64) -> f64 {
    // Beyond u32::MAX seconds (136 years) every score has long decayed to nothing.
    let elapsed = u32::try_from(now.saturating_sub(since).max(0)).unwrap_or(u32::MAX);
    value * 0.5f64.powf(f64::from(elapsed) / HALF_LIFE_SECS)
}

/// The prefix key a query is stored and looked up under: lowercased, whitespace
/// collapsed, at most [`MAX_PREFIX_CHARS`] characters.
pub fn normalize(query: &str) -> String {
    let joined = query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    joined.chars().take(MAX_PREFIX_CHARS).collect()
}

/// Every prefix credited when an item is picked for `query`, shortest (empty) first.
pub fn prefixes(query: &str) -> Vec<String> {
    let normalized = normalize(query);
    let mut out = vec![String::new()];
    let mut current = String::new();
    for c in normalized.chars() {
        current.push(c);
        if !c.is_whitespace() {
            out.push(current.clone());
        }
    }
    out
}

impl Store {
    /// Opens (creating if needed) the database at `path`; the parent directory must exist.
    ///
    /// # Errors
    /// When SQLite cannot open or migrate the file.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::init(Connection::open(path)?)
    }

    /// A private in-memory database, for tests and for running without a data directory.
    ///
    /// # Errors
    /// When SQLite cannot create it.
    pub fn in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(StoreError::TooNew(version));
        }
        if version < 1 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS frecency (
                     prefix   TEXT    NOT NULL,
                     provider TEXT    NOT NULL,
                     key      TEXT    NOT NULL,
                     score    REAL    NOT NULL,
                     updated  INTEGER NOT NULL,
                     PRIMARY KEY (prefix, provider, key)
                 ) WITHOUT ROWID;
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        Ok(Self { conn })
    }

    /// Records that `id` was picked after typing `query`, at `now` (Unix seconds).
    ///
    /// # Errors
    /// When SQLite fails; nothing is written then.
    pub fn record(&mut self, query: &str, id: &ItemId, now: i64) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        {
            let mut select =
                tx.prepare_cached("SELECT score, updated FROM frecency WHERE prefix = ?1 AND provider = ?2 AND key = ?3")?;
            let mut upsert = tx.prepare_cached(
                "INSERT INTO frecency (prefix, provider, key, score, updated) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (prefix, provider, key) DO UPDATE SET score = excluded.score, updated = excluded.updated",
            )?;
            for prefix in prefixes(query) {
                let old: Option<(f64, i64)> = select
                    .query_row(params![prefix, id.provider, id.key], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })
                    .optional()?;
                let score = old.map_or(0.0, |(score, since)| decay(score, since, now)) + 1.0;
                upsert.execute(params![prefix, id.provider, id.key, score, now])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Frecency of every item ever picked under `query`'s prefix, decayed to `now`.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn boosts(&self, query: &str, now: i64) -> Result<Boosts, StoreError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT provider, key, score, updated FROM frecency WHERE prefix = ?1",
        )?;
        let rows = statement.query_map(params![normalize(query)], |row| {
            Ok((
                ItemId::new(row.get::<_, String>(0)?, row.get::<_, String>(1)?),
                row.get::<_, f64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut boosts = Boosts::new();
        for row in rows {
            let (id, score, updated) = row?;
            boosts.insert(id, decay(score, updated, now));
        }
        Ok(boosts)
    }

    /// Deletes credits that decayed below [`PRUNE_BELOW`] by `now`; returns how many.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn prune(&mut self, now: i64) -> Result<usize, StoreError> {
        let tx = self.conn.transaction()?;
        let mut stale = Vec::new();
        {
            let mut statement =
                tx.prepare("SELECT prefix, provider, key, score, updated FROM frecency")?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?;
            for row in rows {
                let (prefix, provider, key, score, updated) = row?;
                if decay(score, updated, now) < PRUNE_BELOW {
                    stale.push((prefix, provider, key));
                }
            }
        }
        {
            let mut delete = tx
                .prepare("DELETE FROM frecency WHERE prefix = ?1 AND provider = ?2 AND key = ?3")?;
            for (prefix, provider, key) in &stale {
                delete.execute(params![prefix, provider, key])?;
            }
        }
        tx.commit()?;
        Ok(stale.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60;

    fn id(key: &str) -> ItemId {
        ItemId::new("apps", key)
    }

    #[test]
    fn prefixes_cover_every_typed_prefix() {
        assert_eq!(prefixes(" Fi  r "), vec!["", "f", "fi", "fi r"]);
        assert_eq!(prefixes(""), vec![""]);
        assert_eq!(prefixes(&"x".repeat(50)).len(), MAX_PREFIX_CHARS + 1);
    }

    #[test]
    fn a_pick_halves_after_seven_days() {
        let mut store = Store::in_memory().unwrap();
        store.record("fir", &id("firefox"), 0).unwrap();
        let now = store.boosts("fi", 0).unwrap()[&id("firefox")];
        let later = store.boosts("fi", 7 * DAY).unwrap()[&id("firefox")];
        assert!((now - 1.0).abs() < 1e-9, "{now}");
        assert!((later - 0.5).abs() < 1e-9, "{later}");
    }

    #[test]
    fn picks_accumulate_with_decay() {
        let mut store = Store::in_memory().unwrap();
        store.record("f", &id("firefox"), 0).unwrap();
        store.record("f", &id("firefox"), 7 * DAY).unwrap();
        let total = store.boosts("F", 7 * DAY).unwrap()[&id("firefox")];
        assert!((total - 1.5).abs() < 1e-9, "{total}");
    }

    #[test]
    fn only_the_typed_prefix_is_credited() {
        let mut store = Store::in_memory().unwrap();
        store.record("fi", &id("files"), 0).unwrap();
        assert_eq!(store.boosts("fir", 0).unwrap().len(), 0);
        assert!(store.boosts("", 0).unwrap().contains_key(&id("files")));
    }

    #[test]
    fn prune_drops_decayed_credits() {
        let mut store = Store::in_memory().unwrap();
        store.record("a", &id("old"), 0).unwrap();
        store.record("a", &id("new"), 100 * DAY).unwrap();
        assert_eq!(store.prune(100 * DAY).unwrap(), 2);
        let left = store.boosts("a", 100 * DAY).unwrap();
        assert_eq!(left.keys().collect::<Vec<_>>(), vec![&id("new")]);
    }

    #[test]
    fn reopens_a_file_and_rejects_newer_schemas() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.sqlite");
        Store::open(&path)
            .unwrap()
            .record("x", &id("y"), 0)
            .unwrap();
        assert_eq!(Store::open(&path).unwrap().boosts("x", 0).unwrap().len(), 1);
        Connection::open(&path)
            .unwrap()
            .execute_batch("PRAGMA user_version = 9")
            .unwrap();
        assert!(matches!(Store::open(&path), Err(StoreError::TooNew(9))));
    }

    #[test]
    fn clock_going_backwards_does_not_grow_scores() {
        assert!((decay(2.0, 100, 50) - 2.0).abs() < f64::EPSILON);
    }
}
