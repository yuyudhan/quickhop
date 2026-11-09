//! Sending `⌘V` to the frontmost app (CLP-03) and the Accessibility permission it needs
//! (PLT-02).
//!
//! macOS delivers synthesized key events to other apps only from processes the user trusted
//! under System Settings › Privacy & Security › Accessibility. [`is_trusted`] checks that
//! without asking; [`request_trust`] also shows the system prompt that leads the user there.

use objc2_application_services::{
    AXIsProcessTrusted, AXIsProcessTrustedWithOptions, kAXTrustedCheckOptionPrompt,
};
use objc2_core_foundation::{CFBoolean, CFDictionary, CFString};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};
use quickhop_core::paste::{ClipboardWriter, PasteKeys};

use crate::pasteboard::Pasteboard;

/// The virtual key code of the `V` key on an ANSI keyboard (`kVK_ANSI_V`).
const KEY_V: u16 = 9;

/// The Accessibility pane of System Settings, for an "Open Settings" action.
pub const ACCESSIBILITY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

/// Whether this process may send key events to other apps. Never shows a prompt.
pub fn is_trusted() -> bool {
    // SAFETY: `AXIsProcessTrusted` takes no arguments and only reads the TCC database.
    unsafe { AXIsProcessTrusted() }
}

/// Like [`is_trusted`], but when the process is not trusted macOS also shows its prompt that
/// opens the Accessibility settings (once per launch of the process).
pub fn request_trust() -> bool {
    // SAFETY: `kAXTrustedCheckOptionPrompt` is an immutable `CFString` constant exported by
    // the ApplicationServices framework, valid for the life of the process.
    let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
    let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[CFBoolean::new(true)]);
    // SAFETY: `options` is a valid dictionary of the documented key and a `CFBoolean` value,
    // alive for the whole call.
    unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) }
}

/// Sends `⌘V` through the HID event tap, so it reaches whichever app is frontmost.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemKeys;

impl PasteKeys for SystemKeys {
    fn can_send_keys(&self) -> bool {
        is_trusted()
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
    fn write_text(&self, text: &str) -> bool {
        Pasteboard::write_text(self, text, false)
    }
}
