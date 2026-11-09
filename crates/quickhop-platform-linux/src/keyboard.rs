//! Synthetic key presses on X11 through the `XTest` extension, for pasting into the focused
//! window.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::protocol::xtest::ConnectionExt as _;

use crate::clipboard::ClipboardError;

/// `Control_L`.
const KEYSYM_CONTROL_L: u32 = 0xffe3;
/// Latin small letter `v`.
const KEYSYM_V: u32 = 0x0076;

/// The keycode that produces `keysym` in the server's keyboard mapping.
fn keycode<C: Connection>(conn: &C, keysym: u32) -> Result<Option<u8>, ClipboardError> {
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let mapping = conn
        .get_keyboard_mapping(min, max.saturating_sub(min).saturating_add(1))?
        .reply()?;
    let per = usize::from(mapping.keysyms_per_keycode).max(1);
    Ok(mapping
        .keysyms
        .chunks(per)
        .position(|syms| syms.contains(&keysym))
        .and_then(|index| u8::try_from(index).ok())
        .map(|index| min.saturating_add(index)))
}

/// Presses and releases Ctrl+V in the X server `display` (`None`: `$DISPLAY`), which pastes
/// the clipboard into the focused window of most applications (terminals want
/// Ctrl+Shift+V).
///
/// # Errors
/// When the server cannot be reached, lacks `XTest`, or has no key for Control or V.
pub fn press_ctrl_v(display: Option<&str>) -> Result<(), ClipboardError> {
    let (conn, screen) = x11rb::connect(display)?;
    let root = conn
        .setup()
        .roots
        .get(screen)
        .ok_or(ClipboardError::NoScreen(screen))?
        .root;
    conn.xtest_get_version(2, 2)?.reply()?;
    let control = keycode(&conn, KEYSYM_CONTROL_L)?.ok_or(ClipboardError::NoKey("Control_L"))?;
    let v = keycode(&conn, KEYSYM_V)?.ok_or(ClipboardError::NoKey("v"))?;
    for (kind, code) in [
        (xproto::KEY_PRESS_EVENT, control),
        (xproto::KEY_PRESS_EVENT, v),
        (xproto::KEY_RELEASE_EVENT, v),
        (xproto::KEY_RELEASE_EVENT, control),
    ] {
        conn.xtest_fake_input(kind, code, x11rb::CURRENT_TIME, root, 0, 0, 0)?;
    }
    // A round trip makes sure the server handled the events before the connection closes.
    conn.get_input_focus()?.reply()?;
    Ok(())
}
