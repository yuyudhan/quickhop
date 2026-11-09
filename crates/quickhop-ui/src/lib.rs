//! The quickhop launcher window: a search field over ranked [`quickhop_core::Ranked`] results.
//!
//! The toolkit is gpui (`docs/adr/0001-ui-toolkit.md`). [`model`] is the toolkit-free view
//! model the window renders: query, results, selection, navigation stack, action panel and
//! toasts; [`onboarding`] is the one of the onboarding window, which asks for the permission
//! pasting needs. [`window`] is the window itself, on macOS for now.

#![forbid(unsafe_code)]

pub mod ansi;
pub mod model;
pub mod onboarding;
#[cfg(target_os = "macos")]
mod onboarding_window;
#[cfg(target_os = "macos")]
pub mod window;
