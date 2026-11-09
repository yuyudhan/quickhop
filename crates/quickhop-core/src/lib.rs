//! Platform-independent core of quickhop.
//!
//! A [`Search`] runs a [`Query`] against every registered [`Provider`], scores each returned
//! [`Item`] with the [`fuzzy`] matcher (nucleo), adds the frecency [`Boosts`] the caller
//! supplies, and returns the best matches in rank order. Nothing here does IO.
#![forbid(unsafe_code)]

pub mod clipboard;
pub mod config;
pub mod fuzzy;
pub mod item;
pub mod paste;
pub mod provider;
pub mod search;

pub use clipboard::{ClipboardSource, Contents, Copied, Watcher};
pub use config::{Apps, Clipboard, Config, ConfigError, Providers, Quicklink, Scripts};
pub use fuzzy::{Match, Matcher, score};
pub use item::{Action, Item, ItemId, ScriptArgument, ScriptArgumentKind, ScriptMode};
pub use provider::{Cancel, Provider, ProviderError, Query};
pub use search::{Boosts, Ranked, Search, SearchResults, Tier, rank};
