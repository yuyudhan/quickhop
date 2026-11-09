//! Real input and what is on screen, for the UI tests (`quickhop-uitest`, plan "How the UI is
//! tested"): key presses and typing posted through the HID event tap as a person's would be,
//! the frontmost app, how long since the last input, and the windows on screen.
//!
//! Posting keys needs the Accessibility permission ([`crate::keys::is_trusted`]); reading the
//! window list's owners, ids and bounds needs none (titles would need Screen Recording and are
//! not read).

use std::ptr::NonNull;

use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_application_services::{AXError, AXUIElement};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType,
};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGEventType,
    CGSessionCopyCurrentDictionary, CGWindowListCopyWindowInfo, CGWindowListOption,
    kCGNullWindowID, kCGWindowBounds, kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerPID,
};
use objc2_foundation::NSString;
use quickhop_core::hotkey::{Hotkey, Modifiers};

use crate::layout::{Layout, Typed};

/// A key the UI tests press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// `Return`.
    Return,
    /// `Esc`.
    Escape,
    /// `Delete` (backspace).
    Delete,
    /// `Tab`.
    Tab,
    /// The down arrow.
    Down,
    /// The up arrow.
    Up,
    /// The key that types this character on the current layout (`⌘K`: `Char('k')`).
    Char(char),
}

impl Key {
    /// The virtual key code and whether `Shift` goes with it.
    fn typed(self) -> Result<Typed, String> {
        let code = match self {
            Self::Return => 36,
            Self::Escape => 53,
            Self::Delete => 51,
            Self::Tab => 48,
            Self::Down => 125,
            Self::Up => 126,
            Self::Char(character) => return typed(&Layout::current(), character),
        };
        Ok(Typed { code, shift: false })
    }
}

fn typed(layout: &Layout, character: char) -> Result<Typed, String> {
    layout
        .get(character)
        .ok_or_else(|| format!("no key types {character:?} on this keyboard layout"))
}

fn source() -> Result<CFRetained<CGEventSource>, String> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .ok_or_else(|| "macOS refused a keyboard event source".to_owned())
}

/// Posts a press and release of the key `code` with `flags` held, as the keyboard would.
fn post(source: &CGEventSource, code: u16, flags: CGEventFlags) -> Result<(), String> {
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(Some(source), code, down)
            .ok_or("macOS refused to create a key event")?;
        CGEvent::set_flags(Some(&event), flags);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    }
    Ok(())
}

fn shift_flag(shift: bool) -> CGEventFlags {
    if shift {
        CGEventFlags::MaskShift
    } else {
        CGEventFlags(0)
    }
}

/// Presses and releases `key`, with `⌘` held when `command` is set.
///
/// # Errors
/// When the layout has no key for a [`Key::Char`], or macOS refuses to create the events.
pub fn press(key: Key, command: bool) -> Result<(), String> {
    let Typed { code, shift } = key.typed()?;
    let mut flags = shift_flag(shift);
    if command {
        flags |= CGEventFlags::MaskCommand;
    }
    let source = source()?;
    post(&source, code, flags)
}

/// Presses and releases `hotkey`'s key with its modifiers held. Function, arrow and
/// navigation keys carry the `fn` flag, as a keyboard's own presses of them do.
///
/// # Errors
/// When the layout has no key for it, or macOS refuses to create the events.
pub fn press_hotkey(hotkey: &Hotkey) -> Result<(), String> {
    use quickhop_core::hotkey::Key as Named;

    let code = crate::hotkey::key_code(hotkey.key, &Layout::current())
        .ok_or_else(|| format!("no key for {hotkey} on this keyboard layout"))?;
    let Modifiers {
        command,
        control,
        alt,
        shift,
    } = hotkey.modifiers;
    let function = matches!(
        hotkey.key,
        Named::Function(_)
            | Named::Up
            | Named::Down
            | Named::Left
            | Named::Right
            | Named::Home
            | Named::End
            | Named::PageUp
            | Named::PageDown
            | Named::Delete
    );
    let mut flags = CGEventFlags(0);
    for (held, flag) in [
        (command, CGEventFlags::MaskCommand),
        (control, CGEventFlags::MaskControl),
        (alt, CGEventFlags::MaskAlternate),
        (shift, CGEventFlags::MaskShift),
        (function, CGEventFlags::MaskSecondaryFn),
    ] {
        if held {
            flags |= flag;
        }
    }
    let source = source()?;
    post(&source, code, flags)
}

/// Types `text`, one key press per character, through the current keyboard layout (apps turn
/// key codes into characters with it).
///
/// # Errors
/// When a character has no key on the layout (nothing is typed then), or macOS refuses to
/// create the events.
pub fn type_text(text: &str) -> Result<(), String> {
    let keys = Layout::current();
    let presses = text
        .chars()
        .map(|character| typed(&keys, character))
        .collect::<Result<Vec<_>, _>>()?;
    let source = source()?;
    for Typed { code, shift } in presses {
        post(&source, code, shift_flag(shift))?;
        // A person types a few characters per second; the app sees each one on its own.
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    Ok(())
}

/// Seconds since the last keyboard or pointer input from any source, people's included.
pub fn seconds_since_input() -> f64 {
    // `kCGAnyInputEventType`: every kind of input.
    let any_input = CGEventType(u32::MAX);
    CGEventSource::seconds_since_last_event_type(CGEventSourceStateID::HIDSystemState, any_input)
}

/// Whether the console's screen is locked (an ssh session sees the console's too), or `None`
/// when the window server reports no session at all. Keys posted while it is locked go to the
/// lock screen's password field, so the UI tests never post any then.
pub fn screen_locked() -> Option<bool> {
    let session = CGSessionCopyCurrentDictionary()?;
    // SAFETY: the session dictionary maps `CFString` keys to property-list values.
    let session: &CFDictionary<CFString, CFType> = unsafe { session.cast_unchecked() };
    // Undocumented but long stable: the window server sets it to true while locked.
    let locked = session
        .get(&CFString::from_static_str("CGSSessionScreenIsLocked"))
        .and_then(|value| value.downcast::<CFBoolean>().ok())
        .is_some_and(|value| value.as_bool());
    Some(locked)
}

/// The process id of the frontmost app.
pub fn frontmost_pid() -> Option<i32> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
}

/// The process id of the app with the keyboard focus, as Accessibility reports it. Unlike
/// [`frontmost_pid`] it names the owner of a non-activating panel (quickhop's window) that has
/// the focus while another app stays in front. Needs the Accessibility permission.
pub fn focused_pid() -> Option<i32> {
    // SAFETY: no arguments; returns a new reference.
    let system = unsafe { AXUIElement::new_system_wide() };
    let app = attribute(&system, "AXFocusedApplication")?
        .downcast::<AXUIElement>()
        .ok()?;
    let mut pid = 0;
    // SAFETY: `pid` receives the element's process id.
    let error = unsafe { app.pid(NonNull::from(&mut pid)) };
    (error == AXError::Success).then_some(pid)
}

/// The title of the focused window of the app with process id `pid`, as Accessibility
/// reports it: System Settings names the pane it shows. Needs the Accessibility permission.
pub fn focused_window_title(pid: i32) -> Option<String> {
    // SAFETY: any process id is accepted; returns a new reference.
    let app = unsafe { AXUIElement::new_application(pid) };
    let window = attribute(&app, "AXFocusedWindow")?
        .downcast::<AXUIElement>()
        .ok()?;
    let title = attribute(&window, "AXTitle")?.downcast::<CFString>().ok()?;
    Some(title.to_string())
}

/// The value of `element`'s attribute `name`, when it has one.
fn attribute(element: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let mut value: *const CFType = std::ptr::null();
    // SAFETY: `value` receives a +1 reference when the call succeeds.
    let error = unsafe {
        element.copy_attribute_value(&CFString::from_static_str(name), NonNull::from(&mut value))
    };
    if error != AXError::Success {
        return None;
    }
    // SAFETY: the +1 reference just returned, released when dropped.
    Some(unsafe { CFRetained::from_raw(NonNull::new(value.cast_mut())?) })
}

/// The bundle identifier of the running app with process id `pid`, else its name, for
/// messages about who has the focus.
pub fn app_name(pid: i32) -> Option<String> {
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
    app.bundleIdentifier()
        .or_else(|| app.localizedName())
        .map(|name| name.to_string())
}

/// Whether an app with the bundle identifier `bundle_id` runs.
pub fn is_running(bundle_id: &str) -> bool {
    !NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle_id))
        .is_empty()
}

/// Asks every running app with the bundle identifier `bundle_id` to quit, as `⌘Q` does.
pub fn quit(bundle_id: &str) {
    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(
        bundle_id,
    ));
    for app in &apps {
        app.terminate();
    }
}

/// One window on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnScreen {
    /// The window server's id (`screencapture -l <id>` captures it).
    pub id: u32,
    /// The owning process.
    pub pid: i32,
    /// The window layer: 0 for normal windows, higher for panels and the menu bar.
    pub layer: i32,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
}

/// Every window on screen, front to back, without the desktop's own.
pub fn windows_on_screen() -> Vec<OnScreen> {
    let options =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(list) = CGWindowListCopyWindowInfo(options, kCGNullWindowID) else {
        return Vec::new();
    };
    // SAFETY: the window server documents the list as one dictionary per window, keyed by
    // `CFString`s.
    let list: &CFArray<CFDictionary<CFString, CFType>> = unsafe { list.cast_unchecked() };
    (0..list.len())
        .filter_map(|index| list.get(index))
        .filter_map(|window| on_screen(&window))
        .collect()
}

fn number(
    dictionary: &CFDictionary<CFString, CFType>,
    key: &CFString,
) -> Option<CFRetained<CFNumber>> {
    dictionary.get(key)?.downcast::<CFNumber>().ok()
}

fn integer<T: TryFrom<i64>>(
    dictionary: &CFDictionary<CFString, CFType>,
    key: &CFString,
) -> Option<T> {
    T::try_from(number(dictionary, key)?.as_i64()?).ok()
}

fn real(dictionary: &CFDictionary<CFString, CFType>, key: &str) -> Option<f64> {
    number(dictionary, &CFString::from_str(key))?.as_f64()
}

fn on_screen(window: &CFDictionary<CFString, CFType>) -> Option<OnScreen> {
    // SAFETY: the `kCGWindow*` keys are immutable `CFString` constants exported by
    // CoreGraphics, valid for the life of the process.
    let (id_key, pid_key, layer_key, bounds_key) = unsafe {
        (
            kCGWindowNumber,
            kCGWindowOwnerPID,
            kCGWindowLayer,
            kCGWindowBounds,
        )
    };
    let bounds = window.get(bounds_key)?.downcast::<CFDictionary>().ok()?;
    // SAFETY: the window server documents the bounds as a dictionary of `CFNumber`s keyed by
    // `CFString`s (`X`, `Y`, `Width`, `Height`).
    let bounds: &CFDictionary<CFString, CFType> = unsafe { bounds.cast_unchecked() };
    Some(OnScreen {
        id: integer(window, id_key)?,
        pid: integer(window, pid_key)?,
        layer: integer(window, layer_key)?,
        width: real(bounds, "Width")?,
        height: real(bounds, "Height")?,
    })
}
