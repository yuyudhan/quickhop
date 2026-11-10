//! Pasting text into the focused application (CLP-03) through
//! [`quickhop_core::paste::paste_text`]: the text goes on the clipboard, then the paste
//! shortcut is sent where this session allows it: `⌘V` through `CGEventPost` with the
//! Accessibility permission on macOS, Ctrl+V through `XTest` on X11. Elsewhere (Wayland, until
//! the virtual-keyboard protocol and the `RemoteDesktop` portal; Windows, until its slice) the
//! text stays on the clipboard and the caller says so.
//!
//! On macOS `QUICKHOP_PASTEBOARD` names a private pasteboard to use instead of the general one
//! (the command-line tests), and nothing is ever pasted from it.

use std::io::{self, Write};

use quickhop_core::paste::{
    ClipboardRefused, ClipboardWriter, NotPasted, PasteKeys, Pasted, paste_text,
};

/// Why pasting failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PasteError {
    #[cfg(not(target_os = "macos"))]
    #[error("no graphical session: DISPLAY and WAYLAND_DISPLAY are unset")]
    NoSession,
    #[error(transparent)]
    Refused(#[from] ClipboardRefused),
}

/// No way to send the paste shortcut in this session, and why.
struct NoKeys(&'static str);

impl PasteKeys for NoKeys {
    fn can_send_keys(&self) -> Result<(), NotPasted> {
        Err(NotPasted::Unsupported(self.0.into()))
    }

    fn send_paste(&self) -> Result<(), String> {
        Err(self.0.into())
    }
}

/// The pasteboard the clipboard commands use: `QUICKHOP_PASTEBOARD`'s private one, else the
/// general one.
#[cfg(target_os = "macos")]
pub(crate) fn pasteboard() -> quickhop_platform_macos::pasteboard::Pasteboard {
    use quickhop_platform_macos::pasteboard::Pasteboard;
    std::env::var("QUICKHOP_PASTEBOARD")
        .ok()
        .filter(|name| !name.is_empty())
        .map_or_else(Pasteboard::general, Pasteboard::named)
}

/// Puts `text` on the clipboard the clipboard commands use, without pasting it: the
/// launcher window's copy actions.
///
/// # Errors
/// When the pasteboard refuses the text.
#[cfg(target_os = "macos")]
pub(crate) fn copy(text: &str) -> Result<(), PasteError> {
    Ok(ClipboardWriter::write_text(&pasteboard(), text).map_err(ClipboardRefused)?)
}

/// Where text goes and how the shortcut is sent in this session.
struct Session {
    clipboard: Box<dyn ClipboardWriter>,
    keys: Box<dyn PasteKeys>,
}

/// The pasteboard and `⌘V`; never a keystroke for a private pasteboard.
#[cfg(target_os = "macos")]
fn session() -> Session {
    let keys: Box<dyn PasteKeys> = if std::env::var_os("QUICKHOP_PASTEBOARD").is_some() {
        Box::new(NoKeys(
            "a private pasteboard (QUICKHOP_PASTEBOARD) is never pasted",
        ))
    } else {
        Box::new(quickhop_platform_macos::keys::SystemKeys)
    };
    Session {
        clipboard: Box::new(pasteboard()),
        keys,
    }
}

/// The helper tool for this session's clipboard, and `XTest` on X11.
#[cfg(not(target_os = "macos"))]
fn session() -> Result<Session, PasteError> {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    if cfg!(windows) {
        return Ok(Session {
            clipboard: Box::new(copier::Copier::powershell()),
            keys: Box::new(NoKeys(
                "synthetic paste on Windows comes with its slice: press Ctrl+V",
            )),
        });
    }
    if var("WAYLAND_DISPLAY").is_some() {
        return Ok(Session {
            clipboard: Box::new(copier::Copier::wl_copy()),
            keys: Box::new(NoKeys(
                "synthetic paste on Wayland comes later: press Ctrl+V",
            )),
        });
    }
    if var("DISPLAY").is_none() {
        return Err(PasteError::NoSession);
    }
    #[cfg(unix)]
    let keys: Box<dyn PasteKeys> = Box::new(XTestKeys);
    #[cfg(not(unix))]
    let keys: Box<dyn PasteKeys> = Box::new(NoKeys("no X11 support in this build"));
    Ok(Session {
        clipboard: Box::new(copier::Copier::xclip()),
        keys,
    })
}

/// Ctrl+V through the X server's `XTest` extension.
#[cfg(all(unix, not(target_os = "macos")))]
struct XTestKeys;

#[cfg(all(unix, not(target_os = "macos")))]
impl PasteKeys for XTestKeys {
    fn can_send_keys(&self) -> Result<(), NotPasted> {
        Ok(())
    }

    fn send_paste(&self) -> Result<(), String> {
        quickhop_platform_linux::keyboard::press_ctrl_v(None).map_err(|e| e.to_string())
    }
}

/// Copying through the platform's helper tool, then waiting until the clipboard holds the
/// text, so the paste shortcut never inserts the previous clipboard.
#[cfg(not(target_os = "macos"))]
mod copier {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use quickhop_core::paste::ClipboardWriter;

    use crate::launch::clipboard_text;

    /// How long the clipboard may take to hold the new text.
    const SET_TIMEOUT: Duration = Duration::from_secs(1);

    /// A program that copies its standard input to the clipboard.
    pub(super) struct Copier {
        program: &'static str,
        args: &'static [&'static str],
        /// What it is for in this session, for the message when it cannot be started.
        purpose: &'static str,
    }

    impl Copier {
        pub(super) fn xclip() -> Self {
            Self {
                program: "xclip",
                args: &["-selection", "clipboard", "-in"],
                purpose: "it copies text on X11",
            }
        }

        pub(super) fn wl_copy() -> Self {
            Self {
                program: "wl-copy",
                args: &[],
                purpose: "it copies text in this Wayland session; xclip is not used here, even \
                          with DISPLAY set for XWayland",
            }
        }

        pub(super) fn powershell() -> Self {
            Self {
                program: "powershell.exe",
                args: &[
                    "-NoProfile",
                    "-Command",
                    "Set-Clipboard -Value ([Console]::In.ReadToEnd())",
                ],
                purpose: "it copies text on Windows",
            }
        }
    }

    impl ClipboardWriter for Copier {
        fn write_text(&self, text: &str) -> Result<(), String> {
            let program = self.program;
            let mut child = Command::new(program)
                .args(self.args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| crate::tools::cannot_start(program, self.purpose, &e))?;
            let io_error = |e: std::io::Error| format!("{program}: {e}");
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(text.as_bytes()).map_err(io_error)?;
            }
            // wl-copy and xclip leave a background process serving the text, then exit.
            if !child.wait().map_err(io_error)?.success() {
                return Err(format!("{program} could not set the clipboard"));
            }
            let deadline = Instant::now() + SET_TIMEOUT;
            while clipboard_text().as_deref() != Some(text) {
                if Instant::now() > deadline {
                    return Err("the clipboard did not take the text within a second".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(())
        }
    }
}

/// Pastes `text` into the focused application, or leaves it on the clipboard and says why.
///
/// # Errors
/// When there is no clipboard to use or the clipboard refuses the text.
pub(crate) fn paste(text: &str) -> Result<Pasted, PasteError> {
    #[cfg(target_os = "macos")]
    let session = session();
    #[cfg(not(target_os = "macos"))]
    let session = session()?;
    Ok(paste_text(
        text,
        session.clipboard.as_ref(),
        session.keys.as_ref(),
    )?)
}

/// Pastes `text` and tells the user when it was only copied.
///
/// # Errors
/// As [`paste`].
pub(crate) fn paste_and_report(text: &str) -> Result<(), PasteError> {
    if let Pasted::CopiedOnly(why) = paste(text)? {
        let _ = writeln!(io::stderr(), "quickhop: copied, not pasted: {why}");
    }
    Ok(())
}
