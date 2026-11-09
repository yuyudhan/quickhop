//! The onboarding window's model (PLT-02, plan slice C2): whether quickhop may paste into the
//! app in front, which on macOS needs the Accessibility permission, and the one button that
//! leads there. Everything else in quickhop works without the permission, so the window
//! explains rather than blocks.
//!
//! The model holds no toolkit or system state: the window asks the system whether the
//! permission is granted and passes the answer to [`Onboarding::recheck`], whenever quickhop
//! becomes the active app again (the user comes back from System Settings).

/// What the onboarding window says, as the UI tests' snapshot reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Status {
    /// Whether quickhop may paste into other apps.
    pub granted: bool,
    /// Whether the button already led the user to the settings.
    pub asked: bool,
}

/// What pressing the window's button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// Ask for the permission: the system's prompt, then the settings pane that grants it.
    Ask,
    /// Close the window: nothing is missing.
    Close,
}

/// The onboarding window's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Onboarding {
    granted: bool,
    asked: bool,
    /// Why asking failed (System Settings did not open), until the next ask.
    failure: Option<String>,
    /// The hotkey that shows the launcher, when one is bound.
    hotkey: Option<String>,
}

impl Onboarding {
    /// The window for a permission that is `granted` or not, naming `hotkey` once it is.
    pub fn new(granted: bool, hotkey: Option<String>) -> Self {
        Self {
            granted,
            asked: false,
            failure: None,
            hotkey,
        }
    }

    /// Whether quickhop may paste into other apps.
    pub fn granted(&self) -> bool {
        self.granted
    }

    /// Takes the system's answer `granted` again. Returns whether it changed what the window
    /// says.
    pub fn recheck(&mut self, granted: bool) -> bool {
        let changed = granted != self.granted;
        self.granted = granted;
        changed
    }

    /// Names `hotkey` (`None`: none is bound) from now on.
    pub fn set_hotkey(&mut self, hotkey: Option<String>) {
        self.hotkey = hotkey;
    }

    /// What pressing the button does now.
    pub fn press(&self) -> Press {
        if self.granted {
            Press::Close
        } else {
            Press::Ask
        }
    }

    /// Records the outcome of a [`Press::Ask`]: `Err` says why the settings did not open.
    pub fn asked(&mut self, outcome: Result<(), String>) {
        self.asked = true;
        self.failure = outcome.err();
    }

    /// The window's heading.
    pub fn title(&self) -> &'static str {
        if self.granted {
            "quickhop is ready"
        } else {
            "Allow quickhop to paste"
        }
    }

    /// The status line.
    pub fn status_line(&self) -> &'static str {
        if self.granted {
            "Accessibility: allowed"
        } else {
            "Accessibility: not allowed"
        }
    }

    /// The text under the status line: why the permission matters, where to grant it, or
    /// how to open quickhop once it is granted.
    pub fn message(&self) -> String {
        if self.granted {
            return match &self.hotkey {
                Some(hotkey) => format!(
                    "quickhop may paste into the app in front. Press {hotkey} to open quickhop."
                ),
                None => "quickhop may paste into the app in front. No hotkey is bound: run \
                         `quickhop toggle` to open quickhop, or set `hotkey` in config.toml."
                    .to_owned(),
            };
        }
        if let Some(failure) = &self.failure {
            return format!(
                "System Settings did not open ({failure}). Open it yourself and turn on \
                 quickhop under Privacy & Security › Accessibility."
            );
        }
        if self.asked {
            return "Turn on quickhop in the Accessibility list of System Settings, then come \
                    back: quickhop checks again when it is in front."
                .to_owned();
        }
        "quickhop pastes clipboard history entries and snippets into the app you were using by \
         pressing ⌘V for you. macOS allows that once you turn quickhop on under System \
         Settings › Privacy & Security › Accessibility. Everything else works without it."
            .to_owned()
    }

    /// The button's label.
    pub fn button(&self) -> &'static str {
        match self.press() {
            Press::Ask => "Open Accessibility Settings",
            Press::Close => "Done",
        }
    }

    /// What the window says, for the UI tests.
    pub fn status(&self) -> Status {
        Status {
            granted: self.granted,
            asked: self.asked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_the_permission_the_button_asks_and_the_text_says_where_to_grant_it() {
        let onboarding = Onboarding::new(false, Some("Alt+Space".into()));
        assert_eq!(onboarding.status_line(), "Accessibility: not allowed");
        assert_eq!(onboarding.press(), Press::Ask);
        assert_eq!(onboarding.button(), "Open Accessibility Settings");
        assert!(
            onboarding
                .message()
                .contains("Privacy & Security › Accessibility")
        );
        assert_eq!(
            onboarding.status(),
            Status {
                granted: false,
                asked: false
            }
        );
    }

    #[test]
    fn after_asking_the_text_says_to_come_back_and_the_button_asks_again() {
        let mut onboarding = Onboarding::new(false, None);
        let before = onboarding.message();
        onboarding.asked(Ok(()));
        assert_ne!(onboarding.message(), before);
        assert!(onboarding.message().contains("come back"));
        assert_eq!(onboarding.press(), Press::Ask);
        assert!(onboarding.status().asked);
    }

    #[test]
    fn a_failed_ask_says_why_until_the_next_one_works() {
        let mut onboarding = Onboarding::new(false, None);
        onboarding.asked(Err("open exited with 1".into()));
        assert!(onboarding.message().contains("open exited with 1"));
        onboarding.asked(Ok(()));
        assert!(!onboarding.message().contains("open exited with 1"));
    }

    #[test]
    fn a_recheck_that_finds_the_permission_offers_done_and_names_the_hotkey() {
        let mut onboarding = Onboarding::new(false, Some("Alt+Space".into()));
        onboarding.asked(Ok(()));
        assert!(onboarding.recheck(true));
        assert_eq!(onboarding.status_line(), "Accessibility: allowed");
        assert_eq!(onboarding.press(), Press::Close);
        assert_eq!(onboarding.button(), "Done");
        assert!(onboarding.message().contains("Press Alt+Space"));
    }

    #[test]
    fn an_unchanged_answer_changes_nothing() {
        let mut onboarding = Onboarding::new(false, None);
        let before = onboarding.clone();
        assert!(!onboarding.recheck(false));
        assert_eq!(onboarding, before);
    }

    #[test]
    fn a_withdrawn_permission_asks_again() {
        let mut onboarding = Onboarding::new(true, None);
        assert_eq!(onboarding.press(), Press::Close);
        assert!(onboarding.recheck(false));
        assert_eq!(onboarding.press(), Press::Ask);
        assert_eq!(onboarding.status_line(), "Accessibility: not allowed");
    }

    #[test]
    fn without_a_hotkey_the_ready_text_says_how_to_open_quickhop() {
        let mut onboarding = Onboarding::new(true, Some("Alt+Space".into()));
        onboarding.set_hotkey(None);
        assert!(!onboarding.message().contains("Alt+Space"));
        assert!(onboarding.message().contains("quickhop toggle"));
    }
}
