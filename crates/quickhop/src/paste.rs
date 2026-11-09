//! Pasting text into the focused application (CLP-03): the text goes on the clipboard, then
//! quickhop presses Ctrl+V for the user through `XTest` on X11. Where no synthetic key press
//! is possible yet (Wayland, until the virtual-keyboard protocol and the `RemoteDesktop`
//! portal; macOS and Windows until their slices) the text stays on the clipboard and the
//! caller says so.

use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::launch::clipboard_text;

/// How long the clipboard may take to hold the new text.
const SET_TIMEOUT: Duration = Duration::from_secs(1);

/// What happened to the text.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// It was pasted into the focused application.
    #[cfg(all(unix, not(target_os = "macos")))]
    Pasted,
    /// It is on the clipboard only, for this reason.
    Copied(&'static str),
}

/// Why pasting failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PasteError {
    #[error("no graphical session: DISPLAY and WAYLAND_DISPLAY are unset")]
    NoSession,
    #[error("cannot start {program} (install it to paste): {source}")]
    Spawn {
        program: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("{0} could not set the clipboard")]
    SetFailed(&'static str),
    #[error("the clipboard did not take the text within a second")]
    NotSet,
    #[cfg(all(unix, not(target_os = "macos")))]
    #[error(transparent)]
    X11(#[from] quickhop_platform_linux::clipboard::ClipboardError),
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The program that copies its standard input to the clipboard here.
fn copier() -> Result<(&'static str, &'static [&'static str]), PasteError> {
    Ok(match std::env::consts::OS {
        "macos" => ("pbcopy", &[]),
        "windows" => (
            "powershell.exe",
            &[
                "-NoProfile",
                "-Command",
                "Set-Clipboard -Value ([Console]::In.ReadToEnd())",
            ],
        ),
        _ if var("WAYLAND_DISPLAY").is_some() => ("wl-copy", &[]),
        _ if var("DISPLAY").is_some() => ("xclip", &["-selection", "clipboard", "-in"]),
        _ => return Err(PasteError::NoSession),
    })
}

/// Puts `text` on the clipboard and waits until the clipboard holds it.
fn set_clipboard(text: &str) -> Result<(), PasteError> {
    let (program, args) = copier()?;
    let spawn_error = |source| PasteError::Spawn { program, source };
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(spawn_error)?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes()).map_err(spawn_error)?;
    }
    // wl-copy and xclip leave a background process serving the text, then exit.
    if !child.wait().map_err(spawn_error)?.success() {
        return Err(PasteError::SetFailed(program));
    }
    let deadline = Instant::now() + SET_TIMEOUT;
    while clipboard_text().as_deref() != Some(text) {
        if Instant::now() > deadline {
            return Err(PasteError::NotSet);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

/// Presses Ctrl+V in the session, or says why it cannot.
#[cfg(all(unix, not(target_os = "macos")))]
fn press_paste() -> Result<Outcome, PasteError> {
    if var("WAYLAND_DISPLAY").is_some() {
        return Ok(Outcome::Copied(
            "synthetic paste on Wayland comes later: press Ctrl+V",
        ));
    }
    quickhop_platform_linux::keyboard::press_ctrl_v(None)?;
    Ok(Outcome::Pasted)
}

/// Pastes `text` into the focused application, or leaves it on the clipboard and says why.
///
/// # Errors
/// When there is no clipboard to use, the helper tool is missing, or X11 refuses.
pub(crate) fn paste(text: &str) -> Result<Outcome, PasteError> {
    set_clipboard(text)?;
    #[cfg(all(unix, not(target_os = "macos")))]
    let outcome = press_paste()?;
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let outcome = Outcome::Copied(
        "synthetic paste on macOS and Windows comes with their slices: press the paste key",
    );
    Ok(outcome)
}

/// Pastes `text` and tells the user when it was only copied.
///
/// # Errors
/// As [`paste`].
pub(crate) fn paste_and_report(text: &str) -> Result<(), PasteError> {
    match paste(text)? {
        #[cfg(all(unix, not(target_os = "macos")))]
        Outcome::Pasted => {}
        Outcome::Copied(why) => {
            let _ = writeln!(io::stderr(), "quickhop: copied, not pasted: {why}");
        }
    }
    Ok(())
}
