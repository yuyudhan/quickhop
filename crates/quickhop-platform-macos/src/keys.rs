//! Sending `⌘V` to the frontmost app (CLP-03) and the Accessibility permission it needs
//! (PLT-02).
//!
//! macOS delivers synthesized key events to other apps only from processes the user trusted
//! under System Settings › Privacy & Security › Accessibility (Device Control and Data Access
//! on macOS 27 and later). [`is_trusted`] checks that without asking; [`ask_for_trust`] clears
//! a stale entry, shows the system prompt and opens that list.

use objc2_application_services::{
    AXIsProcessTrusted, AXIsProcessTrustedWithOptions, kAXTrustedCheckOptionPrompt,
};
use objc2_core_foundation::{CFBoolean, CFDictionary, CFString};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};
use quickhop_core::paste::{ClipboardWriter, NotPasted, PasteKeys};

use crate::pasteboard::Pasteboard;

/// The virtual key code of the `V` key on an ANSI keyboard (`kVK_ANSI_V`).
const KEY_V: u16 = 9;

/// The Accessibility list of System Settings, in the order to try them: the address of the
/// Privacy & Security extension since macOS 13, then the legacy preference pane's, which
/// System Settings still accepts as an alias of the same extension.
pub const ACCESSIBILITY_SETTINGS_URLS: [&str; 2] = [
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility",
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
];

/// Whether this process may send key events to other apps. Never shows a prompt.
pub fn is_trusted() -> bool {
    // SAFETY: `AXIsProcessTrusted` takes no arguments and only reads the TCC database.
    unsafe { AXIsProcessTrusted() }
}

/// Like [`is_trusted`], but when the process is not trusted macOS also shows its prompt that
/// opens the Accessibility settings and adds the running program to the list, switched off
/// (once per launch of the process).
fn request_trust() -> bool {
    // SAFETY: `kAXTrustedCheckOptionPrompt` is an immutable `CFString` constant exported by
    // the ApplicationServices framework, valid for the life of the process.
    let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
    let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[CFBoolean::new(true)]);
    // SAFETY: `options` is a valid dictionary of the documented key and a `CFBoolean` value,
    // alive for the whole call.
    unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) }
}

/// What asking macOS for Accessibility takes, behind a trait so the order of the steps is
/// tested without touching the real permission.
pub trait TrustSystem {
    /// Whether this process is trusted for Accessibility now.
    fn trusted(&mut self) -> bool;
    /// Deletes the Accessibility entry of `bundle_id` from the list (`tccutil reset`).
    ///
    /// # Errors
    /// What went wrong, for the log.
    fn reset(&mut self, bundle_id: &str) -> Result<(), String>;
    /// Shows macOS's prompt, which adds the running program to the list, switched off.
    fn prompt(&mut self);
    /// Opens the System Settings address `url`.
    ///
    /// # Errors
    /// Worded for the user.
    fn open(&mut self, url: &str) -> Result<(), String>;
}

/// What [`ask_for_trust`] did besides opening the settings: why the stale entry stayed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Asked {
    /// Why the old entry of the app could not be cleared, when it could not.
    pub reset_failed: Option<String>,
}

/// Asks for Accessibility and opens its list in System Settings.
///
/// When this process is quickhop.app (`app_id`, its bundle identifier) and is not trusted, the
/// app's old entry is deleted first. macOS keys the grant to the code signature that was
/// allowed, and an ad-hoc signed quickhop has a new one with every build: after a rebuild or an
/// update the list still shows quickhop switched on while macOS refuses the new copy, and
/// switching it off and on keeps the old signature. Without the old entry, the prompt adds the
/// running copy, so the switch the user turns on is the one macOS checks. `None` (a terminal or
/// a bare binary runs quickhop) resets nothing: that entry belongs to another program.
///
/// # Errors
/// When no address of the settings opens; the text names each failure.
pub fn ask_for_trust(system: &mut dyn TrustSystem, app_id: Option<&str>) -> Result<Asked, String> {
    let mut asked = Asked::default();
    if !system.trusted() {
        if let Some(id) = app_id {
            asked.reset_failed = system.reset(id).err();
        }
        system.prompt();
    }
    let mut failures = Vec::new();
    for url in ACCESSIBILITY_SETTINGS_URLS {
        match system.open(url) {
            Ok(()) => return Ok(asked),
            Err(error) => failures.push(error),
        }
    }
    Err(failures.join("; "))
}

/// The real permission, `tccutil` and System Settings.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacTrust;

impl TrustSystem for MacTrust {
    fn trusted(&mut self) -> bool {
        is_trusted()
    }

    fn reset(&mut self, bundle_id: &str) -> Result<(), String> {
        let output = std::process::Command::new("/usr/bin/tccutil")
            .args(["reset", "Accessibility", bundle_id])
            .output()
            .map_err(|error| format!("cannot run tccutil: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "tccutil failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    fn prompt(&mut self) {
        request_trust();
    }

    fn open(&mut self, url: &str) -> Result<(), String> {
        let status = std::process::Command::new("/usr/bin/open")
            .arg(url)
            .status()
            .map_err(|error| format!("cannot run open: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("open {url} failed ({status})"))
        }
    }
}

/// Sends `⌘V` through the HID event tap, so it reaches whichever app is frontmost.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemKeys;

impl PasteKeys for SystemKeys {
    fn can_send_keys(&self) -> Result<(), NotPasted> {
        if is_trusted() {
            Ok(())
        } else {
            Err(NotPasted::NoPermission)
        }
    }

    fn send_paste(&self) -> Result<(), String> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .ok_or("macOS refused a keyboard event source")?;
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(Some(&source), KEY_V, down)
                .ok_or("macOS refused to create the ⌘V event")?;
            CGEvent::set_flags(Some(&event), CGEventFlags::MaskCommand);
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        }
        Ok(())
    }
}

impl ClipboardWriter for Pasteboard {
    fn write_text(&self, text: &str) -> Result<(), String> {
        if Pasteboard::write_text(self, text, false) {
            Ok(())
        } else {
            Err("the pasteboard refused the text".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records what [`ask_for_trust`] did, in order.
    #[derive(Default)]
    struct FakeTrust {
        trusted: bool,
        reset_fails: bool,
        /// How many of the settings addresses fail to open, from the first.
        failing_urls: usize,
        calls: Vec<String>,
    }

    impl TrustSystem for FakeTrust {
        fn trusted(&mut self) -> bool {
            self.trusted
        }

        fn reset(&mut self, bundle_id: &str) -> Result<(), String> {
            self.calls.push(format!("reset {bundle_id}"));
            if self.reset_fails {
                Err("tccutil failed".to_owned())
            } else {
                Ok(())
            }
        }

        fn prompt(&mut self) {
            self.calls.push("prompt".to_owned());
        }

        fn open(&mut self, url: &str) -> Result<(), String> {
            self.calls.push(format!("open {url}"));
            let tried = self.calls.iter().filter(|c| c.starts_with("open ")).count();
            if tried <= self.failing_urls {
                Err(format!("no {url}"))
            } else {
                Ok(())
            }
        }
    }

    const APP: &str = "io.github.yuyudhan.quickhop";

    fn open(url: &str) -> String {
        format!("open {url}")
    }

    #[test]
    fn an_untrusted_app_clears_its_old_entry_before_the_prompt_adds_this_copy() {
        let mut system = FakeTrust::default();
        assert_eq!(ask_for_trust(&mut system, Some(APP)), Ok(Asked::default()));
        assert_eq!(
            system.calls,
            vec![
                format!("reset {APP}"),
                "prompt".to_owned(),
                open(ACCESSIBILITY_SETTINGS_URLS[0])
            ]
        );
    }

    #[test]
    fn a_failed_reset_still_prompts_opens_the_list_and_says_why() {
        let mut system = FakeTrust {
            reset_fails: true,
            ..FakeTrust::default()
        };
        let asked = ask_for_trust(&mut system, Some(APP));
        assert_eq!(
            asked,
            Ok(Asked {
                reset_failed: Some("tccutil failed".to_owned())
            })
        );
        assert_eq!(
            system.calls[1..],
            ["prompt".to_owned(), open(ACCESSIBILITY_SETTINGS_URLS[0])]
        );
    }

    #[test]
    fn without_an_app_identity_nothing_is_reset() {
        let mut system = FakeTrust::default();
        assert!(ask_for_trust(&mut system, None).is_ok());
        assert_eq!(
            system.calls,
            vec!["prompt".to_owned(), open(ACCESSIBILITY_SETTINGS_URLS[0])]
        );
    }

    #[test]
    fn a_trusted_app_keeps_its_grant_and_only_opens_the_list() {
        let mut system = FakeTrust {
            trusted: true,
            ..FakeTrust::default()
        };
        assert!(ask_for_trust(&mut system, Some(APP)).is_ok());
        assert_eq!(system.calls, vec![open(ACCESSIBILITY_SETTINGS_URLS[0])]);
    }

    #[test]
    fn the_legacy_address_is_tried_when_the_current_one_fails() {
        let mut system = FakeTrust {
            trusted: true,
            failing_urls: 1,
            ..FakeTrust::default()
        };
        assert!(ask_for_trust(&mut system, None).is_ok());
        assert_eq!(
            system.calls,
            vec![
                open(ACCESSIBILITY_SETTINGS_URLS[0]),
                open(ACCESSIBILITY_SETTINGS_URLS[1])
            ]
        );
    }

    #[test]
    fn when_no_address_opens_the_error_names_each_failure() {
        let mut system = FakeTrust {
            trusted: true,
            failing_urls: 2,
            ..FakeTrust::default()
        };
        let error = ask_for_trust(&mut system, None).unwrap_err();
        for url in ACCESSIBILITY_SETTINGS_URLS {
            assert!(error.contains(url), "{error}");
        }
    }

    #[test]
    fn both_addresses_name_the_accessibility_anchor() {
        for url in ACCESSIBILITY_SETTINGS_URLS {
            assert!(url.ends_with("?Privacy_Accessibility"), "{url}");
        }
    }
}
