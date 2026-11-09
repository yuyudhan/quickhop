//! Global hotkeys (LCH-01) through the Carbon Event Manager's `RegisterEventHotKey`, the API
//! macOS launchers use: it needs no permission, and the system delivers the key press to the
//! app that registered it, whichever app is in front. The press arrives on the main thread's
//! run loop (gpui's `NSApplication` run loop serves it).
//!
//! A key that types a character is found through the current keyboard layout, so `Cmd+K` is
//! the key labelled K on a Dvorak or AZERTY keyboard too; other keys have fixed codes.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};

use objc2::MainThreadMarker;
use quickhop_core::hotkey::{Hotkey, Key, Modifiers};

use crate::layout::Layout;

/// `EventHotKeyID`.
#[repr(C, packed(2))]
#[derive(Clone, Copy)]
struct HotKeyId {
    signature: u32,
    id: u32,
}

/// `EventTypeSpec`.
#[repr(C, packed(2))]
struct EventTypeSpec {
    class: u32,
    kind: u32,
}

type Handler = extern "C" fn(call: *mut c_void, event: *mut c_void, data: *mut c_void) -> i32;

// SAFETY: the declarations match `CarbonEvents.h`; the refs are opaque pointers owned by the
// Event Manager until they are unregistered or removed.
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: Handler,
        count: usize,
        types: *const EventTypeSpec,
        data: *mut c_void,
        installed: &mut *mut c_void,
    ) -> i32;
    fn RemoveEventHandler(handler: *mut c_void) -> i32;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: HotKeyId,
        target: *mut c_void,
        options: u32,
        registered: &mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hotkey: *mut c_void) -> i32;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        kind: u32,
        actual_kind: *mut u32,
        size: usize,
        actual_size: *mut usize,
        data: *mut c_void,
    ) -> i32;
}

const fn four_chars(text: [u8; 4]) -> u32 {
    u32::from_be_bytes(text)
}

/// `kEventClassKeyboard`.
const KEYBOARD_CLASS: u32 = four_chars(*b"keyb");
/// `kEventHotKeyPressed`.
const HOT_KEY_PRESSED: u32 = 5;
/// `kEventParamDirectObject`.
const DIRECT_OBJECT: u32 = four_chars(*b"----");
/// `typeEventHotKeyID`.
const HOT_KEY_ID_TYPE: u32 = four_chars(*b"hkid");
/// `eventNotHandledErr`: let the next handler look at the event.
const NOT_HANDLED: i32 = -9874;
/// `eventHotKeyExistsErr`.
const EXISTS: i32 = -9878;
/// The signature quickhop's hotkeys carry.
const SIGNATURE: u32 = four_chars(*b"qhop");

/// Carbon modifier bits (`cmdKey`, `shiftKey`, `optionKey`, `controlKey`).
fn modifier_bits(modifiers: Modifiers) -> u32 {
    let Modifiers {
        command,
        control,
        alt,
        shift,
    } = modifiers;
    [
        (command, 1 << 8),
        (shift, 1 << 9),
        (alt, 1 << 11),
        (control, 1 << 12),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .map(|(_, bit)| bit)
    .sum()
}

/// The character a key types unshifted on a US keyboard, for the keys found through the
/// layout.
fn character(key: Key) -> Option<char> {
    Some(match key {
        Key::Letter(c) => c.to_ascii_lowercase(),
        Key::Digit(c) => c,
        Key::Minus => '-',
        Key::Equal => '=',
        Key::LeftBracket => '[',
        Key::RightBracket => ']',
        Key::Backslash => '\\',
        Key::Semicolon => ';',
        Key::Quote => '\'',
        Key::Comma => ',',
        Key::Period => '.',
        Key::Slash => '/',
        Key::Backquote => '`',
        _ => return None,
    })
}

/// Virtual key codes of the US (ANSI) layout, for characters the current layout lacks.
const ANSI: &[(char, u16)] = &[
    ('a', 0x00),
    ('s', 0x01),
    ('d', 0x02),
    ('f', 0x03),
    ('h', 0x04),
    ('g', 0x05),
    ('z', 0x06),
    ('x', 0x07),
    ('c', 0x08),
    ('v', 0x09),
    ('b', 0x0B),
    ('q', 0x0C),
    ('w', 0x0D),
    ('e', 0x0E),
    ('r', 0x0F),
    ('y', 0x10),
    ('t', 0x11),
    ('1', 0x12),
    ('2', 0x13),
    ('3', 0x14),
    ('4', 0x15),
    ('6', 0x16),
    ('5', 0x17),
    ('=', 0x18),
    ('9', 0x19),
    ('7', 0x1A),
    ('-', 0x1B),
    ('8', 0x1C),
    ('0', 0x1D),
    (']', 0x1E),
    ('o', 0x1F),
    ('u', 0x20),
    ('[', 0x21),
    ('i', 0x22),
    ('p', 0x23),
    ('l', 0x25),
    ('j', 0x26),
    ('\'', 0x27),
    ('k', 0x28),
    (';', 0x29),
    ('\\', 0x2A),
    (',', 0x2B),
    ('/', 0x2C),
    ('n', 0x2D),
    ('m', 0x2E),
    ('.', 0x2F),
    ('`', 0x32),
];

/// Virtual key codes of `F1` to `F20` (`kVK_F1` ...).
const FUNCTION: [u16; 20] = [
    0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F, 0x69, 0x6B, 0x71, 0x6A,
    0x40, 0x4F, 0x50, 0x5A,
];

/// The virtual key code of `key` on `layout`.
pub fn key_code(key: Key, layout: &Layout) -> Option<u16> {
    if let Some(character) = character(key) {
        return layout.get(character).map(|typed| typed.code).or_else(|| {
            ANSI.iter()
                .find(|(ansi, _)| *ansi == character)
                .map(|(_, code)| *code)
        });
    }
    Some(match key {
        Key::Function(n) => *FUNCTION.get(usize::from(n).checked_sub(1)?)?,
        Key::Space => 0x31,
        Key::Return => 0x24,
        Key::Tab => 0x30,
        Key::Escape => 0x35,
        Key::Backspace => 0x33,
        Key::Delete => 0x75,
        Key::Up => 0x7E,
        Key::Down => 0x7D,
        Key::Left => 0x7B,
        Key::Right => 0x7C,
        Key::Home => 0x73,
        Key::End => 0x77,
        Key::PageUp => 0x74,
        Key::PageDown => 0x79,
        _ => return None,
    })
}

/// Why a hotkey could not be registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegisterError {
    /// Called on another thread than the main one.
    #[error("hotkeys are registered on the main thread")]
    NotMainThread,
    /// The keyboard layout has no such key.
    #[error("no key for {0} on this keyboard layout")]
    NoKey(Hotkey),
    /// Another app holds the combination.
    #[error("{0} is taken by another app")]
    Taken(Hotkey),
    /// The Event Manager refused for another reason.
    #[error("macOS refused {hotkey} (OSStatus {status})")]
    Refused {
        /// The hotkey.
        hotkey: Hotkey,
        /// The Event Manager's status.
        status: i32,
    },
}

/// What a handler needs: which hotkey is its own, and what to do.
struct Callback {
    id: u32,
    on_press: Box<dyn Fn()>,
}

/// A registered hotkey; dropping it unregisters it.
pub struct Registration {
    hotkey: NonNull<c_void>,
    handler: NonNull<c_void>,
    /// From `Box::into_raw`: read by [`handle`] through the pointer the Event Manager holds,
    /// freed by `drop` once the handler is removed.
    callback: *mut Callback,
}

/// The Event Manager's handler: runs the callback when the pressed hotkey is this one.
extern "C" fn handle(_call: *mut c_void, event: *mut c_void, data: *mut c_void) -> i32 {
    let mut pressed = HotKeyId {
        signature: 0,
        id: 0,
    };
    // SAFETY: `event` is the event being handled; `pressed` has room for an `EventHotKeyID`.
    let status = unsafe {
        GetEventParameter(
            event,
            DIRECT_OBJECT,
            HOT_KEY_ID_TYPE,
            std::ptr::null_mut(),
            size_of::<HotKeyId>(),
            std::ptr::null_mut(),
            (&raw mut pressed).cast(),
        )
    };
    // SAFETY: `data` is the `Callback` installed with this handler, alive until the handler
    // is removed in `Registration::drop`.
    let callback = unsafe { &*data.cast::<Callback>() };
    let HotKeyId { signature, id } = pressed;
    if status != 0 || signature != SIGNATURE || id != callback.id {
        return NOT_HANDLED;
    }
    (callback.on_press)();
    0
}

/// Registers `hotkey` and runs `on_press` on the main thread each time it is pressed, until
/// the [`Registration`] is dropped. Call it on the main thread; `on_press` must return
/// quickly: the app's run loop waits for it.
///
/// # Errors
/// [`RegisterError`] off the main thread, when the layout lacks the key, another app holds
/// the combination, or the Event Manager refuses.
pub fn register(
    hotkey: Hotkey,
    on_press: impl Fn() + 'static,
) -> Result<Registration, RegisterError> {
    static NEXT_ID: AtomicU32 = AtomicU32::new(1);
    MainThreadMarker::new().ok_or(RegisterError::NotMainThread)?;
    let code = key_code(hotkey.key, &Layout::current()).ok_or(RegisterError::NoKey(hotkey))?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let callback = Box::into_raw(Box::new(Callback {
        id,
        on_press: Box::new(on_press),
    }));
    // SAFETY: `callback` came from `Box::into_raw` and nothing else holds it once the handler
    // is removed (or was never installed).
    let free = || drop(unsafe { Box::from_raw(callback) });
    // SAFETY: called on the main thread (the marker), which owns the application target.
    let target = unsafe { GetApplicationEventTarget() };
    let types = EventTypeSpec {
        class: KEYBOARD_CLASS,
        kind: HOT_KEY_PRESSED,
    };
    let mut handler = std::ptr::null_mut();
    // SAFETY: `types` is one valid spec; `callback` stays allocated until the handler is
    // removed (in `drop`, or below on failure).
    let status = unsafe {
        InstallEventHandler(
            target,
            handle,
            1,
            &raw const types,
            callback.cast(),
            &mut handler,
        )
    };
    let Some(handler) = NonNull::new(handler).filter(|_| status == 0) else {
        free();
        return Err(RegisterError::Refused { hotkey, status });
    };
    let mut registered = std::ptr::null_mut();
    let hotkey_id = HotKeyId {
        signature: SIGNATURE,
        id,
    };
    // SAFETY: plain values and an out pointer, on the main thread.
    let status = unsafe {
        RegisterEventHotKey(
            u32::from(code),
            modifier_bits(hotkey.modifiers),
            hotkey_id,
            target,
            0,
            &mut registered,
        )
    };
    let Some(registered) = NonNull::new(registered).filter(|_| status == 0) else {
        // SAFETY: `handler` was installed above and is removed once.
        unsafe {
            RemoveEventHandler(handler.as_ptr());
        }
        free();
        return Err(if status == EXISTS {
            RegisterError::Taken(hotkey)
        } else {
            RegisterError::Refused { hotkey, status }
        });
    };
    Ok(Registration {
        hotkey: registered,
        handler,
        callback,
    })
}

impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: both refs came from a successful registration and are released once, on the
        // main thread that owns them (`Registration` is neither `Send` nor `Sync`).
        unsafe {
            UnregisterEventHotKey(self.hotkey.as_ptr());
            RemoveEventHandler(self.handler.as_ptr());
        }
        // SAFETY: from `Box::into_raw` in `register`; the handler that read it is gone.
        drop(unsafe { Box::from_raw(self.callback) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Typed;

    fn hotkey(text: &str) -> Hotkey {
        Hotkey::parse(text).unwrap_or_else(|error| panic!("{text}: {error}"))
    }

    #[test]
    fn modifiers_map_to_carbon_bits() {
        assert_eq!(modifier_bits(hotkey("Alt+Space").modifiers), 0x800);
        assert_eq!(modifier_bits(hotkey("Cmd+Shift+K").modifiers), 0x300);
        assert_eq!(
            modifier_bits(hotkey("Cmd+Ctrl+Alt+Shift+K").modifiers),
            0x100 | 0x200 | 0x800 | 0x1000
        );
        assert_eq!(modifier_bits(hotkey("F5").modifiers), 0);
    }

    #[test]
    fn keys_have_their_virtual_codes_on_a_us_layout() {
        let us = Layout::default(); // nothing from the layout: the ANSI table answers
        let code = |text: &str| key_code(hotkey(text).key, &us);
        assert_eq!(code("Alt+Space"), Some(0x31));
        assert_eq!(code("Cmd+K"), Some(0x28));
        assert_eq!(code("Cmd+V"), Some(0x09));
        assert_eq!(code("Ctrl+0"), Some(0x1D));
        assert_eq!(code("F1"), Some(0x7A));
        assert_eq!(code("F13"), Some(0x69));
        assert_eq!(code("F20"), Some(0x5A));
        assert_eq!(code("Ctrl+/"), Some(0x2C));
        assert_eq!(code("Ctrl+Left"), Some(0x7B));
    }

    #[test]
    fn typing_keys_follow_the_layout() {
        // Dvorak puts K where QWERTY has T.
        let dvorak = Layout::from_iter([(
            'k',
            Typed {
                code: 0x11,
                shift: false,
            },
        )]);
        assert_eq!(key_code(hotkey("Cmd+K").key, &dvorak), Some(0x11));
        assert_eq!(key_code(hotkey("Cmd+J").key, &dvorak), Some(0x26));
    }

    #[test]
    fn every_key_has_a_code_and_no_two_share_one() {
        let keys = (b'A'..=b'Z')
            .map(|c| Key::Letter(char::from(c)))
            .chain((b'0'..=b'9').map(|d| Key::Digit(char::from(d))))
            .chain((1..=20).map(Key::Function))
            .chain([
                Key::Space,
                Key::Return,
                Key::Tab,
                Key::Escape,
                Key::Backspace,
                Key::Delete,
                Key::Up,
                Key::Down,
                Key::Left,
                Key::Right,
                Key::Home,
                Key::End,
                Key::PageUp,
                Key::PageDown,
                Key::Minus,
                Key::Equal,
                Key::LeftBracket,
                Key::RightBracket,
                Key::Backslash,
                Key::Semicolon,
                Key::Quote,
                Key::Comma,
                Key::Period,
                Key::Slash,
                Key::Backquote,
            ]);
        let mut seen = std::collections::HashMap::new();
        for key in keys {
            let code = key_code(key, &Layout::default()).unwrap_or_else(|| panic!("{key:?}"));
            if let Some(other) = seen.insert(code, key) {
                panic!("{key:?} and {other:?} both have code {code:#x}");
            }
        }
    }
}
