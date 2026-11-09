//! quickhop's macOS platform layer: the `AppKit` calls the rest of the workspace needs, behind
//! safe functions and traits. Together with `quickhop-platform-linux` it is the only place
//! `unsafe` is allowed; every unsafe operation sits in its own block with a `// SAFETY:` note.
//!
//! The crate compiles on every OS: the `AppKit` code is `cfg`-gated, and elsewhere each entry
//! point reports [`Unsupported`].

#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]

pub mod icon;
#[cfg(target_os = "macos")]
pub mod pasteboard;

/// The operation needs macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("this needs macOS")]
pub struct Unsupported;
