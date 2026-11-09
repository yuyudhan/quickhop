//! File and folder search by name (FIL-01) with the file actions (FIL-02).
//!
//! Paths come from a [`FileSource`]: Spotlight on macOS ([`Spotlight`], `mdfind`), and on
//! Linux quickhop's own index once it exists (plan slice F1). Queries shorter than
//! [`MIN_QUERY_CHARS`] are not sent to the source: one or two letters match most of a home
//! directory and would drown the other providers.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use quickhop_core::{Action, Cancel, Item, ItemId, Provider, ProviderError, Query};

/// Provider name.
pub const NAME: &str = "files";

/// Shortest query, in characters, that searches files.
pub const MIN_QUERY_CHARS: usize = 3;

/// Most paths taken from the source for one query; ranking picks the best of them.
pub const MAX_PATHS: usize = 200;

/// How long one Spotlight query may take before it is abandoned.
pub const SPOTLIGHT_TIMEOUT: Duration = Duration::from_secs(2);

/// Characters kept as they are in a `file://` URI path.
const URI_PATH: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

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

/// The file actions for one OS, the first being the default.
fn actions(os: &str, path: &str) -> Vec<Action> {
    let run = |program: &str, args: Vec<String>, confirm: bool| Action::Run {
        program: program.into(),
        args,
        confirm,
    };
    let open = Action::Open {
        target: path.into(),
    };
    let copy = Action::Copy { text: path.into() };
    if os == "macos" {
        return vec![
            open,
            run("open", vec!["-R".into(), path.into()], false),
            copy,
            run("qlmanage", vec!["-p".into(), path.into()], false),
            // `/usr/bin/trash` (macOS 15 and later) moves to the Trash like Finder does,
            // so "Put Back" still works.
            run("trash", vec![path.into()], true),
        ];
    }
    let uri = format!("file://{}", utf8_percent_encode(path, URI_PATH));
    vec![
        open,
        run(
            "dbus-send",
            vec![
                "--session".into(),
                "--dest=org.freedesktop.FileManager1".into(),
                "--type=method_call".into(),
                "/org/freedesktop/FileManager1".into(),
                "org.freedesktop.FileManager1.ShowItems".into(),
                format!("array:string:{uri}"),
                "string:".into(),
            ],
            false,
        ),
        copy,
        run("gio", vec!["trash".into(), path.into()], true),
    ]
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

    fn item(&self, path: &Path) -> Option<Item> {
        let name = path.file_name()?.to_string_lossy().into_owned();
        let text = path.to_str()?;
        let mut actions = actions(&self.os, text).into_iter();
        let mut item = Item::new(ItemId::new(NAME, text), name, actions.next()?);
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
            .filter_map(|path| self.item(path))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

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
                Action::Run {
                    program: "open".into(),
                    args: vec!["-R".into(), path.into()],
                    confirm: false
                },
                Action::Copy { text: path.into() },
                Action::Run {
                    program: "qlmanage".into(),
                    args: vec!["-p".into(), path.into()],
                    confirm: false
                },
                Action::Run {
                    program: "trash".into(),
                    args: vec![path.into()],
                    confirm: true
                },
            ]
        );
    }

    #[test]
    fn linux_reveal_sends_an_encoded_file_uri() {
        let provider = provider("linux", &["/home/ada/My File #1.pdf"]);
        let found = items(&provider, "file");
        let Some(Action::Run { program, args, .. }) = found[0].actions.get(1) else {
            panic!("no reveal action: {:?}", found[0].actions);
        };
        assert_eq!(program, "dbus-send");
        assert!(
            args.contains(&"array:string:file:///home/ada/My%20File%20%231.pdf".to_owned()),
            "{args:?}"
        );
        assert!(found[0].actions.iter().any(|a| matches!(
            a,
            Action::Run { program, confirm: true, .. } if program == "gio"
        )));
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
