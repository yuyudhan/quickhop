//! Reload on save: the daemon watches the folder of `config.toml` and reads the files again
//! once `config.toml` or `snippets.toml` changed and stayed unchanged for [`QUIET`], so a
//! settings change, an editor's save or a `git pull` of a dotfiles repository applies without
//! `quickhop reload`. Editors that save by replacing the file are seen too: the folder is
//! watched, not the file.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecursiveMode, Watcher as _};

/// How long the files must stay unchanged before they are read: an editor's save is often
/// several events (write, rename, chmod).
const QUIET: Duration = Duration::from_millis(300);

/// The files whose changes reload the daemon, by name, in the config folder.
const WATCHED: [&str; 2] = ["config.toml", "snippets.toml"];

/// A running watch; dropping it stops it.
pub(crate) struct ConfigWatch {
    _watcher: notify::RecommendedWatcher,
}

/// The contents of the watched files next to `config` (`None` for a missing one): what a
/// change is measured against, so events that change nothing (an editor saving the same text,
/// a `touch`) reload nothing.
fn contents(config: &Path) -> Vec<Option<Vec<u8>>> {
    let dir = config.parent().unwrap_or_else(|| Path::new("."));
    files(config, dir)
        .iter()
        .map(|path| std::fs::read(path).ok())
        .collect()
}

/// The watched files: `config` itself (whatever its name, with `QUICKHOP_CONFIG`) and
/// `snippets.toml` in `dir`.
fn files(config: &Path, dir: &Path) -> Vec<PathBuf> {
    vec![config.to_path_buf(), dir.join(WATCHED[1])]
}

/// Whether `event` touches one of the watched files of `config`.
fn concerns(event: &notify::Event, config: &Path) -> bool {
    let name = config.file_name();
    event.paths.iter().any(|path| {
        let file = path.file_name();
        file.is_some()
            && (file == name
                || WATCHED
                    .iter()
                    .any(|w| file == Some(std::ffi::OsStr::new(w))))
    })
}

/// Watches the folder of `config` and calls `reload` on a thread of its own after the watched
/// files changed. The folder is created when missing, so a config written later is seen.
///
/// # Errors
/// When the folder cannot be created or watched.
pub(crate) fn start(
    config: &Path,
    reload: impl Fn() + Send + 'static,
) -> Result<ConfigWatch, String> {
    let dir = config
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (events, changes) = mpsc::channel::<()>();
    let file = config.to_path_buf();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && concerns(&event, &file)
        {
            let _ = events.send(());
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(&dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let config = config.to_path_buf();
    // Read here, once the folder is watched, not on the thread: a save right after `start`
    // returned would otherwise be in the baseline whenever the thread runs late, and reload
    // nothing.
    let mut last = contents(&config);
    std::thread::Builder::new()
        .name("quickhop-config-watch".into())
        .spawn(move || {
            // Ends when the watcher, and with it the sender, is dropped.
            while changes.recv().is_ok() {
                while changes.recv_timeout(QUIET).is_ok() {}
                let now = contents(&config);
                if now != last {
                    last = now;
                    reload();
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(ConfigWatch { _watcher: watcher })
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// The reloads of a watch, counted as they arrive.
    struct Reloads {
        arrived: mpsc::Receiver<()>,
        seen: usize,
    }

    impl Reloads {
        fn watch(config: &Path) -> (ConfigWatch, Self) {
            let (reloaded, arrived) = mpsc::channel();
            let watch = start(config, move || {
                let _ = reloaded.send(());
            })
            .unwrap();
            (watch, Self { arrived, seen: 0 })
        }

        /// Waits until `at_least` reloads arrived (10 s at most), then counts those already
        /// queued too, and returns how many arrived in all.
        fn wait_for(&mut self, at_least: usize) -> usize {
            let deadline = Instant::now() + Duration::from_secs(10);
            while self.seen < at_least {
                let left = deadline.saturating_duration_since(Instant::now());
                if self.arrived.recv_timeout(left).is_err() {
                    break;
                }
                self.seen += 1;
            }
            while self.arrived.try_recv().is_ok() {
                self.seen += 1;
            }
            self.seen
        }
    }

    #[test]
    fn saving_a_watched_file_reloads_once_and_other_files_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "max_results = 8\n").unwrap();
        let (watch, mut reloads) = Reloads::watch(&config);

        std::fs::write(dir.path().join("notes.txt"), "unrelated").unwrap();
        std::fs::write(&config, "max_results = 9\n").unwrap();
        assert_eq!(reloads.wait_for(1), 1);
        // The same text again changes nothing.
        std::fs::write(&config, "max_results = 9\n").unwrap();
        std::fs::write(dir.path().join("snippets.toml"), "").unwrap();
        assert_eq!(reloads.wait_for(2), 2, "a new snippets.toml reloads");
        // Replaced by rename, as editors save.
        let temporary = dir.path().join(".config.toml.swp");
        std::fs::write(&temporary, "max_results = 10\n").unwrap();
        std::fs::rename(&temporary, &config).unwrap();
        assert_eq!(reloads.wait_for(3), 3);
        drop(watch);
    }

    #[test]
    fn a_missing_folder_is_created_and_watched() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("quickhop/config.toml");
        let (_watch, mut reloads) = Reloads::watch(&config);
        std::fs::write(&config, "theme = \"dark\"\n").unwrap();
        assert_eq!(reloads.wait_for(1), 1);
    }

    #[test]
    fn a_save_right_after_start_reloads() {
        // A change is measured against the files as they were when `start` returned, however
        // late its thread runs. Many watches at once, so a save lost to that race shows.
        let dirs: Vec<_> = (0..16).map(|_| tempfile::tempdir().unwrap()).collect();
        let mut watches: Vec<_> = dirs
            .iter()
            .map(|dir| {
                let config = dir.path().join("config.toml");
                std::fs::write(&config, "max_results = 8\n").unwrap();
                let watch = Reloads::watch(&config);
                std::fs::write(&config, "max_results = 9\n").unwrap();
                watch
            })
            .collect();
        for (_, reloads) in &mut watches {
            assert_eq!(reloads.wait_for(1), 1);
        }
    }
}
