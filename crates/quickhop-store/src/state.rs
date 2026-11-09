//! `state.sqlite`: what quickhop remembers beyond frecency. Today the clipboard history
//! (CLP-01); snippet usage, favorites and AI chats join it as later migrations.
//!
//! Clipboard entries are deduplicated by the SHA-256 of their text: copying something already
//! in the history moves that entry to the top instead of adding another. Order is the order of
//! copies (a sequence number), not of wall-clock times, so a clock that jumps cannot reorder
//! the history; times only decide retention.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

/// Schema migrations, oldest first; `PRAGMA user_version` counts the ones applied.
const MIGRATIONS: &[&str] = &["CREATE TABLE clipboard (
         id           INTEGER PRIMARY KEY,
         hash         TEXT    NOT NULL UNIQUE,
         kind         TEXT    NOT NULL,
         text         TEXT,
         source_app   TEXT,
         pinned       INTEGER NOT NULL DEFAULT 0,
         first_copied INTEGER NOT NULL,
         last_copied  INTEGER NOT NULL,
         copies       INTEGER NOT NULL DEFAULT 1,
         seq          INTEGER NOT NULL
     );
     CREATE INDEX clipboard_by_seq ON clipboard (seq DESC);"];

/// Milliseconds in a day.
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// Why the state database failed.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// SQLite failed.
    #[error("state store: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The file was written by a newer quickhop.
    #[error("state store: schema version {found} is newer than this quickhop ({known})")]
    TooNew {
        /// Version in the file.
        found: i64,
        /// Newest version this build knows.
        known: i64,
    },
}

/// One clipboard history entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    /// Stable id of the entry.
    pub id: i64,
    /// The copied text.
    pub text: String,
    /// The app it was last copied from, when known.
    pub source_app: Option<String>,
    /// When it was first copied, Unix milliseconds.
    pub first_copied: i64,
    /// When it was last copied, Unix milliseconds.
    pub last_copied: i64,
    /// How many times it was copied.
    pub copies: i64,
}

/// An open `state.sqlite`.
pub struct State {
    conn: Connection,
}

impl State {
    /// Opens (creating and migrating if needed) the database at `path`; the parent directory
    /// must exist.
    ///
    /// # Errors
    /// When SQLite cannot open or migrate the file, or a newer quickhop wrote it.
    pub fn open(path: &Path) -> Result<Self, StateError> {
        Self::init(Connection::open(path)?)
    }

    /// A private in-memory database, for tests and for running without a data directory.
    ///
    /// # Errors
    /// When SQLite cannot create it.
    pub fn in_memory() -> Result<Self, StateError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self, StateError> {
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let known = i64::try_from(MIGRATIONS.len()).unwrap_or(i64::MAX);
        let found: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if found > known {
            return Err(StateError::TooNew { found, known });
        }
        for (version, sql) in (1..)
            .zip(MIGRATIONS)
            .skip(usize::try_from(found).unwrap_or(0))
        {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            // PRAGMA takes no bound parameters; `version` is a number from this loop.
            tx.execute_batch(&format!("PRAGMA user_version = {version}"))?;
            tx.commit()?;
        }
        Ok(Self { conn })
    }

    /// Records that `text` was copied from `source_app` at `now` (Unix milliseconds) and
    /// returns its entry's id. Text already in the history moves that entry to the top.
    ///
    /// # Errors
    /// When SQLite fails; nothing is written then.
    pub fn record_clip(
        &mut self,
        text: &str,
        source_app: Option<&str>,
        now: i64,
    ) -> Result<i64, StateError> {
        let hash = text_hash(text);
        let tx = self.conn.transaction()?;
        let seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM clipboard",
            [],
            |row| row.get(0),
        )?;
        let id = tx.query_row(
            "INSERT INTO clipboard (hash, kind, text, source_app, first_copied, last_copied, seq)
             VALUES (?1, 'text', ?2, ?3, ?4, ?4, ?5)
             ON CONFLICT (hash) DO UPDATE SET
                 source_app = COALESCE(excluded.source_app, source_app),
                 last_copied = excluded.last_copied,
                 copies = copies + 1,
                 seq = excluded.seq
             RETURNING id",
            params![hash, text, source_app, now, seq],
            |row| row.get(0),
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// The newest `limit` entries, most recently copied first.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn clips(&self, limit: usize) -> Result<Vec<Clip>, StateError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut select = self.conn.prepare_cached(
            "SELECT id, text, source_app, first_copied, last_copied, copies FROM clipboard
             WHERE kind = 'text' ORDER BY seq DESC LIMIT ?1",
        )?;
        let rows = select.query_map([limit], |row| {
            Ok(Clip {
                id: row.get(0)?,
                text: row.get(1)?,
                source_app: row.get(2)?,
                first_copied: row.get(3)?,
                last_copied: row.get(4)?,
                copies: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Applies the retention settings at `now` (Unix milliseconds): deletes unpinned entries
    /// last copied more than `retention_days` ago, then all but the newest `max_entries`
    /// unpinned ones. Returns how many were deleted.
    ///
    /// # Errors
    /// When SQLite fails; nothing is deleted then.
    pub fn prune_clips(
        &mut self,
        retention_days: u32,
        max_entries: u32,
        now: i64,
    ) -> Result<usize, StateError> {
        let cutoff = now.saturating_sub(i64::from(retention_days).saturating_mul(DAY_MS));
        let tx = self.conn.transaction()?;
        let expired = tx.execute(
            "DELETE FROM clipboard WHERE pinned = 0 AND last_copied < ?1",
            [cutoff],
        )?;
        let over = tx.execute(
            "DELETE FROM clipboard WHERE pinned = 0 AND id NOT IN (
                 SELECT id FROM clipboard WHERE pinned = 0 ORDER BY seq DESC LIMIT ?1
             )",
            [i64::from(max_entries)],
        )?;
        tx.commit()?;
        Ok(expired + over)
    }

    /// Deletes one entry; returns whether it existed.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn delete_clip(&mut self, id: i64) -> Result<bool, StateError> {
        Ok(self
            .conn
            .execute("DELETE FROM clipboard WHERE id = ?1", [id])?
            == 1)
    }

    /// The entry with this id.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn clip(&self, id: i64) -> Result<Option<Clip>, StateError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, text, source_app, first_copied, last_copied, copies FROM clipboard
                 WHERE id = ?1",
                [id],
                |row| {
                    Ok(Clip {
                        id: row.get(0)?,
                        text: row.get(1)?,
                        source_app: row.get(2)?,
                        first_copied: row.get(3)?,
                        last_copied: row.get(4)?,
                        copies: row.get(5)?,
                    })
                },
            )
            .optional()?)
    }
}

/// The dedupe key of a text entry.
fn text_hash(text: &str) -> String {
    let digest = Sha256::new()
        .chain_update(b"text\0")
        .chain_update(text.as_bytes())
        .finalize();
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write as _;
            // Writing to a String cannot fail.
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_listed_newest_first() {
        let mut state = State::in_memory().unwrap();
        for (n, text) in ["one", "two", "three"].into_iter().enumerate() {
            state
                .record_clip(text, None, 1_000 + i64::try_from(n).unwrap())
                .unwrap();
        }
        let texts: Vec<String> = state
            .clips(10)
            .unwrap()
            .into_iter()
            .map(|c| c.text)
            .collect();
        assert_eq!(texts, ["three", "two", "one"]);
        assert_eq!(state.clips(2).unwrap().len(), 2);
    }

    #[test]
    fn copying_again_moves_the_entry_to_the_top() {
        let mut state = State::in_memory().unwrap();
        let first = state
            .record_clip("one", Some("com.apple.Safari"), 1_000)
            .unwrap();
        state.record_clip("two", None, 2_000).unwrap();
        let again = state.record_clip("one", None, 3_000).unwrap();
        assert_eq!(first, again, "the same entry, not a new one");
        let clips = state.clips(10).unwrap();
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].text, "one");
        assert_eq!(clips[0].copies, 2);
        assert_eq!(clips[0].first_copied, 1_000);
        assert_eq!(clips[0].last_copied, 3_000);
        assert_eq!(
            clips[0].source_app.as_deref(),
            Some("com.apple.Safari"),
            "an unknown source keeps the known one"
        );
    }

    #[test]
    fn order_follows_copies_even_when_the_clock_goes_back() {
        let mut state = State::in_memory().unwrap();
        state.record_clip("earlier", None, 5_000).unwrap();
        state.record_clip("later", None, 4_000).unwrap();
        assert_eq!(state.clips(1).unwrap()[0].text, "later");
    }

    #[test]
    fn retention_deletes_old_entries_then_keeps_the_newest() {
        let mut state = State::in_memory().unwrap();
        let now = 100 * DAY_MS;
        state
            .record_clip("ancient", None, now - 31 * DAY_MS)
            .unwrap();
        state.record_clip("old", None, now - 29 * DAY_MS).unwrap();
        state.record_clip("new", None, now - DAY_MS).unwrap();
        state.record_clip("newest", None, now).unwrap();
        assert_eq!(state.prune_clips(30, 2, now).unwrap(), 2);
        let texts: Vec<String> = state
            .clips(10)
            .unwrap()
            .into_iter()
            .map(|c| c.text)
            .collect();
        assert_eq!(texts, ["newest", "new"]);
    }

    #[test]
    fn retention_keeps_entries_exactly_retention_days_old() {
        let mut state = State::in_memory().unwrap();
        let now = 100 * DAY_MS;
        state
            .record_clip("just expired", None, now - 30 * DAY_MS - 1)
            .unwrap();
        state
            .record_clip("thirty days", None, now - 30 * DAY_MS)
            .unwrap();
        assert_eq!(state.prune_clips(30, 10, now).unwrap(), 1);
        assert_eq!(state.clips(10).unwrap()[0].text, "thirty days");
    }

    #[test]
    fn hashes_separate_texts_that_differ_only_in_whitespace() {
        assert_ne!(text_hash("a"), text_hash("a "));
        assert_eq!(text_hash("a").len(), 64);
    }

    #[test]
    fn delete_and_lookup() {
        let mut state = State::in_memory().unwrap();
        let id = state.record_clip("gone soon", None, 1).unwrap();
        assert_eq!(
            state.clip(id).unwrap().map(|c| c.text),
            Some("gone soon".into())
        );
        assert!(state.delete_clip(id).unwrap());
        assert!(!state.delete_clip(id).unwrap());
        assert_eq!(state.clip(id).unwrap(), None);
    }

    #[test]
    fn reopening_keeps_the_history_and_newer_schemas_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        State::open(&path)
            .unwrap()
            .record_clip("kept", None, 1)
            .unwrap();
        assert_eq!(
            State::open(&path).unwrap().clips(1).unwrap()[0].text,
            "kept"
        );
        Connection::open(&path)
            .unwrap()
            .execute_batch("PRAGMA user_version = 99")
            .unwrap();
        assert!(matches!(
            State::open(&path),
            Err(StateError::TooNew {
                found: 99,
                known: 1
            })
        ));
    }
}
