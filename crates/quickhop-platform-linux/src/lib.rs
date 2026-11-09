//! Linux desktop integration for quickhop. For now: watching the X11 clipboard
//! ([`clipboard::watch_x11`]) and pressing Ctrl+V through `XTest` ([`keyboard::press_ctrl_v`]);
//! on Wayland the caller uses `wl-paste --watch` and `wtype`.
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
// Everything so far goes through x11rb's safe API.
#![forbid(unsafe_code)]

pub mod clipboard;
pub mod keyboard;
