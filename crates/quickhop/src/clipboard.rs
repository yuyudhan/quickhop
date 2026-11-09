//! `quickhop clipboard`: records copied text into the history in `state.sqlite`, lists and
//! clears it.
//!
//! Every watcher hands what it saw to [`keep`] as [`Contents`], so the rules are the same on
//! every system: secrets (copies a password manager marks as concealed or transient) and
//! non-text contents are never recorded, blank texts neither.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use quickhop_core::config::data_dir;
use quickhop_core::{Clipboard, Config, Contents};
use quickhop_store::state::{State, StateError};

/// What `quickhop clipboard` does.
#[derive(Debug, clap::Subcommand)]
pub(crate) enum ClipboardAction {
    /// Record every text copied from now on until stopped: macOS, X11, or Wayland
    /// compositors with the data-control protocol (sway, Hyprland, `KWin`, COSMIC) through
    /// `wl-paste --watch`.
    Watch,
    /// Record the text on standard input as copied now.
    Add,
    /// Print the history, newest first.
    List {
        /// Print JSON instead of one line per entry.
        #[arg(long)]
        json: bool,
        /// Most entries printed.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Paste an entry into the focused application (1 is the newest).
    Paste {
        /// Which entry, newest first.
        #[arg(default_value_t = 1)]
        number: usize,
    },
    /// Delete every entry.
    Clear,
}

/// Why a clipboard command failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ClipboardError {
    #[error(transparent)]
    State(#[from] StateError),
    #[error(transparent)]
    Config(#[from] quickhop_core::ConfigError),
    #[error("cannot create {path}: {source}")]
    DataDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot read standard input: {0}")]
    Stdin(io::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[cfg(all(unix, not(target_os = "macos")))]
    #[error(transparent)]
    X11(#[from] quickhop_platform_linux::clipboard::ClipboardError),
    #[cfg(all(unix, not(target_os = "macos")))]
    #[error("cannot start wl-paste (install wl-clipboard): {0}")]
    WlPaste(io::Error),
    #[cfg(all(unix, not(target_os = "macos")))]
    #[error(
        "wl-paste stopped ({0}): the compositor may lack the data-control protocol; GNOME has none"
    )]
    WlPasteFailed(String),
    #[error("{0}")]
    Unsupported(&'static str),
    #[error("no entry {0}: the history holds {1}")]
    NoEntry(usize, usize),
    #[error(transparent)]
    Paste(#[from] crate::paste::PasteError),
}

/// The history settings a running watcher reads at every copy, so a reload takes effect.
pub(crate) type Settings = Arc<Mutex<Clipboard>>;

/// Records `contents`, seen at `now` (Unix milliseconds), when it is worth remembering, then
/// applies the retention of `settings`. Returns whether it was recorded.
///
/// # Errors
/// When SQLite fails.
pub(crate) fn keep(
    state: &mut State,
    contents: Contents,
    settings: &Clipboard,
    now: i64,
) -> Result<bool, StateError> {
    let Some(copied) = contents.into_copied() else {
        return Ok(false);
    };
    let kept = state
        .record_clip(&copied.text, copied.source_app.as_deref(), now)?
        .is_some();
    state.prune_clips(settings.max_age_days, settings.max_entries, now)?;
    Ok(kept)
}

/// Runs `action` with the `[clipboard]` settings of `config`; `now` gives Unix milliseconds.
pub(crate) fn run(
    action: &ClipboardAction,
    config: &Config,
    now: fn() -> i64,
    out: &mut impl Write,
) -> Result<(), ClipboardError> {
    if let ClipboardAction::Watch = action {
        let watching = start(Arc::new(Mutex::new(config.clipboard.clone())), now)?;
        writeln!(
            out,
            "watching the {} clipboard (Ctrl+C stops)",
            watching.session()
        )?;
        out.flush()?;
        return watching.wait();
    }
    let mut state = open()?;
    match action {
        ClipboardAction::Watch => {}
        ClipboardAction::Add => {
            let mut bytes = Vec::new();
            io::stdin()
                .read_to_end(&mut bytes)
                .map_err(ClipboardError::Stdin)?;
            let contents = Contents::Text {
                text: String::from_utf8_lossy(&bytes).into_owned(),
                source_app: None,
            };
            keep(&mut state, contents, &config.clipboard, now())?;
        }
        ClipboardAction::List { json, limit } => {
            let clips = state.clips(*limit)?;
            if *json {
                let values: Vec<serde_json::Value> = clips
                    .iter()
                    .map(|clip| {
                        serde_json::json!({
                            "id": clip.id,
                            "text": clip.text,
                            "source_app": clip.source_app,
                            "first_copied": clip.first_copied,
                            "last_copied": clip.last_copied,
                            "copies": clip.copies,
                        })
                    })
                    .collect();
                serde_json::to_writer_pretty(&mut *out, &values).map_err(io::Error::other)?;
                writeln!(out)?;
            } else {
                for clip in &clips {
                    writeln!(out, "{}", one_line(&clip.text))?;
                }
            }
        }
        ClipboardAction::Paste { number } => {
            let clips = state.clips(*number)?;
            let clip = number
                .checked_sub(1)
                .and_then(|i| clips.get(i))
                .ok_or(ClipboardError::NoEntry(*number, clips.len()))?;
            crate::paste::paste_and_report(&clip.text)?;
        }
        ClipboardAction::Clear => {
            state.clear_clips()?;
        }
    }
    Ok(())
}

/// The first line of `text`, at most 100 characters, with `…` when anything was cut.
fn one_line(text: &str) -> String {
    let first = text.trim_start().lines().next().unwrap_or_default();
    let mut line: String = first.chars().take(100).collect();
    if line.len() < text.trim().len() {
        line.push('…');
    }
    line
}

/// The history in the data directory, created if needed.
pub(crate) fn open() -> Result<State, ClipboardError> {
    let dir = data_dir(std::env::consts::OS, |name| std::env::var(name).ok())?;
    std::fs::create_dir_all(&dir).map_err(|source| ClipboardError::DataDir {
        path: dir.clone(),
        source,
    })?;
    Ok(State::open(&dir.join("state.sqlite"))?)
}

/// Records one copy seen by a watcher thread; problems go to standard error, so one bad
/// copy does not stop the watcher. Watchers exist on Unix only so far.
#[cfg(unix)]
fn keep_reporting(state: &mut State, contents: Contents, settings: &Settings, now: i64) {
    let settings = settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if !settings.enabled {
        return;
    }
    if let Err(error) = keep(state, contents, &settings, now) {
        let _ = writeln!(io::stderr(), "quickhop: {error}");
    }
}

/// A running clipboard watcher.
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) enum Watching {
    /// `wl-paste --watch`, which runs `quickhop clipboard add` for every copy.
    Wayland(std::process::Child),
    /// The X11 watcher thread; it ends with the process or a broken connection.
    X11(std::thread::JoinHandle<Result<(), ClipboardError>>),
}

#[cfg(all(unix, not(target_os = "macos")))]
impl Watching {
    /// The kind of session watched.
    pub(crate) fn session(&self) -> &'static str {
        match self {
            Self::Wayland(_) => "Wayland",
            Self::X11(_) => "X11",
        }
    }

    /// Blocks until the watcher ends, which normally means never.
    pub(crate) fn wait(self) -> Result<(), ClipboardError> {
        match self {
            Self::Wayland(mut child) => {
                let status = child.wait()?;
                if status.success() {
                    Ok(())
                } else {
                    Err(ClipboardError::WlPasteFailed(status.to_string()))
                }
            }
            Self::X11(thread) => thread
                .join()
                .unwrap_or(Err(ClipboardError::Unsupported("the X11 watcher panicked"))),
        }
    }

    /// Stops a Wayland watcher; the X11 thread stops with the process.
    pub(crate) fn stop(&mut self) {
        if let Self::Wayland(child) = self {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Starts recording every copied text into the history: through `wl-paste --watch` on
/// Wayland, else the X11 watcher. `settings` is read at every copy.
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn start(settings: Settings, now: fn() -> i64) -> Result<Watching, ClipboardError> {
    use std::ops::ControlFlow;

    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    if var("WAYLAND_DISPLAY").is_some() {
        // wl-paste runs `quickhop clipboard add` with the new text on stdin for every copy.
        let exe = std::env::current_exe()?;
        let child = std::process::Command::new("wl-paste")
            .args(["--type", "text", "--watch"])
            .arg(exe)
            .args(["clipboard", "add"])
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(ClipboardError::WlPaste)?;
        return Ok(Watching::Wayland(child));
    }
    if var("DISPLAY").is_none() {
        return Err(ClipboardError::Unsupported(
            "no graphical session: DISPLAY and WAYLAND_DISPLAY are unset",
        ));
    }
    let mut state = open()?;
    let thread = std::thread::Builder::new()
        .name("quickhop-clipboard".into())
        .spawn(move || {
            quickhop_platform_linux::clipboard::watch_x11(None, |contents| {
                keep_reporting(&mut state, contents, &settings, now());
                ControlFlow::Continue(())
            })?;
            Ok(())
        })?;
    Ok(Watching::X11(thread))
}

/// How often the macOS pasteboard's change count is read: macOS sends no notification.
#[cfg(target_os = "macos")]
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// A running clipboard watcher: a thread polling the pasteboard.
#[cfg(target_os = "macos")]
pub(crate) struct Watching {
    /// Which pasteboard is watched, for messages.
    label: &'static str,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "macos")]
impl Watching {
    /// The kind of session watched.
    pub(crate) fn session(&self) -> &'static str {
        self.label
    }

    /// Blocks until the watcher ends, which means never unless [`Self::stop`] runs.
    pub(crate) fn wait(mut self) -> Result<(), ClipboardError> {
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| ClipboardError::Unsupported("the clipboard watcher panicked"))?;
        }
        Ok(())
    }

    /// Stops polling.
    pub(crate) fn stop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts recording every copied text into the history by polling the pasteboard through
/// [`quickhop_core::Watcher`]. `settings` is read at every copy.
#[cfg(target_os = "macos")]
pub(crate) fn start(settings: Settings, now: fn() -> i64) -> Result<Watching, ClipboardError> {
    use std::sync::atomic::{AtomicBool, Ordering};

    let mut state = open()?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    let thread = std::thread::Builder::new()
        .name("quickhop-clipboard".into())
        .spawn(move || {
            let pasteboard = crate::paste::pasteboard();
            let mut watcher = quickhop_core::Watcher::new();
            while !stopped.load(Ordering::SeqCst) {
                if let Some(copied) = watcher.poll(&pasteboard) {
                    let contents = Contents::Text {
                        text: copied.text,
                        source_app: copied.source_app,
                    };
                    keep_reporting(&mut state, contents, &settings, now());
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        })?;
    Ok(Watching {
        label: if std::env::var_os("QUICKHOP_PASTEBOARD").is_some() {
            "private macOS"
        } else {
            "macOS"
        },
        stop,
        thread: Some(thread),
    })
}

/// No clipboard watcher exists on this system yet.
#[cfg(not(unix))]
pub(crate) enum Watching {}

#[cfg(not(unix))]
impl Watching {
    /// The kind of session watched.
    pub(crate) fn session(&self) -> &'static str {
        match *self {}
    }

    /// Blocks until the watcher ends.
    pub(crate) fn wait(self) -> Result<(), ClipboardError> {
        match self {}
    }
}

/// Starts recording every copied text into the history.
#[cfg(not(unix))]
pub(crate) fn start(_settings: Settings, _now: fn() -> i64) -> Result<Watching, ClipboardError> {
    Err(ClipboardError::Unsupported(
        "watching the clipboard on this system comes with the Windows slice",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_cuts_long_and_multi_line_texts() {
        assert_eq!(one_line("  hello\n"), "hello");
        assert_eq!(one_line("first\nsecond"), "first…");
        assert_eq!(one_line(&"é".repeat(150)), format!("{}…", "é".repeat(100)));
    }

    #[test]
    fn secrets_and_non_text_are_never_kept() {
        let mut state = State::in_memory().unwrap();
        let settings = Clipboard::default();
        for contents in [Contents::Secret, Contents::Other] {
            assert!(!keep(&mut state, contents, &settings, 1).unwrap());
        }
        let text = Contents::Text {
            text: "kept".into(),
            source_app: Some("com.apple.TextEdit".into()),
        };
        assert!(keep(&mut state, text, &settings, 2).unwrap());
        let clips = state.clips(10).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].source_app.as_deref(), Some("com.apple.TextEdit"));
    }

    #[test]
    fn retention_applies_after_every_copy() {
        let mut state = State::in_memory().unwrap();
        let settings = Clipboard {
            max_entries: 2,
            ..Clipboard::default()
        };
        for (now, text) in [(1, "a"), (2, "b"), (3, "c")] {
            let contents = Contents::Text {
                text: text.into(),
                source_app: None,
            };
            keep(&mut state, contents, &settings, now).unwrap();
        }
        let texts: Vec<String> = state
            .clips(10)
            .unwrap()
            .into_iter()
            .map(|c| c.text)
            .collect();
        assert_eq!(texts, ["c", "b"]);
    }
}
