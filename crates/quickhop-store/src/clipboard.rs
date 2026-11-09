//! Clipboard history (CLP-01): the text entries copied while quickhop watched the clipboard,
//! newest first, kept in `state.sqlite`.
//!
//! Copying a text that is already in the history moves it to the front and counts the copy
//! instead of adding a second entry (entries are found by an FNV-1a hash of the text, then
//! compared in full). After every addition the history is cut to the [`Retention`] limits.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

/// Texts longer than this many bytes are not kept.
pub const MAX_TEXT_BYTES: usize = 1 << 20;

const SCHEMA_VERSION: i64 = 1;

const SECS_PER_DAY: i64 = 24 * 60 * 60;

/// Why the history failed.
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    /// SQLite failed.
    #[error("clipboard history: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The file was written by a newer quickhop.
    #[error("clipboard history: schema version {0} is newer than this quickhop ({SCHEMA_VERSION})")]
    TooNew(i64),
}

/// How much history is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    /// Most entries kept; older ones are dropped first.
    pub max_entries: usize,
    /// Entries not copied for this many days are dropped; 0 keeps them forever.
    pub max_age_days: u32,
}

/// One remembered text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Row id, stable while the entry exists.
    pub id: i64,
    /// The copied text.
    pub text: String,
    /// When it was first copied (Unix seconds).
    pub first_copied: i64,
    /// When it was last copied (Unix seconds).
    pub last_copied: i64,
    /// How many times it was copied while in the history.
    pub copies: u32,
}

/// The 64-bit FNV-1a hash of `text`, as SQLite stores it. Stable across versions and
/// platforms, unlike `std`'s hasher.
pub fn content_hash(text: &str) -> i64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    i64::from_ne_bytes(hash.to_ne_bytes())
}

/// An open clipboard history.
pub struct History {
    conn: Connection,
}

impl History {
    /// Opens (creating if needed) the database at `path`; the parent directory must exist.
    ///
    /// # Errors
    /// When SQLite cannot open or migrate the file.
    pub fn open(path: &Path) -> Result<Self, HistoryError> {
        Self::init(Connection::open(path)?)
    }

    /// A private in-memory history, for tests.
    ///
    /// # Errors
    /// When SQLite cannot create it.
    pub fn in_memory() -> Result<Self, HistoryError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, HistoryError> {
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(HistoryError::TooNew(version));
        }
        // One step per schema version, applied in order.
        if version < 1 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS clipboard (
                     id           INTEGER PRIMARY KEY,
                     hash         INTEGER NOT NULL,
                     text         TEXT    NOT NULL,
                     first_copied INTEGER NOT NULL,
                     last_copied  INTEGER NOT NULL,
                     copies       INTEGER NOT NULL,
                     -- Order of the last copy, so equal timestamps still sort newest first.
                     seq          INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS clipboard_hash ON clipboard (hash);
                 CREATE INDEX IF NOT EXISTS clipboard_seq ON clipboard (seq);
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        Ok(Self { conn })
    }

    /// Records that `text` was copied at `now` (Unix seconds), then applies `retention`.
    /// Returns whether it was kept: blank texts and ones over [`MAX_TEXT_BYTES`] are not.
    ///
    /// # Errors
    /// When SQLite fails; nothing is written then.
    pub fn add(
        &mut self,
        text: &str,
        now: i64,
        retention: Retention,
    ) -> Result<bool, HistoryError> {
        if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
            return Ok(false);
        }
        let hash = content_hash(text);
        let tx = self.conn.transaction()?;
        let seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM clipboard",
            [],
            |row| row.get(0),
        )?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM clipboard WHERE hash = ?1 AND text = ?2",
                params![hash, text],
                |row| row.get(0),
            )
            .optional()?;
        match existing {
            Some(id) => {
                tx.execute(
                    "UPDATE clipboard SET last_copied = ?1, copies = copies + 1, seq = ?2
                     WHERE id = ?3",
                    params![now, seq, id],
                )?;
            }
            None => {
                tx.execute(
                    "INSERT INTO clipboard (hash, text, first_copied, last_copied, copies, seq)
                     VALUES (?1, ?2, ?3, ?3, 1, ?4)",
                    params![hash, text, now, seq],
                )?;
            }
        }
        if retention.max_age_days > 0 {
            let oldest = now.saturating_sub(i64::from(retention.max_age_days) * SECS_PER_DAY);
            tx.execute("DELETE FROM clipboard WHERE last_copied < ?1", [oldest])?;
        }
        let keep = i64::try_from(retention.max_entries).unwrap_or(i64::MAX);
        tx.execute(
            "DELETE FROM clipboard WHERE id NOT IN
             (SELECT id FROM clipboard ORDER BY seq DESC LIMIT ?1)",
            [keep],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// The `limit` most recently copied entries, newest first.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn recent(&self, limit: usize) -> Result<Vec<Entry>, HistoryError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT id, text, first_copied, last_copied, copies FROM clipboard
             ORDER BY seq DESC LIMIT ?1",
        )?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = statement.query_map([limit], |row| {
            Ok(Entry {
                id: row.get(0)?,
                text: row.get(1)?,
                first_copied: row.get(2)?,
                last_copied: row.get(3)?,
                copies: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Number of entries.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn len(&self) -> Result<usize, HistoryError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM clipboard", [], |row| row.get(0))?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    /// Whether the history holds no entry.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn is_empty(&self) -> Result<bool, HistoryError> {
        Ok(self.len()? == 0)
    }

    /// Deletes the entry `id`; returns whether it existed.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn remove(&mut self, id: i64) -> Result<bool, HistoryError> {
        Ok(self
            .conn
            .execute("DELETE FROM clipboard WHERE id = ?1", [id])?
            > 0)
    }

    /// Deletes every entry.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn clear(&mut self) -> Result<(), HistoryError> {
        self.conn.execute("DELETE FROM clipboard", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEEP: Retention = Retention {
        max_entries: 100,
        max_age_days: 0,
    };

    fn texts(history: &History) -> Vec<String> {
        history
            .recent(usize::MAX)
            .unwrap()
            .into_iter()
            .map(|e| e.text)
            .collect()
    }

    #[test]
    fn hash_is_fnv1a() {
        // Published FNV-1a 64 test vectors.
        assert_eq!(content_hash("").cast_unsigned(), 0xcbf2_9ce4_8422_2325);
        assert_eq!(content_hash("a").cast_unsigned(), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(
            content_hash("foobar").cast_unsigned(),
            0x8594_4171_f739_67e8
        );
    }

    #[test]
    fn recopying_moves_to_front_and_counts() {
        let mut history = History::in_memory().unwrap();
        for (now, text) in [(10, "one"), (10, "two"), (10, "one"), (11, "three")] {
            assert!(history.add(text, now, KEEP).unwrap());
        }
        assert_eq!(texts(&history), ["three", "one", "two"]);
        let one = &history.recent(3).unwrap()[1];
        assert_eq!((one.copies, one.first_copied, one.last_copied), (2, 10, 10));
    }

    #[test]
    fn blank_and_huge_texts_are_skipped() {
        let mut history = History::in_memory().unwrap();
        assert!(!history.add(" \n\t", 1, KEEP).unwrap());
        assert!(
            !history
                .add(&"x".repeat(MAX_TEXT_BYTES + 1), 1, KEEP)
                .unwrap()
        );
        assert!(history.add(&"x".repeat(MAX_TEXT_BYTES), 1, KEEP).unwrap());
        assert_eq!(history.len().unwrap(), 1);
    }

    #[test]
    fn remove_and_clear() {
        let mut history = History::in_memory().unwrap();
        history.add("a", 1, KEEP).unwrap();
        history.add("b", 2, KEEP).unwrap();
        let a = history.recent(10).unwrap()[1].id;
        assert!(history.remove(a).unwrap());
        assert!(!history.remove(a).unwrap());
        assert_eq!(texts(&history), ["b"]);
        history.clear().unwrap();
        assert!(history.is_empty().unwrap());
    }

    #[test]
    fn survives_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        History::open(&path).unwrap().add("kept", 5, KEEP).unwrap();
        assert_eq!(texts(&History::open(&path).unwrap()), ["kept"]);
    }
}
