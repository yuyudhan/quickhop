//! Pasting a chosen entry into the app the user came from (CLP-03).
//!
//! The text goes on the clipboard first, so it is never lost; then, when the platform lets
//! quickhop send keystrokes, the paste shortcut is sent to the frontmost app. Without that
//! permission (macOS Accessibility) or a way to type (a Wayland compositor without a virtual
//! keyboard) the result says the text was only copied and why, for the toast. The caller hides
//! the launcher before calling [`paste_text`], so the shortcut reaches the app the user came
//! from.

/// Puts text on the system clipboard.
pub trait ClipboardWriter {
    /// Replaces the clipboard with `text`.
    ///
    /// # Errors
    /// Why the clipboard did not take it, shown to the user.
    fn write_text(&self, text: &str) -> Result<(), String>;
}

/// Sends the paste shortcut (`⌘V`, `Ctrl+V`) to the frontmost app.
pub trait PasteKeys {
    /// Whether keystrokes may be sent here at all.
    ///
    /// # Errors
    /// Why not: [`NotPasted::NoPermission`] or [`NotPasted::Unsupported`].
    fn can_send_keys(&self) -> Result<(), NotPasted>;

    /// Sends the paste shortcut.
    ///
    /// # Errors
    /// A description of what failed, shown to the user.
    fn send_paste(&self) -> Result<(), String>;
}

/// What happened to the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pasted {
    /// It was pasted into the frontmost app.
    Pasted,
    /// It is on the clipboard but was not pasted.
    CopiedOnly(NotPasted),
}

/// Why text was copied but not pasted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotPasted {
    /// quickhop may not send keystrokes; the user has to grant the permission.
    #[error(
        "quickhop may not send keystrokes: allow it under Privacy & Security › Accessibility in System Settings"
    )]
    NoPermission,
    /// This session offers no way to send keystrokes (yet).
    #[error("{0}")]
    Unsupported(String),
    /// Sending the shortcut failed.
    #[error("sending the paste shortcut failed: {0}")]
    KeysFailed(String),
}

/// Why nothing happened at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the clipboard did not take the text: {0}")]
pub struct ClipboardRefused(pub String);

/// Copies `text`, then pastes it into the frontmost app when keystrokes are allowed.
///
/// # Errors
/// When the clipboard refuses the text; nothing is pasted then.
pub fn paste_text(
    text: &str,
    clipboard: &dyn ClipboardWriter,
    keys: &dyn PasteKeys,
) -> Result<Pasted, ClipboardRefused> {
    clipboard.write_text(text).map_err(ClipboardRefused)?;
    if let Err(why) = keys.can_send_keys() {
        return Ok(Pasted::CopiedOnly(why));
    }
    Ok(match keys.send_paste() {
        Ok(()) => Pasted::Pasted,
        Err(error) => Pasted::CopiedOnly(NotPasted::KeysFailed(error)),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// Records what reached the clipboard and the keyboard, in order.
    #[derive(Default)]
    struct Fake {
        refuses: Option<&'static str>,
        blocked: Option<NotPasted>,
        keys_fail: bool,
        log: RefCell<Vec<String>>,
    }

    impl ClipboardWriter for Fake {
        fn write_text(&self, text: &str) -> Result<(), String> {
            self.log.borrow_mut().push(format!("copy {text}"));
            self.refuses.map_or(Ok(()), |why| Err(why.into()))
        }
    }

    impl PasteKeys for Fake {
        fn can_send_keys(&self) -> Result<(), NotPasted> {
            self.blocked.clone().map_or(Ok(()), Err)
        }

        fn send_paste(&self) -> Result<(), String> {
            self.log.borrow_mut().push("paste".into());
            if self.keys_fail {
                Err("event source unavailable".into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn copies_then_pastes() {
        let fake = Fake::default();
        assert_eq!(paste_text("hello", &fake, &fake), Ok(Pasted::Pasted));
        assert_eq!(*fake.log.borrow(), ["copy hello", "paste"]);
    }

    #[test]
    fn without_permission_it_only_copies() {
        let fake = Fake {
            blocked: Some(NotPasted::NoPermission),
            ..Fake::default()
        };
        assert_eq!(
            paste_text("hello", &fake, &fake),
            Ok(Pasted::CopiedOnly(NotPasted::NoPermission))
        );
        assert_eq!(*fake.log.borrow(), ["copy hello"], "no keystroke is sent");
    }

    #[test]
    fn an_unsupported_session_only_copies_and_says_why() {
        let why = NotPasted::Unsupported("synthetic paste on Wayland comes later".into());
        let fake = Fake {
            blocked: Some(why.clone()),
            ..Fake::default()
        };
        assert_eq!(
            paste_text("hello", &fake, &fake),
            Ok(Pasted::CopiedOnly(why))
        );
        assert_eq!(*fake.log.borrow(), ["copy hello"]);
    }

    #[test]
    fn a_failed_shortcut_leaves_the_text_copied() {
        let fake = Fake {
            keys_fail: true,
            ..Fake::default()
        };
        assert_eq!(
            paste_text("hello", &fake, &fake),
            Ok(Pasted::CopiedOnly(NotPasted::KeysFailed(
                "event source unavailable".into()
            )))
        );
    }

    #[test]
    fn a_refusing_clipboard_pastes_nothing() {
        let fake = Fake {
            refuses: Some("xclip is not installed"),
            ..Fake::default()
        };
        assert_eq!(
            paste_text("hello", &fake, &fake),
            Err(ClipboardRefused("xclip is not installed".into()))
        );
        assert_eq!(
            *fake.log.borrow(),
            ["copy hello"],
            "pasting would insert the old clipboard"
        );
    }
}
