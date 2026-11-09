//! Platform-independent core of quickhop.
//!
//! A [`Search`] runs a [`Query`] against every registered [`Provider`], scores each returned
//! [`Item`] with the [`fuzzy`] matcher (nucleo), adds the frecency [`Boosts`] the caller
//! supplies, and returns the best matches in rank order. Nothing here does IO.
#![forbid(unsafe_code)]

pub mod config;
pub mod datefmt;
pub mod fuzzy;
pub mod item;
pub mod placeholders;
pub mod provider;
pub mod search;
pub mod snippets;

pub use config::{Apps, CommandSettings, Config, ConfigError, Providers, Quicklink, Scripts};
pub use fuzzy::{Match, Matcher, score};
pub use item::{Action, Item, ItemId, ScriptArgument, ScriptArgumentKind, ScriptMode};
pub use provider::{Cancel, Provider, ProviderError, Query};
pub use search::{Boosts, Ranked, Search, SearchResults, Tier, rank};
