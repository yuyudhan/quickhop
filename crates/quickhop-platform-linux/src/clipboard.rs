//! Watching the X11 `CLIPBOARD` selection with `XFixes`: every time an application takes
//! ownership, its targets are requested, then, unless it offers the password manager hint,
//! its content as `UTF8_STRING` (large ones arrive in `INCR` chunks); the caller gets
//! [`Contents`].

use std::ops::ControlFlow;

use quickhop_core::Contents;
use quickhop_core::clipboard::PASSWORD_MANAGER_HINT;
use x11rb::connection::Connection;
use x11rb::errors::{ConnectError, ConnectionError, ReplyError, ReplyOrIdError};
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, GetPropertyReply, Property,
    WindowClass,
};
use x11rb::rust_connection::RustConnection;

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
    /// No key in the keyboard mapping produces a needed key symbol.
    #[error("X11: no key for {0} in the keyboard mapping")]
    NoKey(&'static str),
}

/// What the watcher asks the clipboard owner for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want {
    /// The targets (formats) the owner offers.
    Targets,
    /// The text, as `UTF8_STRING`.
    Text,
}

/// What the owner answered to a [`Want`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer<'a> {
    /// The owner cannot convert to what was asked (the reply property is `None`).
    Refused,
    /// The targets offered, as atoms.
    Targets(&'a [u32]),
    /// The bytes of the text.
    Text(&'a [u8]),
}

/// What to do after an [`Answer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Ask the owner for this next.
    Ask(Want),
    /// The owner's copy is read.
    Done(Contents),
}

/// Reads one clipboard owner's copy: its targets first, so a copy that offers the password
/// manager hint (`hint`, the atom of [`PASSWORD_MANAGER_HINT`]) is a secret whose text is
/// never requested; then the text. Owners without `TARGETS` are asked for the text directly.
#[must_use]
pub fn next(asked: Want, answer: Answer<'_>, hint: u32) -> Next {
    match (asked, answer) {
        (Want::Targets, Answer::Targets(targets)) if targets.contains(&hint) => {
            Next::Done(Contents::Secret)
        }
        (Want::Targets, _) => Next::Ask(Want::Text),
        (Want::Text, Answer::Text(bytes)) => Next::Done(Contents::Text {
            text: String::from_utf8_lossy(bytes).into_owned(),
            source_app: None,
        }),
        (Want::Text, _) => Next::Done(Contents::Other),
    }
}

/// A connection to the X server with a hidden window that receives the clipboard owners'
/// answers, and the atoms the watcher uses.
struct Session {
    conn: RustConnection,
    window: u32,
    clipboard: u32,
    targets: u32,
    utf8: u32,
    hint: u32,
    property: u32,
    incr: u32,
}

impl Session {
    /// Connects to `display` and subscribes to changes of the clipboard's owner.
    fn open(display: Option<&str>) -> Result<Self, ClipboardError> {
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
        let session = Self {
            clipboard: atom(b"CLIPBOARD")?,
            targets: atom(b"TARGETS")?,
            utf8: atom(b"UTF8_STRING")?,
            hint: atom(PASSWORD_MANAGER_HINT.as_bytes())?,
            property: atom(b"QUICKHOP_CLIPBOARD")?,
            incr: atom(b"INCR")?,
            window,
            conn,
        };
        session.conn.xfixes_select_selection_input(
            window,
            session.clipboard,
            SelectionEventMask::SET_SELECTION_OWNER,
        )?;
        session.conn.flush()?;
        Ok(session)
    }

    /// Asks the owner that took the clipboard at `time` for `want`.
    fn ask(&self, want: Want, time: u32) -> Result<(), ClipboardError> {
        let target = match want {
            Want::Targets => self.targets,
            Want::Text => self.utf8,
        };
        self.conn
            .convert_selection(self.window, self.clipboard, target, self.property, time)?;
        self.conn.flush()?;
        Ok(())
    }

    /// Reads and deletes the answer property; deleting it also asks an `INCR` owner for the
    /// next chunk.
    fn take(&self) -> Result<GetPropertyReply, ClipboardError> {
        let reply = self
            .conn
            .get_property(
                true,
                self.window,
                self.property,
                AtomEnum::ANY,
                0,
                u32::MAX / 4,
            )?
            .reply()?;
        self.conn.flush()?;
        Ok(reply)
    }
}

/// Watches the clipboard of the X server `display` (`None`: `$DISPLAY`) and calls
/// `on_contents` with what every new clipboard owner offers, until it breaks: the text, or
/// [`Contents::Secret`] for a password manager's copy (see [`next`]), or [`Contents::Other`]
/// for owners without UTF-8 text (images, files). Invalid UTF-8 is replaced.
///
/// # Errors
/// When the X server cannot be reached, lacks `XFixes`, or the connection breaks.
pub fn watch_x11(
    display: Option<&str>,
    mut on_contents: impl FnMut(Contents) -> ControlFlow<()>,
) -> Result<(), ClipboardError> {
    let session = Session::open(display)?;
    // What was asked of the current owner, and when it took the clipboard.
    let mut asked: Option<(Want, u32)> = None;
    // Bytes of an INCR transfer in progress.
    let mut incoming: Option<Vec<u8>> = None;
    loop {
        // The complete answer to what was asked once it is in: whether the owner converted,
        // and the bytes.
        let answer: Option<(bool, Vec<u8>)> = match session.conn.wait_for_event()? {
            Event::XfixesSelectionNotify(event) => {
                incoming = None;
                asked = (event.owner != x11rb::NONE)
                    .then_some((Want::Targets, event.selection_timestamp));
                if let Some((want, time)) = asked {
                    session.ask(want, time)?;
                }
                None
            }
            Event::SelectionNotify(event) if event.property == x11rb::NONE => {
                Some((false, Vec::new()))
            }
            Event::SelectionNotify(event) if event.property == session.property => {
                let reply = session.take()?;
                if reply.type_ == session.incr {
                    incoming = Some(Vec::new());
                    None
                } else {
                    Some((true, reply.value))
                }
            }
            Event::PropertyNotify(event)
                if event.atom == session.property
                    && event.state == Property::NEW_VALUE
                    && incoming.is_some() =>
            {
                let reply = session.take()?;
                if reply.value.is_empty() {
                    // A zero-length chunk ends the transfer.
                    incoming.take().map(|bytes| (true, bytes))
                } else {
                    if let Some(buffer) = incoming.as_mut() {
                        buffer.extend_from_slice(&reply.value);
                    }
                    None
                }
            }
            _ => None,
        };
        let (Some((want, time)), Some((converted, bytes))) = (asked, answer) else {
            continue;
        };
        let atoms: Vec<u32>;
        let answer = match (converted, want) {
            (false, _) => Answer::Refused,
            (true, Want::Targets) => {
                // Atoms arrive in the connection's (native) byte order.
                atoms = bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|atom| u32::from_ne_bytes(*atom))
                    .collect();
                Answer::Targets(&atoms)
            }
            (true, Want::Text) => Answer::Text(&bytes),
        };
        asked = None;
        match next(want, answer, session.hint) {
            Next::Ask(want) => {
                asked = Some((want, time));
                session.ask(want, time)?;
            }
            Next::Done(contents) => {
                if on_contents(contents).is_break() {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HINT: u32 = 400;
    const UTF8: u32 = 300;

    #[test]
    fn a_copy_offering_the_password_manager_hint_is_a_secret_whose_text_is_never_asked() {
        assert_eq!(
            next(Want::Targets, Answer::Targets(&[1, UTF8, HINT]), HINT),
            Next::Done(Contents::Secret)
        );
    }

    #[test]
    fn other_copies_are_asked_for_their_text_then_read() {
        assert_eq!(
            next(Want::Targets, Answer::Targets(&[1, UTF8]), HINT),
            Next::Ask(Want::Text)
        );
        // An owner that cannot list its targets may still have text.
        assert_eq!(
            next(Want::Targets, Answer::Refused, HINT),
            Next::Ask(Want::Text)
        );
        assert_eq!(
            next(Want::Text, Answer::Text("café".as_bytes()), HINT),
            Next::Done(Contents::Text {
                text: "café".into(),
                source_app: None,
            })
        );
    }

    #[test]
    fn owners_without_text_give_other_contents() {
        assert_eq!(
            next(Want::Text, Answer::Refused, HINT),
            Next::Done(Contents::Other)
        );
    }
}
