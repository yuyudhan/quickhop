//! Watching the X11 `CLIPBOARD` selection with `XFixes`: every time an application takes
//! ownership, its content is requested as `UTF8_STRING` (large ones arrive in `INCR`
//! chunks) and handed to the caller.

use std::ops::ControlFlow;

use x11rb::connection::Connection;
use x11rb::errors::{ConnectError, ConnectionError, ReplyError, ReplyOrIdError};
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, Property, WindowClass,
};

/// Why watching the clipboard failed.
#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    /// No X server to connect to.
    #[error("cannot connect to the X server: {0}")]
    Connect(#[from] ConnectError),
    /// The connection broke.
    #[error("X11: {0}")]
    Connection(#[from] ConnectionError),
    /// The X server refused a request.
    #[error("X11: {0}")]
    Reply(#[from] ReplyError),
    /// The X server refused a request or ran out of ids.
    #[error("X11: {0}")]
    ReplyOrId(#[from] ReplyOrIdError),
    /// The display named a screen the server does not have.
    #[error("X11: no screen {0}")]
    NoScreen(usize),
}

/// Watches the clipboard of the X server `display` (`None`: `$DISPLAY`) and calls `on_text`
/// with the text of every new clipboard owner, until `on_text` breaks. Owners that offer no
/// UTF-8 text (images, files) are skipped; invalid UTF-8 is replaced.
///
/// # Errors
/// When the X server cannot be reached, lacks `XFixes`, or the connection breaks.
pub fn watch_x11(
    display: Option<&str>,
    mut on_text: impl FnMut(String) -> ControlFlow<()>,
) -> Result<(), ClipboardError> {
    let (conn, screen) = x11rb::connect(display)?;
    let root = conn
        .setup()
        .roots
        .get(screen)
        .ok_or(ClipboardError::NoScreen(screen))?
        .root;
    let window = conn.generate_id()?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;
    conn.xfixes_query_version(5, 0)?.reply()?;
    let atom = |name: &[u8]| -> Result<u32, ClipboardError> {
        Ok(conn.intern_atom(false, name)?.reply()?.atom)
    };
    let clipboard = atom(b"CLIPBOARD")?;
    let utf8 = atom(b"UTF8_STRING")?;
    let property = atom(b"QUICKHOP_CLIPBOARD")?;
    let incr = atom(b"INCR")?;
    conn.xfixes_select_selection_input(window, clipboard, SelectionEventMask::SET_SELECTION_OWNER)?;
    conn.flush()?;

    // Bytes of an INCR transfer in progress.
    let mut incoming: Option<Vec<u8>> = None;
    loop {
        let text = match conn.wait_for_event()? {
            Event::XfixesSelectionNotify(event) => {
                if event.owner != x11rb::NONE {
                    incoming = None;
                    conn.convert_selection(
                        window,
                        clipboard,
                        utf8,
                        property,
                        event.selection_timestamp,
                    )?;
                    conn.flush()?;
                }
                None
            }
            // `property` is NONE when the owner has no UTF-8 text.
            Event::SelectionNotify(event) if event.property == property => {
                // Deleting the property also tells an INCR owner to send the first chunk.
                let reply = conn
                    .get_property(true, window, property, AtomEnum::ANY, 0, u32::MAX / 4)?
                    .reply()?;
                conn.flush()?;
                if reply.type_ == incr {
                    incoming = Some(Vec::new());
                    None
                } else {
                    Some(reply.value)
                }
            }
            Event::PropertyNotify(event)
                if event.atom == property
                    && event.state == Property::NEW_VALUE
                    && incoming.is_some() =>
            {
                let reply = conn
                    .get_property(true, window, property, AtomEnum::ANY, 0, u32::MAX / 4)?
                    .reply()?;
                conn.flush()?;
                if reply.value.is_empty() {
                    // A zero-length chunk ends the transfer.
                    incoming.take()
                } else {
                    if let Some(buffer) = incoming.as_mut() {
                        buffer.extend_from_slice(&reply.value);
                    }
                    None
                }
            }
            _ => None,
        };
        if let Some(bytes) = text
            && on_text(String::from_utf8_lossy(&bytes).into_owned()).is_break()
        {
            return Ok(());
        }
    }
}
