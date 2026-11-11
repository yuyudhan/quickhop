//! The quickhop launcher window: a search field over ranked [`quickhop_core::Ranked`] results.
//!
//! The toolkit is gpui (`docs/adr/0001-ui-toolkit.md`). [`model`] is the toolkit-free view
//! model the window renders: query, results, selection, navigation stack, action panel and
//! toasts; [`edit`] is how the bar's deleting keys edit its text; [`onboarding`] is the view
//! model of the onboarding window, which asks for the permission pasting needs; [`settings`]
//! is the view model of the settings window; [`theme`] holds the colours of the `theme`
//! setting. [`window`] is the window itself, on macOS for now.

#![forbid(unsafe_code)]

pub mod ansi;
pub mod edit;
pub mod model;
pub mod onboarding;
#[cfg(target_os = "macos")]
pub mod onboarding_window;
pub mod settings;
#[cfg(target_os = "macos")]
pub mod settings_window;
pub mod theme;
#[cfg(target_os = "macos")]
pub mod window;
