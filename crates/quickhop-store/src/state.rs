//! `state.sqlite`: what quickhop remembers beyond frecency. Today the clipboard history
//! (CLP-01); snippet usage, favorites and AI chats join it as later migrations.
//!
//! Clipboard entries are deduplicated by the SHA-256 of their text: copying something already
//! in the history moves that entry to the top instead of adding another. Order is the order of
//! copies (a sequence number), not of wall-clock times, so a clock that jumps cannot reorder
//! the history; times only decide retention. Blank texts and texts over [`MAX_TEXT_BYTES`]
//! are not kept.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

/// Texts longer than this many bytes are not kept.
pub const MAX_TEXT_BYTES: usize = 1 << 20;

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
        if found == 1 {
            let tx = conn.transaction()?;
            adopt_text_history(&tx)?;
            tx.commit()?;
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
    /// returns its entry's id, or `None` when the text is not kept (blank, or longer than
    /// [`MAX_TEXT_BYTES`]). Text already in the history moves that entry to the top.
    ///
    /// # Errors
    /// When SQLite fails; nothing is written then.
    pub fn record_clip(
        &mut self,
        text: &str,
        source_app: Option<&str>,
        now: i64,
    ) -> Result<Option<i64>, StateError> {
        if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
            return Ok(None);
        }
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
        Ok(Some(id))
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
    /// last copied more than `max_age_days` ago (0 keeps them whatever their age), then all
    /// but the newest `max_entries` unpinned ones. Returns how many were deleted.
    ///
    /// # Errors
    /// When SQLite fails; nothing is deleted then.
    pub fn prune_clips(
        &mut self,
        max_age_days: u32,
        max_entries: usize,
        now: i64,
    ) -> Result<usize, StateError> {
        let tx = self.conn.transaction()?;
        let expired = if max_age_days == 0 {
            0
        } else {
            let cutoff = now.saturating_sub(i64::from(max_age_days).saturating_mul(DAY_MS));
            tx.execute(
                "DELETE FROM clipboard WHERE pinned = 0 AND last_copied < ?1",
                [cutoff],
            )?
        };
        let over = tx.execute(
            "DELETE FROM clipboard WHERE pinned = 0 AND id NOT IN (
                 SELECT id FROM clipboard WHERE pinned = 0 ORDER BY seq DESC LIMIT ?1
             )",
            [i64::try_from(max_entries).unwrap_or(i64::MAX)],
        )?;
        tx.commit()?;
        Ok(expired + over)
    }

    /// Deletes every clipboard entry, pinned ones too; returns how many there were.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn clear_clips(&mut self) -> Result<usize, StateError> {
        Ok(self.conn.execute("DELETE FROM clipboard", [])?)
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

/// Moves a text history written by the first command-line `quickhop clipboard` (also schema
/// version 1, but a 64-bit hash and times in seconds) into the [`MIGRATIONS`] layout. Does
/// nothing when the table already has that layout.
fn adopt_text_history(tx: &Transaction<'_>) -> Result<(), StateError> {
    let has_kind: bool = tx.query_row(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('clipboard') WHERE name = 'kind'",
        [],
        |row| row.get(0),
    )?;
    if has_kind {
        return Ok(());
    }
    let rows: Vec<(String, i64, i64, i64, i64)> = tx
        .prepare("SELECT text, first_copied, last_copied, copies, seq FROM clipboard")?
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    tx.execute_batch("DROP TABLE clipboard;")?;
    tx.execute_batch(MIGRATIONS[0])?;
    let mut insert = tx.prepare(
        "INSERT INTO clipboard (hash, kind, text, first_copied, last_copied, copies, seq)
         VALUES (?1, 'text', ?2, ?3, ?4, ?5, ?6)",
    )?;
    for (text, first, last, copies, seq) in rows {
        insert.execute(params![
            text_hash(&text),
            text,
            first.saturating_mul(1000),
            last.saturating_mul(1000),
            copies,
            seq
        ])?;
    }
    Ok(())
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
    fn retention_of_zero_days_keeps_any_age() {
        let mut state = State::in_memory().unwrap();
        state.record_clip("ancient", None, 0).unwrap();
        state.record_clip("new", None, 1_000 * DAY_MS).unwrap();
        assert_eq!(state.prune_clips(0, 10, 1_000 * DAY_MS).unwrap(), 0);
        assert_eq!(state.clips(10).unwrap().len(), 2);
        assert_eq!(
            state.prune_clips(0, 1, 1_000 * DAY_MS).unwrap(),
            1,
            "max_entries still applies"
        );
    }

    #[test]
    fn blank_and_huge_texts_are_not_kept() {
        let mut state = State::in_memory().unwrap();
        assert_eq!(state.record_clip(" \n\t", None, 1).unwrap(), None);
        let huge = "x".repeat(MAX_TEXT_BYTES + 1);
        assert_eq!(state.record_clip(&huge, None, 1).unwrap(), None);
        assert!(state.record_clip(&huge[1..], None, 1).unwrap().is_some());
        assert_eq!(state.clips(10).unwrap().len(), 1);
    }

    #[test]
    fn hashes_separate_texts_that_differ_only_in_whitespace() {
        assert_ne!(text_hash("a"), text_hash("a "));
        assert_eq!(text_hash("a").len(), 64);
    }

    #[test]
    fn delete_lookup_and_clear() {
        let mut state = State::in_memory().unwrap();
        let id = state.record_clip("gone soon", None, 1).unwrap().unwrap();
        assert_eq!(
            state.clip(id).unwrap().map(|c| c.text),
            Some("gone soon".into())
        );
        assert!(state.delete_clip(id).unwrap());
        assert!(!state.delete_clip(id).unwrap());
        assert_eq!(state.clip(id).unwrap(), None);
        state.record_clip("a", None, 2).unwrap();
        state.record_clip("b", None, 3).unwrap();
        assert_eq!(state.clear_clips().unwrap(), 2);
        assert_eq!(state.clips(10).unwrap(), Vec::new());
    }

    #[test]
    fn the_first_command_line_history_is_adopted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE clipboard (id INTEGER PRIMARY KEY, hash INTEGER NOT NULL,
                     text TEXT NOT NULL, first_copied INTEGER NOT NULL,
                     last_copied INTEGER NOT NULL, copies INTEGER NOT NULL, seq INTEGER NOT NULL);
                 CREATE INDEX clipboard_hash ON clipboard (hash);
                 INSERT INTO clipboard VALUES (1, 7, 'older', 10, 20, 2, 1), (2, 8, 'newer', 30, 30, 1, 2);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        let mut state = State::open(&path).unwrap();
        let clips = state.clips(10).unwrap();
        let seen: Vec<(&str, i64, i64, i64)> = clips
            .iter()
            .map(|c| (c.text.as_str(), c.first_copied, c.last_copied, c.copies))
            .collect();
        assert_eq!(
            seen,
            [("newer", 30_000, 30_000, 1), ("older", 10_000, 20_000, 2)]
        );
        state.record_clip("older", None, 40_000).unwrap();
        assert_eq!(
            state.clips(1).unwrap()[0].copies,
            3,
            "adopted entries dedupe"
        );
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
