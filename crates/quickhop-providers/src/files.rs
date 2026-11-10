//! File and folder search by name (FIL-01) with the file actions (FIL-02).
//!
//! Paths come from a [`FileSource`]: Spotlight on macOS ([`Spotlight`], `mdfind`), and
//! elsewhere quickhop's own file index ([`Indexed`], built by `quickhop index` and kept current
//! by the daemon). Queries shorter than [`MIN_QUERY_CHARS`] are not sent to the source: one or
//! two letters match most of a home directory and would drown the other providers.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use quickhop_core::{Action, Cancel, Item, ItemId, Provider, ProviderError, Query};
use quickhop_index::Index;

/// Provider name.
pub const NAME: &str = "files";

/// Shortest query, in characters, that searches files.
pub const MIN_QUERY_CHARS: usize = 3;

/// Most paths taken from the source for one query; ranking picks the best of them.
pub const MAX_PATHS: usize = 200;

/// How long one Spotlight query may take before it is abandoned.
pub const SPOTLIGHT_TIMEOUT: Duration = Duration::from_secs(2);

/// Where file paths come from.
pub trait FileSource: Send + Sync {
    /// Paths whose file name contains `text` (case-insensitively), at most `limit` of them.
    ///
    /// # Errors
    /// [`ProviderError::Cancelled`] when `cancel` is set while searching, or any failure
    /// of the source.
    fn find(
        &self,
        text: &str,
        limit: usize,
        cancel: &Cancel,
    ) -> Result<Vec<PathBuf>, ProviderError>;

    /// Whether `path` is one of the files this source can find: what `[[commands]]` ids of
    /// the `files` provider are checked against. The default asks the file system.
    ///
    /// # Errors
    /// When the source cannot be read.
    fn contains(&self, path: &Path) -> Result<bool, ProviderError> {
        Ok(path.is_absolute() && path.exists())
    }
}

/// quickhop's own file index (`quickhop-index`), as `quickhop index` and the daemon keep it.
pub struct Indexed {
    index: Mutex<Index>,
}

impl Indexed {
    /// Searches `index`.
    pub fn new(index: Index) -> Self {
        Self {
            index: Mutex::new(index),
        }
    }

    /// The index at `path`, when it exists (it is never created here).
    ///
    /// # Errors
    /// When the file exists but cannot be opened as an index.
    pub fn open(path: &Path) -> Result<Option<Self>, String> {
        if !path.is_file() {
            return Ok(None);
        }
        Index::open(path)
            .map(|index| Some(Self::new(index)))
            .map_err(|error| error.to_string())
    }
}

impl FileSource for Indexed {
    fn find(
        &self,
        text: &str,
        limit: usize,
        cancel: &Cancel,
    ) -> Result<Vec<PathBuf>, ProviderError> {
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        let hits = self
            .index
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .search(text, limit)
            .map_err(|error| other(error.to_string()))?;
        Ok(hits.into_iter().map(|hit| hit.entry.path).collect())
    }

    fn contains(&self, path: &Path) -> Result<bool, ProviderError> {
        self.index
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(path)
            .map_err(|error| other(error.to_string()))
    }
}

/// Spotlight's index, queried with `mdfind -onlyin <root> -name <text>`.
#[derive(Debug, Clone)]
pub struct Spotlight {
    root: PathBuf,
    timeout: Duration,
}

impl Spotlight {
    /// Searches below `root` (the home directory), giving up after `timeout`.
    pub fn new(root: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            root: root.into(),
            timeout,
        }
    }
}

fn other(message: impl Into<String>) -> ProviderError {
    ProviderError::Other {
        provider: NAME.into(),
        message: message.into(),
    }
}

fn cancelled() -> ProviderError {
    ProviderError::Cancelled {
        provider: NAME.into(),
    }
}

impl FileSource for Spotlight {
    fn find(
        &self,
        text: &str,
        limit: usize,
        cancel: &Cancel,
    ) -> Result<Vec<PathBuf>, ProviderError> {
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        let mut child = Command::new("mdfind")
            .arg("-onlyin")
            .arg(&self.root)
            .arg("-name")
            .arg(text)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| ProviderError::Io {
                provider: NAME.into(),
                source,
            })?;
        let (sender, receiver) = mpsc::channel();
        if let Some(stdout) = child.stdout.take() {
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        let deadline = Instant::now() + self.timeout;
        let mut paths = Vec::new();
        let outcome = loop {
            if cancel.is_cancelled() {
                break Err(cancelled());
            }
            if paths.len() >= limit {
                break Ok(());
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                break Err(other(format!(
                    "Spotlight gave no answer within {:?}",
                    self.timeout
                )));
            };
            match receiver.recv_timeout(left.min(Duration::from_millis(10))) {
                Ok(line) if !line.is_empty() => paths.push(PathBuf::from(line)),
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(()),
            }
        };
        // Stops a search that is still running; one that already exited cannot be killed,
        // and either way it is reaped below.
        let _ = child.kill();
        let status = child.wait().map_err(|source| ProviderError::Io {
            provider: NAME.into(),
            source,
        })?;
        outcome?;
        // Killed above because enough paths arrived, or finished on its own: only a search
        // that ended by itself with nothing and a failure status is an error.
        if paths.is_empty() && status.code().is_some_and(|code| code != 0) {
            return Err(other(format!("mdfind failed ({status})")));
        }
        Ok(paths)
    }
}

/// The file actions for one OS, the first being the default: open, reveal in the file
/// manager, copy the path, Quick Look (macOS), move to the trash.
fn actions(os: &str, path: &str) -> Vec<Action> {
    let mut actions = vec![
        Action::Open {
            target: path.into(),
        },
        Action::Reveal { path: path.into() },
        Action::Copy { text: path.into() },
    ];
    if os == "macos" {
        actions.push(Action::Run {
            program: "qlmanage".into(),
            args: vec!["-p".into(), path.into()],
            confirm: false,
        });
    }
    actions.push(Action::Trash { path: path.into() });
    actions
}

/// `dir` with the home directory written as `~`.
fn display_dir(dir: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| dir.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    }
}

/// Files and folders by name from one [`FileSource`].
pub struct FilesProvider {
    source: Box<dyn FileSource>,
    os: String,
    home: Option<PathBuf>,
}

impl FilesProvider {
    /// Searches `source`; `os` picks the file actions, `home` shortens the shown folders.
    pub fn new(source: Box<dyn FileSource>, os: &str, home: Option<PathBuf>) -> Self {
        Self {
            source,
            os: os.into(),
            home,
        }
    }

    /// The item for `path`; its full path is a keyword, so the folder's name matches too
    /// (`work report` finds `~/work/quarterly-report.md`).
    fn item_for(&self, path: &Path) -> Option<Item> {
        let name = path.file_name()?.to_string_lossy().into_owned();
        let text = path.to_str()?;
        let mut actions = actions(&self.os, text).into_iter();
        let mut item = Item::new(ItemId::new(NAME, text), name, actions.next()?)
            .with_keywords([text.to_owned()]);
        item.actions.extend(actions);
        Some(match path.parent() {
            Some(dir) => item.with_subtitle(display_dir(dir, self.home.as_deref())),
            None => item,
        })
    }
}

impl Provider for FilesProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn items(&self, query: &Query, cancel: &Cancel) -> Result<Vec<Item>, ProviderError> {
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        let text = query.text();
        if text.chars().count() < MIN_QUERY_CHARS {
            return Ok(Vec::new());
        }
        let paths = self.source.find(text, MAX_PATHS, cancel)?;
        let mut seen = std::collections::HashSet::new();
        Ok(paths
            .iter()
            .filter(|path| seen.insert(path.as_path()))
            .filter_map(|path| self.item_for(path))
            .collect())
    }

    fn item(&self, key: &str) -> Result<Option<Item>, ProviderError> {
        let path = Path::new(key);
        Ok(if self.source.contains(path)? {
            self.item_for(path)
        } else {
            None
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers with fixed paths and records the texts it was asked for.
    struct Fixed {
        paths: Vec<PathBuf>,
        asked: Mutex<Vec<String>>,
    }

    impl FileSource for Fixed {
        fn find(
            &self,
            text: &str,
            limit: usize,
            _: &Cancel,
        ) -> Result<Vec<PathBuf>, ProviderError> {
            if let Ok(mut asked) = self.asked.lock() {
                asked.push(text.to_owned());
            }
            Ok(self.paths.iter().take(limit).cloned().collect())
        }
    }

    fn provider(os: &str, paths: &[&str]) -> FilesProvider {
        let source = Fixed {
            paths: paths.iter().map(PathBuf::from).collect(),
            asked: Mutex::new(Vec::new()),
        };
        FilesProvider::new(Box::new(source), os, Some(PathBuf::from("/home/ada")))
    }

    fn items(provider: &FilesProvider, text: &str) -> Vec<Item> {
        provider
            .items(&Query::new(text), &Cancel::new())
            .unwrap_or_default()
    }

    #[test]
    fn short_queries_never_reach_the_source() {
        let source = Fixed {
            paths: vec![PathBuf::from("/home/ada/a.txt")],
            asked: Mutex::new(Vec::new()),
        };
        let provider = FilesProvider::new(Box::new(source), "linux", None);
        assert_eq!(items(&provider, "ab").len(), 0);
        assert_eq!(
            items(&provider, " ab ").len(),
            0,
            "surrounding spaces do not count"
        );
        assert_eq!(items(&provider, "abc").len(), 1);
    }

    #[test]
    fn items_show_the_name_and_the_folder_under_home() {
        let provider = provider(
            "linux",
            &[
                "/home/ada/docs/plan.md",
                "/home/ada/plan.md",
                "/srv/plan.md",
            ],
        );
        let found = items(&provider, "plan");
        let shown: Vec<_> = found
            .iter()
            .map(|i| (i.title.as_str(), i.subtitle.as_deref()))
            .collect();
        assert_eq!(
            shown,
            [
                ("plan.md", Some("~/docs")),
                ("plan.md", Some("~")),
                ("plan.md", Some("/srv")),
            ]
        );
        assert_eq!(found[0].id, ItemId::new(NAME, "/home/ada/docs/plan.md"));
    }

    #[test]
    fn duplicate_paths_become_one_item() {
        let provider = provider("macos", &["/Users/ada/a.txt", "/Users/ada/a.txt"]);
        assert_eq!(items(&provider, "a.txt").len(), 1);
    }

    #[test]
    fn macos_actions_open_reveal_copy_preview_and_trash() {
        let provider = provider("macos", &["/Users/ada/My File.pdf"]);
        let found = items(&provider, "file");
        let path = "/Users/ada/My File.pdf";
        assert_eq!(
            found[0].actions,
            [
                Action::Open {
                    target: path.into()
                },
                Action::Reveal { path: path.into() },
                Action::Copy { text: path.into() },
                Action::Run {
                    program: "qlmanage".into(),
                    args: vec!["-p".into(), path.into()],
                    confirm: false
                },
                Action::Trash { path: path.into() },
            ]
        );
    }

    #[test]
    fn linux_actions_have_no_quick_look() {
        let provider = provider("linux", &["/home/ada/My File #1.pdf"]);
        let found = items(&provider, "file");
        let path = "/home/ada/My File #1.pdf";
        assert_eq!(
            found[0].actions,
            [
                Action::Open {
                    target: path.into()
                },
                Action::Reveal { path: path.into() },
                Action::Copy { text: path.into() },
                Action::Trash { path: path.into() },
            ]
        );
    }

    #[test]
    fn a_missing_index_is_no_source_and_an_empty_one_finds_nothing() {
        assert!(matches!(
            Indexed::open(Path::new("/nonexistent/index.sqlite")),
            Ok(None)
        ));
        let Ok(index) = Index::in_memory() else {
            panic!("no in-memory index");
        };
        let provider = FilesProvider::new(Box::new(Indexed::new(index)), "linux", None);
        assert_eq!(items(&provider, "report"), Vec::<Item>::new());
        assert!(matches!(provider.item("/home/ada/report.txt"), Ok(None)));
    }

    #[test]
    fn paths_without_a_name_are_skipped() {
        let provider = provider("linux", &["/", "/home/ada/x.txt"]);
        let titles: Vec<_> = items(&provider, "txt")
            .into_iter()
            .map(|i| i.title)
            .collect();
        assert_eq!(titles, ["x.txt"]);
    }
}
