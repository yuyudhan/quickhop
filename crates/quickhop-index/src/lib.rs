//! File and folder index for quickhop (FIL-01): a parallel walk of the configured roots
//! (honouring `.gitignore`, skipping hidden and excluded folders), paths stored in SQLite
//! with an FTS5 trigram index, kept current by a filesystem watcher (inotify, `FSEvents`), and
//! searched through quickhop's fuzzy matcher.
//!
//! A query's words of three or more characters must each appear in the path (trigram
//! match); shorter words must appear too (substring filter). The candidates are then
//! ranked by the nucleo score of the file name, else of the whole path.
#![forbid(unsafe_code)]

mod watch;

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use quickhop_core::{Cancel, Matcher};
use rusqlite::{Connection, OptionalExtension, params};

pub use watch::Watcher;

/// Shortest query word the trigram index can look up.
pub const MIN_WORD_CHARS: usize = 3;

/// Most candidates read from the trigram index per query before fuzzy ranking.
pub const MAX_CANDIDATES: usize = 20_000;

/// Rarest trigrams of each query word looked up in the index.
pub const TRIGRAMS_PER_WORD: usize = 2;

/// Points subtracted when only the path, not the file name, matched.
pub const PATH_PENALTY: i64 = 24;

const SCHEMA_VERSION: i64 = 1;

/// Folder names skipped by default: version control, dependency and build output, caches.
pub const DEFAULT_EXCLUDE: [&str; 10] = [
    ".git",
    "node_modules",
    "target",
    "__pycache__",
    ".cache",
    ".venv",
    "venv",
    "build",
    "dist",
    ".Trash",
];

/// Why the index failed.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    /// SQLite failed.
    #[error("file index: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The file was written by a newer quickhop.
    #[error("file index: schema version {0} is newer than this quickhop ({SCHEMA_VERSION})")]
    TooNew(i64),
    /// The walk was cancelled.
    #[error("file index: cancelled")]
    Cancelled,
    /// The watcher could not be started.
    #[error("file index watcher: {0}")]
    Watch(#[from] notify::Error),
}

/// What is indexed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Folders walked and watched.
    pub roots: Vec<PathBuf>,
    /// Index hidden files and folders (names starting with `.`).
    pub hidden: bool,
    /// Honour `.gitignore` files inside git repositories.
    pub gitignore: bool,
    /// Folder or file names skipped wherever they appear.
    pub exclude: Vec<String>,
    /// Absolute paths never indexed, e.g. the folder holding the index itself.
    pub skip: Vec<PathBuf>,
}

impl Options {
    /// Default options for `roots`: no hidden files, `.gitignore` honoured,
    /// [`DEFAULT_EXCLUDE`] skipped.
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            hidden: false,
            gitignore: true,
            exclude: DEFAULT_EXCLUDE.iter().map(|&s| s.to_owned()).collect(),
            skip: Vec::new(),
        }
    }

    /// Whether `path`, found under one of the roots, should stay out of the index: it is in
    /// `skip`, or a component below its root is hidden or excluded.
    pub fn is_ignored(&self, path: &Path) -> bool {
        if self.skip.iter().any(|skip| path.starts_with(skip)) {
            return true;
        }
        let Some(root) = self.roots.iter().find(|root| path.starts_with(root)) else {
            return true;
        };
        let Ok(relative) = path.strip_prefix(root) else {
            return true;
        };
        relative.components().any(|component| {
            let name = component.as_os_str().to_string_lossy();
            (!self.hidden && name.starts_with('.')) || self.exclude.iter().any(|e| *e == name)
        })
    }
}

/// One indexed file or folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Absolute path.
    pub path: PathBuf,
    /// Last component.
    pub name: String,
    /// Whether it is a folder.
    pub is_dir: bool,
}

/// A ranked search result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The entry.
    pub entry: Entry,
    /// Fuzzy score, higher first.
    pub score: i64,
}

/// What a rebuild found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Files indexed.
    pub files: usize,
    /// Folders indexed.
    pub dirs: usize,
    /// Wall time in milliseconds.
    pub millis: u128,
}

/// An open index database.
pub struct Index {
    conn: Connection,
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Trigrams of each word that are looked up in the index: the [`TRIGRAMS_PER_WORD`] rarest
/// by `count` (entries containing it; `None` when unknown, e.g. folded differently by
/// SQLite, which sorts it last). Rare trigrams have short posting lists, which is what makes
/// a query fast; the caller checks that each word really is a substring.
fn query_trigrams(words: &[&str], mut count: impl FnMut(&str) -> Option<i64>) -> Vec<String> {
    let mut chosen: Vec<String> = Vec::new();
    for word in words {
        let chars: Vec<char> = word.to_lowercase().chars().collect();
        let mut trigrams: Vec<(i64, String)> = chars
            .windows(MIN_WORD_CHARS)
            .map(|window| {
                let trigram: String = window.iter().collect();
                let docs = count(&trigram).filter(|&n| n > 0).unwrap_or(i64::MAX);
                (docs, trigram)
            })
            .collect();
        trigrams.sort();
        trigrams.dedup_by(|a, b| a.1 == b.1);
        for (_, trigram) in trigrams.into_iter().take(TRIGRAMS_PER_WORD) {
            if !chosen.contains(&trigram) {
                chosen.push(trigram);
            }
        }
    }
    chosen
}

/// The FTS5 query requiring every trigram, each quoted. With `detail=none` the index cannot
/// match a phrase, so trigrams are separate terms.
fn fts_query(trigrams: &[String]) -> String {
    trigrams
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

impl Index {
    /// Opens (creating if needed) the index at `path`; the parent directory must exist.
    ///
    /// # Errors
    /// When SQLite cannot open or migrate the file.
    pub fn open(path: &Path) -> Result<Self, IndexError> {
        Self::init(Connection::open(path)?)
    }

    /// An index that lives in memory (tests, previews).
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn in_memory() -> Result<Self, IndexError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, IndexError> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(IndexError::TooNew(version));
        }
        if version < SCHEMA_VERSION {
            Self::create(&conn)?;
        }
        Ok(Self { conn })
    }

    fn create(conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch(&format!(
            "DROP TABLE IF EXISTS trigram_docs;
             DROP TABLE IF EXISTS files_vocab;
             DROP TABLE IF EXISTS files_fts;
             DROP TABLE IF EXISTS files;
             CREATE TABLE files (
                 id INTEGER PRIMARY KEY,
                 path TEXT NOT NULL UNIQUE,
                 name TEXT NOT NULL,
                 is_dir INTEGER NOT NULL
             );
             -- detail=none keeps the index small and fast; it matches every trigram of a
             -- word without checking their order, so `search` verifies the substrings.
             CREATE VIRTUAL TABLE files_fts USING fts5(
                 path, content='', contentless_delete=1, detail=none, tokenize='trigram'
             );
             CREATE VIRTUAL TABLE files_vocab USING fts5vocab(files_fts, 'row');
             -- Entries per trigram as of the last rebuild, to look up the rarest ones
             -- (counting through files_vocab costs as much as the query itself).
             CREATE TABLE trigram_docs (term TEXT PRIMARY KEY, docs INTEGER NOT NULL)
                 WITHOUT ROWID;
             PRAGMA user_version = {SCHEMA_VERSION};"
        ))
    }

    /// Number of indexed entries.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn len(&self) -> Result<usize, IndexError> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM files", [], |row| row.get(0))?;
        Ok(usize::try_from(count).unwrap_or_default())
    }

    /// Whether nothing is indexed.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn is_empty(&self) -> Result<bool, IndexError> {
        Ok(self.len()? == 0)
    }

    /// Whether `path` is indexed.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn contains(&self, path: &Path) -> Result<bool, IndexError> {
        let found = self
            .conn
            .query_row(
                "SELECT 1 FROM files WHERE path = ?1",
                [path.to_string_lossy()],
                |_| Ok(()),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Replaces the whole index with a fresh walk of `options.roots`.
    ///
    /// # Errors
    /// When SQLite fails, or with [`IndexError::Cancelled`] (the old index is kept).
    pub fn rebuild(&mut self, options: &Options, cancel: &Cancel) -> Result<Stats, IndexError> {
        let started = Instant::now();
        let entries = walk(options, &options.roots, cancel)?;
        let tx = self.conn.transaction()?;
        Self::create(&tx)?;
        let mut stats = Stats {
            files: 0,
            dirs: 0,
            millis: 0,
        };
        {
            let mut row =
                tx.prepare("INSERT INTO files (path, name, is_dir) VALUES (?1, ?2, ?3)")?;
            let mut fts = tx.prepare("INSERT INTO files_fts (rowid, path) VALUES (?1, ?2)")?;
            for entry in &entries {
                let path = entry.path.to_string_lossy();
                row.execute(params![path, entry.name, entry.is_dir])?;
                fts.execute(params![tx.last_insert_rowid(), path])?;
                if entry.is_dir {
                    stats.dirs += 1;
                } else {
                    stats.files += 1;
                }
            }
        }
        tx.commit()?;
        // One merged FTS segment, fresh trigram counts and an empty write-ahead log keep
        // queries fast.
        self.conn.execute_batch(
            "INSERT INTO files_fts (files_fts) VALUES ('optimize');
             DELETE FROM trigram_docs;
             INSERT INTO trigram_docs SELECT term, doc FROM files_vocab;",
        )?;
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        stats.millis = started.elapsed().as_millis();
        Ok(stats)
    }

    /// Adds `path` (and, for a folder, everything under it that `options` allows), or
    /// removes it and everything under it when it no longer exists or is ignored. A path
    /// already indexed is left as it is: changes inside a known folder arrive as their own
    /// events.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn refresh(&mut self, options: &Options, path: &Path) -> Result<(), IndexError> {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return self.remove(path);
        };
        if options.is_ignored(path) {
            return self.remove(path);
        }
        if self.contains(path)? {
            return Ok(());
        }
        let mut entries = vec![Entry {
            path: path.to_path_buf(),
            name: name_of(path),
            is_dir: meta.is_dir(),
        }];
        if meta.is_dir() {
            entries.extend(walk(options, &[path.to_path_buf()], &Cancel::new())?);
        }
        let tx = self.conn.transaction()?;
        {
            let mut known = tx.prepare("SELECT 1 FROM files WHERE path = ?1")?;
            let mut row =
                tx.prepare("INSERT INTO files (path, name, is_dir) VALUES (?1, ?2, ?3)")?;
            let mut fts = tx.prepare("INSERT INTO files_fts (rowid, path) VALUES (?1, ?2)")?;
            for entry in &entries {
                let path = entry.path.to_string_lossy();
                if known.exists([&path])? {
                    continue;
                }
                row.execute(params![path, entry.name, entry.is_dir])?;
                fts.execute(params![tx.last_insert_rowid(), path])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Removes `path` and everything indexed under it.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn remove(&mut self, path: &Path) -> Result<(), IndexError> {
        let path = path.to_string_lossy();
        let separator = std::path::MAIN_SEPARATOR;
        // Everything under `path/` sorts between `path/` and `path` + the next character.
        let next = char::from_u32(u32::from(separator) + 1).unwrap_or(separator);
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM files_fts WHERE rowid IN (SELECT id FROM files
             WHERE path = ?1 OR (path >= ?1 || ?2 AND path < ?1 || ?3))",
            params![path, separator.to_string(), next.to_string()],
        )?;
        tx.execute(
            "DELETE FROM files WHERE path = ?1 OR (path >= ?1 || ?2 AND path < ?1 || ?3)",
            params![path, separator.to_string(), next.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The best `limit` entries for `query`, best first. Empty when no word has
    /// [`MIN_WORD_CHARS`] characters.
    ///
    /// # Errors
    /// When SQLite fails.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>, IndexError> {
        let words: Vec<&str> = query.split_whitespace().collect();
        let long: Vec<&str> = words
            .iter()
            .copied()
            .filter(|w| w.chars().count() >= MIN_WORD_CHARS)
            .collect();
        if long.is_empty() {
            return Ok(Vec::new());
        }
        let lowered: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        let mut statement = self.conn.prepare_cached(
            "SELECT f.path, f.name, f.is_dir FROM files_fts JOIN files f ON f.id = files_fts.rowid
             WHERE files_fts MATCH ?1 LIMIT ?2",
        )?;
        let trigrams = {
            let mut vocab = self
                .conn
                .prepare_cached("SELECT docs FROM trigram_docs WHERE term = ?1")?;
            query_trigrams(&long, |trigram| {
                vocab
                    .query_row([trigram], |row| row.get(0))
                    .optional()
                    .ok()
                    .flatten()
            })
        };
        let limit_candidates = i64::try_from(MAX_CANDIDATES).unwrap_or(i64::MAX);
        let rows = statement.query_map(params![fts_query(&trigrams), limit_candidates], |row| {
            Ok(Entry {
                path: PathBuf::from(row.get::<_, String>(0)?),
                name: row.get(1)?,
                is_dir: row.get(2)?,
            })
        })?;
        let mut matcher = Matcher::new(query);
        let mut hits = Vec::new();
        for entry in rows {
            let entry = entry?;
            let lower = entry.path.to_string_lossy().to_lowercase();
            if !lowered.iter().all(|w| lower.contains(w.as_str())) {
                continue;
            }
            let score = match matcher.score(&entry.name) {
                Some(m) => i64::from(m.score),
                None => match matcher.score(&entry.path.to_string_lossy()) {
                    Some(m) => i64::from(m.score) - PATH_PENALTY,
                    // Every word is in the path, just not as one fuzzy run: rank it last.
                    None => i64::MIN / 2,
                },
            };
            hits.push(Hit { entry, score });
        }
        hits.sort_by(|a, b| {
            b.score.cmp(&a.score).then_with(|| {
                let (a, b) = (a.entry.path.as_os_str(), b.entry.path.as_os_str());
                a.len().cmp(&b.len()).then_with(|| a.cmp(b))
            })
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

/// Every entry under `roots` that `options` allows (the roots themselves excluded), walked
/// in parallel.
fn walk(options: &Options, roots: &[PathBuf], cancel: &Cancel) -> Result<Vec<Entry>, IndexError> {
    let Some((first, rest)) = roots.split_first() else {
        return Ok(Vec::new());
    };
    let mut builder = ignore::WalkBuilder::new(first);
    for root in rest {
        builder.add(root);
    }
    let filter = options.clone();
    builder
        .hidden(!options.hidden)
        .git_ignore(options.gitignore)
        .git_exclude(options.gitignore)
        .git_global(false)
        .ignore(false)
        .parents(options.gitignore)
        .follow_links(false)
        .filter_entry(move |entry| {
            let name = entry.file_name().to_string_lossy();
            !filter.exclude.iter().any(|e| *e == name)
                && !filter.skip.iter().any(|s| entry.path().starts_with(s))
        });
    let (sender, receiver) = mpsc::channel();
    builder.build_parallel().run(|| {
        let sender = sender.clone();
        let cancel = cancel.clone();
        Box::new(move |result| {
            if cancel.is_cancelled() {
                return ignore::WalkState::Quit;
            }
            if let Ok(entry) = result
                && entry.depth() > 0
            {
                let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
                let path = entry.into_path();
                let _ = sender.send(Entry {
                    name: name_of(&path),
                    path,
                    is_dir,
                });
            }
            ignore::WalkState::Continue
        })
    });
    drop(sender);
    if cancel.is_cancelled() {
        return Err(IndexError::Cancelled);
    }
    let mut entries: Vec<Entry> = receiver.into_iter().collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries.dedup_by(|a, b| a.path == b.path);
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(paths: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for path in paths {
            let full = dir.path().join(path);
            if path.ends_with('/') {
                std::fs::create_dir_all(&full).unwrap();
            } else {
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(&full, "x").unwrap();
            }
        }
        dir
    }

    fn names(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.entry.name.as_str()).collect()
    }

    #[test]
    fn walks_honouring_hidden_excluded_and_gitignore() {
        let dir = tree(&[
            "notes/report.md",
            "notes/.secret.md",
            ".hidden/inside.md",
            "code/app/node_modules/dep/index.js",
            "code/app/.git/HEAD",
            "code/app/.gitignore",
            "code/app/out/built.js",
            "code/app/src/main.rs",
        ]);
        std::fs::write(dir.path().join("code/app/.gitignore"), "out/\n").unwrap();
        let mut index = Index::in_memory().unwrap();
        let options = Options::new(vec![dir.path().to_path_buf()]);
        let stats = index.rebuild(&options, &Cancel::new()).unwrap();
        let has = |p: &str| index.contains(&dir.path().join(p)).unwrap();
        assert!(has("notes/report.md") && has("code/app/src/main.rs") && has("code/app/src"));
        for missing in [
            "notes/.secret.md",
            ".hidden/inside.md",
            "code/app/node_modules/dep/index.js",
            "code/app/.git/HEAD",
            "code/app/out/built.js",
        ] {
            assert!(!has(missing), "{missing}");
        }
        // notes, code, code/app, code/app/src.
        assert_eq!((stats.files, stats.dirs), (2, 4), "{stats:?}");
    }

    #[test]
    fn search_ranks_names_before_paths() {
        let dir = tree(&[
            "docs/quickhop-report.md",
            "quickhop/notes.txt",
            "quickhop/report/q3.md",
            "other/report-final.pdf",
        ]);
        let mut index = Index::in_memory().unwrap();
        index
            .rebuild(
                &Options::new(vec![dir.path().to_path_buf()]),
                &Cancel::new(),
            )
            .unwrap();
        assert_eq!(
            names(&index.search("report", 10).unwrap()),
            vec!["report", "report-final.pdf", "quickhop-report.md", "q3.md"]
        );
        // Both words must appear; `.m` is short, so it filters instead of using the index
        // (and, unlike letters, cannot occur in the random temporary folder name).
        assert_eq!(
            names(&index.search("report .m", 10).unwrap()),
            vec!["quickhop-report.md", "q3.md"]
        );
        assert_eq!(
            names(&index.search("REPORT final", 10).unwrap()),
            vec!["report-final.pdf"]
        );
        assert_eq!(index.search("re", 10).unwrap(), Vec::<Hit>::new());
        assert_eq!(index.search("zzzz", 10).unwrap(), Vec::<Hit>::new());
        assert_eq!(index.search("report", 2).unwrap().len(), 2);
        assert_eq!(index.search("a\"b\" OR c", 10).unwrap(), Vec::<Hit>::new());
    }

    #[test]
    fn refresh_adds_and_removes_subtrees() {
        let dir = tree(&["keep.txt", "box/"]);
        let options = Options::new(vec![dir.path().to_path_buf()]);
        let mut index = Index::in_memory().unwrap();
        index.rebuild(&options, &Cancel::new()).unwrap();
        let moved = dir.path().join("box/inner");
        std::fs::create_dir_all(moved.join("deep")).unwrap();
        std::fs::write(moved.join("deep/file.txt"), "x").unwrap();
        index.refresh(&options, &moved).unwrap();
        assert!(index.contains(&moved.join("deep/file.txt")).unwrap());
        assert_eq!(
            names(&index.search("file.txt", 5).unwrap()),
            vec!["file.txt"]
        );
        std::fs::remove_dir_all(&moved).unwrap();
        index.refresh(&options, &moved).unwrap();
        assert!(!index.contains(&moved.join("deep/file.txt")).unwrap());
        assert_eq!(index.search("file.txt", 5).unwrap(), Vec::<Hit>::new());
        // A sibling whose name extends the removed one survives.
        assert!(index.contains(&dir.path().join("keep.txt")).unwrap());
        assert_eq!(index.len().unwrap(), 2);
    }

    #[test]
    fn ignored_paths() {
        let options = Options {
            skip: vec![PathBuf::from("/h/.local/share/quickhop")],
            hidden: true,
            ..Options::new(vec![PathBuf::from("/h")])
        };
        assert!(options.is_ignored(Path::new("/h/.local/share/quickhop/index.sqlite")));
        assert!(options.is_ignored(Path::new("/h/code/target/debug")));
        assert!(options.is_ignored(Path::new("/elsewhere/x")));
        assert!(!options.is_ignored(Path::new("/h/.config/x")));
        assert!(Options::new(vec![PathBuf::from("/h")]).is_ignored(Path::new("/h/.config/x")));
    }

    #[test]
    fn cancelled_rebuild_keeps_the_old_index() {
        let dir = tree(&["a.txt"]);
        let options = Options::new(vec![dir.path().to_path_buf()]);
        let mut index = Index::in_memory().unwrap();
        index.rebuild(&options, &Cancel::new()).unwrap();
        let cancel = Cancel::new();
        cancel.cancel();
        assert!(matches!(
            index.rebuild(&options, &cancel),
            Err(IndexError::Cancelled)
        ));
        assert_eq!(index.len().unwrap(), 1);
    }
}
