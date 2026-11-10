//! The onboarding window's model (PLT-02, plan slice C2): whether quickhop may paste into the
//! app in front, which on macOS needs the Accessibility permission, and the one button that
//! leads there. Everything else in quickhop works without the permission, so the window
//! explains rather than blocks.
//!
//! The model holds no toolkit or system state: the window asks the system whether the
//! permission is granted and passes the answer to [`Onboarding::recheck`], whenever quickhop
//! becomes the active app again (the user comes back from System Settings), and every moment
//! while [`Onboarding::watching`] (the user is in System Settings).

use std::path::Path;

/// Where System Settings lists the programs allowed to control the computer: the pane's
/// title before macOS 27 and since.
const PANE: &str = "System Settings › Privacy & Security › Accessibility (Device Control and \
                    Data Access on macOS 27 and later)";

/// The program macOS checks the permission of: the one to switch on in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    /// This build's app ("quickhop.app", or "quickhop Dev.app" for a development build), listed
    /// by its name.
    App,
    /// A `quickhop` binary outside the app, at this path: macOS lists it by that file, or,
    /// started from a terminal, checks the terminal app instead.
    Binary(String),
}

impl Program {
    /// The program running from the executable `exe` (symlinks resolved): the app when it is
    /// the executable of a bundle (`<name>.app/Contents/MacOS/<file>`).
    pub fn of(exe: &Path) -> Self {
        let macos = exe.parent().filter(|macos| macos.ends_with("MacOS"));
        let bundle = macos
            .and_then(Path::parent)
            .filter(|contents| contents.ends_with("Contents"))
            .and_then(Path::parent);
        if bundle
            .and_then(Path::extension)
            .is_some_and(|ext| ext == "app")
        {
            Self::App
        } else {
            Self::Binary(exe.display().to_string())
        }
    }
}

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
    /// The program to switch on.
    program: Program,
}

impl Onboarding {
    /// The window for a permission that is `granted` or not to `program`, naming `hotkey`
    /// once it is.
    pub fn new(granted: bool, hotkey: Option<String>, program: Program) -> Self {
        Self {
            granted,
            asked: false,
            failure: None,
            hotkey,
            program,
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

    /// Whether the window keeps asking the system: after the button led to System Settings,
    /// until the permission is granted, so turning quickhop on there shows at once.
    pub fn watching(&self) -> bool {
        self.asked && self.failure.is_none() && !self.granted
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
        let who = match &self.program {
            Program::App => quickhop_core::Flavour::CURRENT.app_name().to_owned(),
            Program::Binary(path) => format!(
                "{path} (the terminal app instead, when you started quickhop from a terminal)"
            ),
        };
        if let Some(failure) = &self.failure {
            return format!(
                "System Settings did not open ({failure}). Open it yourself and turn on {who} \
                 under {PANE}."
            );
        }
        if self.asked {
            let stale = match &self.program {
                Program::App => {
                    "If quickhop was on already, that entry is an older build: switch it off \
                     and on, or remove it with −, then press the button again."
                }
                Program::Binary(_) => {
                    "Not in the list? Add it with +. On already? That entry is an older \
                     build: remove it with −, then add this one with +."
                }
            };
            return format!(
                "Turn on {who} in the list System Settings shows, then come back: quickhop \
                 checks again when it is in front. {stale}"
            );
        }
        format!(
            "quickhop pastes clipboard history entries and snippets into the app you were using \
             by pressing ⌘V for you. macOS allows that once you turn on {who} under {PANE}. \
             Everything else works without it."
        )
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

    fn app(granted: bool, hotkey: Option<&str>) -> Onboarding {
        Onboarding::new(granted, hotkey.map(str::to_owned), Program::App)
    }

    const BINARY: &str = "/Users/me/.cargo/bin/quickhop";

    #[test]
    fn without_the_permission_the_button_asks_and_the_text_says_where_to_grant_it() {
        let onboarding = app(false, Some("Alt+Space"));
        assert_eq!(onboarding.status_line(), "Accessibility: not allowed");
        assert_eq!(onboarding.press(), Press::Ask);
        assert_eq!(onboarding.button(), "Open Accessibility Settings");
        let message = onboarding.message();
        assert!(
            message.contains("Privacy & Security › Accessibility"),
            "{message}"
        );
        assert!(message.contains("turn on quickhop"), "{message}");
        assert!(!onboarding.watching());
        assert_eq!(
            onboarding.status(),
            Status {
                granted: false,
                asked: false
            }
        );
    }

    #[test]
    fn every_text_names_the_macos_27_title_of_the_pane_too() {
        let mut onboarding = app(false, None);
        assert!(
            onboarding
                .message()
                .contains("Device Control and Data Access")
        );
        onboarding.asked(Err("open exited with 1".into()));
        assert!(
            onboarding
                .message()
                .contains("Device Control and Data Access")
        );
    }

    #[test]
    fn after_asking_the_text_says_to_come_back_and_the_button_asks_again() {
        let mut onboarding = app(false, None);
        let before = onboarding.message();
        onboarding.asked(Ok(()));
        assert_ne!(onboarding.message(), before);
        assert!(onboarding.message().contains("come back"));
        assert_eq!(onboarding.press(), Press::Ask);
        assert!(onboarding.status().asked);
    }

    #[test]
    fn after_asking_the_text_says_what_to_do_with_an_entry_that_is_on_already() {
        let mut onboarding = app(false, None);
        onboarding.asked(Ok(()));
        let message = onboarding.message();
        assert!(message.contains("older build"), "{message}");
        assert!(message.contains("switch it off and on"), "{message}");
        assert!(message.contains("remove it with −"), "{message}");
    }

    #[test]
    fn a_binary_outside_the_app_is_named_by_its_path_and_removed_with_minus() {
        let mut onboarding = Onboarding::new(false, None, Program::Binary(BINARY.into()));
        assert!(onboarding.message().contains(&format!("turn on {BINARY}")));
        assert!(onboarding.message().contains("terminal"));
        onboarding.asked(Ok(()));
        let message = onboarding.message();
        assert!(message.contains(BINARY), "{message}");
        assert!(message.contains("remove it with −"), "{message}");
        assert!(message.contains("Add it with +"), "{message}");
    }

    #[test]
    fn the_window_watches_from_the_ask_until_the_permission_is_granted() {
        let mut onboarding = app(false, None);
        onboarding.asked(Ok(()));
        assert!(onboarding.watching());
        assert!(onboarding.recheck(true));
        assert!(!onboarding.watching());
    }

    #[test]
    fn a_failed_ask_is_not_watched() {
        let mut onboarding = app(false, None);
        onboarding.asked(Err("open exited with 1".into()));
        assert!(!onboarding.watching());
    }

    #[test]
    fn a_failed_ask_says_why_until_the_next_one_works() {
        let mut onboarding = app(false, None);
        onboarding.asked(Err("open exited with 1".into()));
        assert!(onboarding.message().contains("open exited with 1"));
        onboarding.asked(Ok(()));
        assert!(!onboarding.message().contains("open exited with 1"));
    }

    #[test]
    fn a_recheck_that_finds_the_permission_offers_done_and_names_the_hotkey() {
        let mut onboarding = app(false, Some("Alt+Space"));
        onboarding.asked(Ok(()));
        assert!(onboarding.recheck(true));
        assert_eq!(onboarding.status_line(), "Accessibility: allowed");
        assert_eq!(onboarding.press(), Press::Close);
        assert_eq!(onboarding.button(), "Done");
        assert!(onboarding.message().contains("Press Alt+Space"));
    }

    #[test]
    fn an_unchanged_answer_changes_nothing() {
        let mut onboarding = app(false, None);
        let before = onboarding.clone();
        assert!(!onboarding.recheck(false));
        assert_eq!(onboarding, before);
    }

    #[test]
    fn a_withdrawn_permission_asks_again() {
        let mut onboarding = app(true, None);
        assert_eq!(onboarding.press(), Press::Close);
        assert!(onboarding.recheck(false));
        assert_eq!(onboarding.press(), Press::Ask);
        assert_eq!(onboarding.status_line(), "Accessibility: not allowed");
    }

    #[test]
    fn without_a_hotkey_the_ready_text_says_how_to_open_quickhop() {
        let mut onboarding = app(true, Some("Alt+Space"));
        onboarding.set_hotkey(None);
        assert!(!onboarding.message().contains("Alt+Space"));
        assert!(onboarding.message().contains("quickhop toggle"));
    }

    #[test]
    fn the_executable_of_a_bundle_is_the_app_and_any_other_a_binary() {
        assert_eq!(
            Program::of(Path::new(
                "/Applications/quickhop.app/Contents/MacOS/quickhop"
            )),
            Program::App
        );
        for exe in [
            BINARY,
            "/Applications/quickhop.app/quickhop",
            "/Applications/quickhop/Contents/MacOS/quickhop",
            "/Applications/quickhop.app/Contents/Resources/quickhop",
            "quickhop",
        ] {
            assert_eq!(
                Program::of(Path::new(exe)),
                Program::Binary(exe.into()),
                "{exe}"
            );
        }
    }
}
