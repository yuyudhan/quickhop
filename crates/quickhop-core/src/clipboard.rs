//! Watching the system clipboard for copied text (CLP-01).
//!
//! The platform crates implement [`ClipboardSource`] (macOS: `NSPasteboard`'s change count;
//! Linux: `XFixes` or data-control events counted the same way); the daemon calls
//! [`Watcher::poll`] on a timer and stores what it returns. Everything here is plain logic, so
//! it is tested with a fake source on every OS.

/// What the clipboard holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Contents {
    /// Plain text, and the application it was copied from when the system says.
    Text {
        /// The copied text.
        text: String,
        /// Identifier of the app that was frontmost when it was copied (a bundle id on
        /// macOS, an X11 class or Wayland app id on Linux).
        source_app: Option<String>,
    },
    /// Something its owner marked as secret or transient (a password manager's copy): never
    /// recorded.
    Secret,
    /// Nothing, or nothing textual (an image, files).
    Other,
}

impl Contents {
    /// The copy worth remembering in these contents: text that is not only whitespace.
    /// Secrets and non-text contents give `None`. Every watcher, polled or pushed, goes
    /// through this, so a password manager's copy is never recorded on any platform.
    pub fn into_copied(self) -> Option<Copied> {
        match self {
            Self::Text { text, source_app } if !text.trim().is_empty() => {
                Some(Copied { text, source_app })
            }
            Self::Text { .. } | Self::Secret | Self::Other => None,
        }
    }
}

/// The system clipboard, as the watcher sees it.
pub trait ClipboardSource {
    /// A number the system changes whenever anything is copied.
    fn change_count(&self) -> i64;

    /// The current contents.
    fn contents(&self) -> Contents;
}

/// Text that was copied and should be remembered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    /// The copied text, exactly as copied.
    pub text: String,
    /// See [`Contents::Text::source_app`].
    pub source_app: Option<String>,
}

/// Notices clipboard changes between polls.
#[derive(Debug, Default)]
pub struct Watcher {
    last_count: Option<i64>,
}

impl Watcher {
    /// A watcher that has not looked yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Looks at the clipboard once. Returns its text when it changed since the previous poll
    /// and holds text worth remembering: not secret, not only whitespace. The first poll only
    /// notes where the clipboard stands, so what was copied before the watcher started is not
    /// recorded as a new copy.
    pub fn poll(&mut self, source: &dyn ClipboardSource) -> Option<Copied> {
        let count = source.change_count();
        let previous = self.last_count.replace(count);
        if previous.is_none_or(|previous| previous == count) {
            return None;
        }
        source.contents().into_copied()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// A clipboard whose copies are scripted by the test.
    #[derive(Default)]
    struct Fake {
        count: RefCell<i64>,
        contents: RefCell<Option<Contents>>,
    }

    impl Fake {
        fn copy(&self, contents: Contents) {
            *self.count.borrow_mut() += 1;
            *self.contents.borrow_mut() = Some(contents);
        }
    }

    impl ClipboardSource for Fake {
        fn change_count(&self) -> i64 {
            *self.count.borrow()
        }

        fn contents(&self) -> Contents {
            self.contents.borrow().clone().unwrap_or(Contents::Other)
        }
    }

    fn text(text: &str) -> Contents {
        Contents::Text {
            text: text.into(),
            source_app: Some("com.apple.TextEdit".into()),
        }
    }

    #[test]
    fn the_first_poll_records_nothing() {
        let fake = Fake::default();
        fake.copy(text("copied before quickhop started"));
        let mut watcher = Watcher::new();
        assert_eq!(watcher.poll(&fake), None);
        assert_eq!(watcher.poll(&fake), None, "nothing changed since");
    }

    #[test]
    fn each_copy_is_reported_once() {
        let fake = Fake::default();
        let mut watcher = Watcher::new();
        watcher.poll(&fake);
        fake.copy(text("one"));
        assert_eq!(
            watcher.poll(&fake),
            Some(Copied {
                text: "one".into(),
                source_app: Some("com.apple.TextEdit".into()),
            })
        );
        assert_eq!(watcher.poll(&fake), None);
        fake.copy(text("one"));
        assert_eq!(
            watcher.poll(&fake).map(|c| c.text),
            Some("one".into()),
            "copying the same text again is a new copy; the store dedupes it"
        );
    }

    #[test]
    fn copies_between_polls_report_the_latest() {
        let fake = Fake::default();
        let mut watcher = Watcher::new();
        watcher.poll(&fake);
        fake.copy(text("first"));
        fake.copy(text("second"));
        assert_eq!(watcher.poll(&fake).map(|c| c.text), Some("second".into()));
    }

    #[test]
    fn secrets_blanks_and_non_text_are_skipped() {
        let fake = Fake::default();
        let mut watcher = Watcher::new();
        watcher.poll(&fake);
        for contents in [Contents::Secret, Contents::Other, text(" \n\t")] {
            fake.copy(contents.clone());
            assert_eq!(
                watcher.poll(&fake),
                None,
                "{contents:?} must not be recorded"
            );
        }
        fake.copy(text("  padded  "));
        assert_eq!(
            watcher.poll(&fake).map(|c| c.text),
            Some("  padded  ".into()),
            "text is kept exactly as copied"
        );
    }
}
