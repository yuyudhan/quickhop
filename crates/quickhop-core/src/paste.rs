//! Pasting a chosen entry into the app the user came from (CLP-03).
//!
//! The text goes on the clipboard first, so it is never lost; then, when the platform lets
//! quickhop send keystrokes, the paste shortcut is sent to the frontmost app. Without that
//! permission (macOS Accessibility, or no virtual keyboard on a Wayland compositor) the result
//! says the text was only copied and why, for the toast. The caller hides the launcher before
//! calling [`paste_text`], so the shortcut reaches the app the user came from.

/// Puts text on the system clipboard.
pub trait ClipboardWriter {
    /// Replaces the clipboard with `text`; returns whether the system accepted it.
    fn write_text(&self, text: &str) -> bool;
}

/// Sends the paste shortcut (`⌘V`, `Ctrl+V`) to the frontmost app.
pub trait PasteKeys {
    /// Whether keystrokes may be sent at all (on macOS: the Accessibility permission).
    fn can_send_keys(&self) -> bool;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotPasted {
    /// quickhop may not send keystrokes; the user has to grant the permission.
    NoPermission,
    /// Sending the shortcut failed.
    KeysFailed(String),
}

/// Why nothing happened at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the clipboard did not accept the text")]
pub struct ClipboardRefused;

/// Copies `text`, then pastes it into the frontmost app when keystrokes are allowed.
///
/// # Errors
/// When the clipboard refuses the text; nothing is pasted then.
pub fn paste_text(
    text: &str,
    clipboard: &dyn ClipboardWriter,
    keys: &dyn PasteKeys,
) -> Result<Pasted, ClipboardRefused> {
    if !clipboard.write_text(text) {
        return Err(ClipboardRefused);
    }
    if !keys.can_send_keys() {
        return Ok(Pasted::CopiedOnly(NotPasted::NoPermission));
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
        accepts: bool,
        allowed: bool,
        keys_fail: bool,
        log: RefCell<Vec<String>>,
    }

    impl ClipboardWriter for Fake {
        fn write_text(&self, text: &str) -> bool {
            self.log.borrow_mut().push(format!("copy {text}"));
            self.accepts
        }
    }

    impl PasteKeys for Fake {
        fn can_send_keys(&self) -> bool {
            self.allowed
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

    fn fake(accepts: bool, allowed: bool, keys_fail: bool) -> Fake {
        Fake {
            accepts,
            allowed,
            keys_fail,
            log: RefCell::default(),
        }
    }

    #[test]
    fn copies_then_pastes() {
        let fake = fake(true, true, false);
        assert_eq!(paste_text("hello", &fake, &fake), Ok(Pasted::Pasted));
        assert_eq!(*fake.log.borrow(), ["copy hello", "paste"]);
    }

    #[test]
    fn without_permission_it_only_copies() {
        let fake = fake(true, false, false);
        assert_eq!(
            paste_text("hello", &fake, &fake),
            Ok(Pasted::CopiedOnly(NotPasted::NoPermission))
        );
        assert_eq!(*fake.log.borrow(), ["copy hello"], "no keystroke is sent");
    }

    #[test]
    fn a_failed_shortcut_leaves_the_text_copied() {
        let fake = fake(true, true, true);
        assert_eq!(
            paste_text("hello", &fake, &fake),
            Ok(Pasted::CopiedOnly(NotPasted::KeysFailed(
                "event source unavailable".into()
            )))
        );
    }

    #[test]
    fn a_refusing_clipboard_pastes_nothing() {
        let fake = fake(false, true, false);
        assert_eq!(paste_text("hello", &fake, &fake), Err(ClipboardRefused));
        assert_eq!(
            *fake.log.borrow(),
            ["copy hello"],
            "pasting would insert the old clipboard"
        );
    }
}
