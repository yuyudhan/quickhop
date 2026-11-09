//! The quickhop launcher window: a search field over ranked [`quickhop_core::Ranked`] results.
//!
//! The toolkit is gpui (`docs/adr/0001-ui-toolkit.md`). [`model`] is the toolkit-free view
//! model the window renders: query, results, selection, navigation stack, action panel and
//! toasts.

#![forbid(unsafe_code)]

pub mod model;
