//! The current keyboard layout, read the way apps read it (`UCKeyTranslate` over the layout's
//! `uchr` data): which key, with or without `Shift`, types a character. Synthesized typing
//! needs it because apps, gpui included, turn key codes into characters through the layout,
//! and hotkeys need it so `Cmd+K` means the key labelled K on a Dvorak or AZERTY keyboard too.

use std::collections::HashMap;
use std::ptr::NonNull;

use objc2_core_foundation::{CFData, CFRetained, CFString, CFType};

// SAFETY: the declarations match HIToolbox's `TextInputSources.h` and `UnicodeUtilities.h`;
// `TISCopyCurrentKeyboardLayoutInputSource` returns a +1 reference or null,
// `TISGetInputSourceProperty` a reference owned by the source.
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardLayoutInputSource() -> Option<NonNull<CFType>>;
    fn TISGetInputSourceProperty(source: &CFType, key: &CFString) -> Option<NonNull<CFData>>;
    static kTISPropertyUnicodeKeyLayoutData: &'static CFString;
    fn LMGetKbdType() -> u8;
    fn UCKeyTranslate(
        layout: NonNull<u8>,
        code: u16,
        action: u16,
        modifier_state: u32,
        keyboard_type: u32,
        options: u32,
        dead_key_state: &mut u32,
        max_length: usize,
        length: &mut usize,
        chars: *mut u16,
    ) -> i32;
}

/// `kUCKeyActionDown`.
const KEY_DOWN: u16 = 0;
/// `shiftKey >> 8`, the modifier state `UCKeyTranslate` takes.
const SHIFT: u32 = 0x02;
/// `kUCKeyTranslateNoDeadKeysMask`: a dead key gives its own character.
const NO_DEAD_KEYS: u32 = 1;
/// Virtual key codes run from 0 to 127.
const KEY_CODES: u16 = 128;

/// A key of the current layout that types one character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Typed {
    /// The virtual key code.
    pub code: u16,
    /// Whether `Shift` is held.
    pub shift: bool,
}

/// Which key types each character on a keyboard layout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout(HashMap<char, Typed>);

impl Layout {
    /// The layout in use now ([`characters`]).
    pub fn current() -> Self {
        Self(characters())
    }

    /// The key that types `character`, if any.
    pub fn get(&self, character: char) -> Option<Typed> {
        self.0.get(&character).copied()
    }
}

impl FromIterator<(char, Typed)> for Layout {
    fn from_iter<I: IntoIterator<Item = (char, Typed)>>(keys: I) -> Self {
        Self(keys.into_iter().collect())
    }
}

/// Which key types each character on the current keyboard layout, the unshifted key first
/// when both do. Empty when the layout has no Unicode data (a few input methods).
fn characters() -> HashMap<char, Typed> {
    let mut found = HashMap::new();
    // SAFETY: no arguments; the result is null or a +1 reference, which `CFRetained` releases.
    let Some(source) = (unsafe { TISCopyCurrentKeyboardLayoutInputSource() }) else {
        return found;
    };
    // SAFETY: `source` is the +1 reference just returned.
    let source = unsafe { CFRetained::from_raw(source) };
    // SAFETY: an immutable `CFString` constant exported by HIToolbox.
    let key = unsafe { kTISPropertyUnicodeKeyLayoutData };
    // SAFETY: `source` is a live input source and `key` one of its property names; the data
    // is owned by `source`, which outlives every use below.
    let Some(data) = (unsafe { TISGetInputSourceProperty(&source, key) }) else {
        return found;
    };
    // SAFETY: a valid `CFData` kept alive by `source`.
    let data = unsafe { data.as_ref() };
    let Some(layout) = NonNull::new(data.byte_ptr().cast_mut()) else {
        return found;
    };
    // SAFETY: no arguments.
    let keyboard = u32::from(unsafe { LMGetKbdType() });
    for shift in [false, true] {
        for code in 0..KEY_CODES {
            let mut dead = 0;
            let mut length = 0;
            let mut chars = [0_u16; 4];
            // SAFETY: `layout` is the layout's `uchr` data, `chars` has room for
            // `chars.len()` units and `length` receives how many were written.
            let status = unsafe {
                UCKeyTranslate(
                    layout,
                    code,
                    KEY_DOWN,
                    if shift { SHIFT } else { 0 },
                    keyboard,
                    NO_DEAD_KEYS,
                    &mut dead,
                    chars.len(),
                    &mut length,
                    chars.as_mut_ptr(),
                )
            };
            if status != 0 {
                continue;
            }
            let mut decoded = char::decode_utf16(chars[..length.min(chars.len())].iter().copied());
            if let (Some(Ok(character)), None) = (decoded.next(), decoded.next())
                && !character.is_control()
            {
                found.entry(character).or_insert(Typed { code, shift });
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_have_keys_and_shift_gives_capitals() {
        // Holds on every Latin layout (US, Dvorak, AZERTY, QWERTZ, ...), whichever is set.
        let keys = characters();
        for (lower, upper) in ('a'..='z').zip('A'..='Z') {
            let typed = keys[&lower];
            assert!(!typed.shift, "{lower}: {typed:?}");
            assert_eq!(
                keys[&upper],
                Typed {
                    shift: true,
                    ..typed
                },
                "{upper}"
            );
        }
        assert_eq!(
            keys[&' '],
            Typed {
                code: 49,
                shift: false
            }
        );
    }
}
