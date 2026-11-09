//! `quickhop clipboard`: records copied text into the history in `state.sqlite`, lists and
//! clears it.

use std::io::{self, Read, Write};
use std::path::PathBuf;

use quickhop_core::Config;
use quickhop_core::config::data_dir;
use quickhop_store::clipboard::{History, HistoryError, Retention};

/// What `quickhop clipboard` does.
#[derive(Debug, clap::Subcommand)]
pub(crate) enum ClipboardAction {
    /// Record every text copied from now on until stopped: X11, or Wayland compositors with
    /// the data-control protocol (sway, Hyprland, `KWin`, COSMIC) through `wl-paste --watch`.
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
    /// Delete every entry.
    Clear,
}

/// Why a clipboard command failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ClipboardError {
    #[error(transparent)]
    History(#[from] HistoryError),
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
    #[error("cannot write output: {0}")]
    Output(#[from] io::Error),
    #[cfg(all(unix, not(target_os = "macos")))]
    #[error(transparent)]
    X11(#[from] quickhop_platform_linux::clipboard::ClipboardError),
    #[error("cannot start wl-paste (install wl-clipboard): {0}")]
    WlPaste(io::Error),
    #[error(
        "wl-paste stopped ({0}): the compositor may lack the data-control protocol; GNOME has none"
    )]
    WlPasteFailed(String),
    #[error("{0}")]
    Unsupported(&'static str),
}

/// Runs `action` with the retention limits of `config`.
pub(crate) fn run(
    action: &ClipboardAction,
    config: &Config,
    now: impl Fn() -> i64,
    out: &mut impl Write,
) -> Result<(), ClipboardError> {
    let retention = Retention {
        max_entries: config.clipboard.max_entries,
        max_age_days: config.clipboard.max_age_days,
    };
    let mut history = open()?;
    match action {
        ClipboardAction::Watch => watch(&mut history, retention, now, out)?,
        ClipboardAction::Add => {
            let mut bytes = Vec::new();
            io::stdin()
                .read_to_end(&mut bytes)
                .map_err(ClipboardError::Stdin)?;
            history.add(&String::from_utf8_lossy(&bytes), now(), retention)?;
        }
        ClipboardAction::List { json, limit } => {
            let entries = history.recent(*limit)?;
            if *json {
                let values: Vec<serde_json::Value> = entries
                    .iter()
                    .map(|e| {
                        serde_json::json!({
                            "id": e.id,
                            "text": e.text,
                            "first_copied": e.first_copied,
                            "last_copied": e.last_copied,
                            "copies": e.copies,
                        })
                    })
                    .collect();
                serde_json::to_writer_pretty(&mut *out, &values).map_err(io::Error::other)?;
                writeln!(out)?;
            } else {
                for entry in &entries {
                    writeln!(out, "{}", one_line(&entry.text))?;
                }
            }
        }
        ClipboardAction::Clear => history.clear()?,
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
fn open() -> Result<History, ClipboardError> {
    let dir = data_dir(std::env::consts::OS, |name| std::env::var(name).ok())?;
    std::fs::create_dir_all(&dir).map_err(|source| ClipboardError::DataDir {
        path: dir.clone(),
        source,
    })?;
    Ok(History::open(&dir.join("state.sqlite"))?)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn watch(
    history: &mut History,
    retention: Retention,
    now: impl Fn() -> i64,
    out: &mut impl Write,
) -> Result<(), ClipboardError> {
    use std::ops::ControlFlow;

    if var("WAYLAND_DISPLAY").is_some() {
        // wl-paste runs `quickhop clipboard add` with the new text on stdin for every copy.
        let exe = std::env::current_exe()?;
        writeln!(out, "watching the Wayland clipboard (Ctrl+C stops)")?;
        out.flush()?;
        let status = std::process::Command::new("wl-paste")
            .args(["--type", "text", "--watch"])
            .arg(exe)
            .args(["clipboard", "add"])
            .status()
            .map_err(ClipboardError::WlPaste)?;
        return if status.success() {
            Ok(())
        } else {
            Err(ClipboardError::WlPasteFailed(status.to_string()))
        };
    }
    if var("DISPLAY").is_none() {
        return Err(ClipboardError::Unsupported(
            "no graphical session: DISPLAY and WAYLAND_DISPLAY are unset",
        ));
    }
    writeln!(out, "watching the X11 clipboard (Ctrl+C stops)")?;
    out.flush()?;
    quickhop_platform_linux::clipboard::watch_x11(None, |text| {
        if let Err(error) = history.add(&text, now(), retention) {
            let _ = writeln!(io::stderr(), "quickhop: {error}");
        }
        ControlFlow::Continue(())
    })?;
    Ok(())
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn watch(
    _history: &mut History,
    _retention: Retention,
    _now: impl Fn() -> i64,
    _out: &mut impl Write,
) -> Result<(), ClipboardError> {
    Err(ClipboardError::Unsupported(
        "watching the clipboard on this system comes with the quickhop daemon",
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
}
