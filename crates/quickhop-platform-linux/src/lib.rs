//! Linux desktop integration for quickhop. For now: watching the X11 clipboard
//! ([`clipboard::watch_x11`]); Wayland compositors with the data-control protocol are watched
//! through `wl-paste --watch` by the caller.
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
// Everything so far goes through x11rb's safe API.
#![forbid(unsafe_code)]

pub mod clipboard;
