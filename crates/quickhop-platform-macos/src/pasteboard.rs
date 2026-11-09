//! The macOS pasteboard as a [`ClipboardSource`] for the clipboard history (CLP-01), and
//! writing text to it (what pasting an entry back starts with, CLP-03).
//!
//! macOS sends no notification when something is copied; `NSPasteboard.changeCount` grows on
//! every copy, so the daemon polls it (every 250 ms) through [`quickhop_core::Watcher`].

use objc2::rc::{Retained, autoreleasepool};
use objc2_app_kit::{NSPasteboard, NSWorkspace};
use objc2_foundation::NSString;
use quickhop_core::{ClipboardSource, Contents};

/// The type of plain UTF-8 text (`NSPasteboardTypeString`).
const TEXT_TYPE: &str = "public.utf8-plain-text";

/// Types that password managers and other apps add to say "do not record this", from the
/// nspasteboard.org conventions that clipboard managers follow.
const SECRET_TYPES: [&str; 2] = [
    "org.nspasteboard.ConcealedType",
    "org.nspasteboard.TransientType",
];

/// A pasteboard: the general one everyone copies to, or a private named one (tests).
#[derive(Debug, Clone)]
pub struct Pasteboard {
    /// `None` for the general pasteboard.
    name: Option<String>,
}

impl Pasteboard {
    /// The general pasteboard: what ⌘C and ⌘V use.
    pub fn general() -> Self {
        Self { name: None }
    }

    /// A pasteboard of its own, shared only with code that knows `name`; nothing the user
    /// copies lands there.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
        }
    }

    fn get(&self) -> Retained<NSPasteboard> {
        match &self.name {
            None => NSPasteboard::generalPasteboard(),
            Some(name) => NSPasteboard::pasteboardWithName(&NSString::from_str(name)),
        }
    }

    /// Replaces the contents with `text`; with `secret`, also marks it concealed, as password
    /// managers do. Returns whether the pasteboard took it.
    pub fn write_text(&self, text: &str, secret: bool) -> bool {
        autoreleasepool(|_| {
            let pasteboard = self.get();
            pasteboard.clearContents();
            let mut written = pasteboard
                .setString_forType(&NSString::from_str(text), &NSString::from_str(TEXT_TYPE));
            if secret {
                written &= pasteboard.setString_forType(
                    &NSString::from_str(""),
                    &NSString::from_str(SECRET_TYPES[0]),
                );
            }
            written
        })
    }
}

impl ClipboardSource for Pasteboard {
    fn change_count(&self) -> i64 {
        // NSInteger is 64-bit on every Mac quickhop supports.
        autoreleasepool(|_| i64::try_from(self.get().changeCount()).unwrap_or(i64::MAX))
    }

    /// The text on the pasteboard, with the frontmost app as its source: the app the user
    /// copied from, as long as the watcher polls soon after the copy.
    fn contents(&self) -> Contents {
        autoreleasepool(|_| {
            let pasteboard = self.get();
            let types: Vec<String> = pasteboard
                .types()
                .map(|types| types.to_vec().iter().map(ToString::to_string).collect())
                .unwrap_or_default();
            if types.iter().any(|t| SECRET_TYPES.contains(&t.as_str())) {
                return Contents::Secret;
            }
            let Some(text) = pasteboard.stringForType(&NSString::from_str(TEXT_TYPE)) else {
                return Contents::Other;
            };
            let source_app = NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .and_then(|app| app.bundleIdentifier())
                .map(|id| id.to_string());
            Contents::Text {
                text: text.to_string(),
                source_app,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use quickhop_core::Watcher;
    use quickhop_store::state::State;

    use super::*;

    /// A private pasteboard per test; the fixed name keeps reruns from piling up pasteboards.
    fn private(test: &str) -> Pasteboard {
        Pasteboard::named(format!("dev.quickhop.test.{test}"))
    }

    #[test]
    fn three_copies_are_listed_newest_first() -> Result<(), Box<dyn std::error::Error>> {
        let board = private("three");
        let mut watcher = Watcher::new();
        let mut state = State::in_memory()?;
        watcher.poll(&board);
        for (now, text) in [
            (1, "searching rust"),
            (2, "on python"),
            (3, "with quickhop"),
        ] {
            assert!(board.write_text(text, false));
            let copied = watcher.poll(&board).ok_or("the copy was noticed")?;
            assert_eq!(copied.text, text);
            state.record_clip(&copied.text, copied.source_app.as_deref(), now)?;
        }
        assert_eq!(watcher.poll(&board), None, "no copy since the last poll");
        let texts: Vec<String> = state.clips(10)?.into_iter().map(|clip| clip.text).collect();
        assert_eq!(texts, ["with quickhop", "on python", "searching rust"]);
        Ok(())
    }

    #[test]
    fn concealed_copies_are_secret() {
        let board = private("secret");
        assert!(board.write_text("hunter2", true));
        assert_eq!(board.contents(), Contents::Secret);
        let mut watcher = Watcher::new();
        watcher.poll(&board);
        assert!(board.write_text("hunter3", true));
        assert_eq!(watcher.poll(&board), None);
    }

    #[test]
    fn writing_bumps_the_change_count() {
        let board = private("count");
        let before = board.change_count();
        board.write_text("x", false);
        assert!(board.change_count() > before);
        assert!(matches!(board.contents(), Contents::Text { text, .. } if text == "x"));
    }
}
