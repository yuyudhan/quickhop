//! Platform-independent core of quickhop.
//!
//! A search runs a [`Query`] against every registered [`Provider`], scores each returned
//! [`Item`] with the fuzzy matcher in [`fuzzy`] and returns the best matches in rank order.

#![forbid(unsafe_code)]

pub mod config;
pub mod fuzzy;
pub mod item;
pub mod provider;
pub mod search;

pub use config::{Config, ConfigError};
pub use fuzzy::{Match, score};
pub use item::{Action, Item, ItemId};
pub use provider::{Provider, ProviderError, Query};
pub use search::{Ranked, Search, SearchResults};
