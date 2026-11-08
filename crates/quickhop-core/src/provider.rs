//! The provider interface every source of items implements.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::item::Item;

/// The user's input, as typed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    text: String,
}

impl Query {
    /// Wraps the typed text.
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    /// The text with surrounding whitespace removed.
    pub fn text(&self) -> &str {
        self.text.trim()
    }

    /// Whether nothing but whitespace was typed.
    pub fn is_empty(&self) -> bool {
        self.text().is_empty()
    }

    /// The first word, lowercased: what aliases are compared with.
    pub fn head(&self) -> String {
        self.text()
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_lowercase()
    }

    /// Everything after the first word, trimmed: the argument of an alias.
    pub fn rest(&self) -> &str {
        let text = self.text();
        text.find(char::is_whitespace)
            .map_or("", |at| text[at..].trim())
    }
}

/// Shared flag that tells running providers to stop: set when a newer query supersedes
/// the current one. Clones share the flag.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// A fresh, unset flag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks every holder to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether [`Cancel::cancel`] was called.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Why a provider could not answer a query.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Reading the provider's source failed.
    #[error("{provider}: {source}")]
    Io {
        /// Provider name.
        provider: String,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The query was cancelled before the provider finished.
    #[error("{provider}: cancelled")]
    Cancelled {
        /// Provider name.
        provider: String,
    },
    /// Any other provider-specific failure.
    #[error("{provider}: {message}")]
    Other {
        /// Provider name.
        provider: String,
        /// Human-readable cause.
        message: String,
    },
}

/// A source of items: applications, a calculator, commands.
///
/// Providers load their index when they are built and answer [`Provider::items`] from
/// memory, so a query stays within the time budget the contract tests enforce.
pub trait Provider: Send + Sync {
    /// Short unique name, used in [`crate::ItemId::provider`] and in config.
    fn name(&self) -> &str;

    /// Candidate items for `query`. Ranking is done by the caller, so a provider may return
    /// every item it has; providers that compute results (a calculator) return only those.
    /// Long-running providers check `cancel` and stop early.
    ///
    /// # Errors
    /// When the provider's source cannot be read, or with [`ProviderError::Cancelled`].
    fn items(&self, query: &Query, cancel: &Cancel) -> Result<Vec<Item>, ProviderError>;

    /// Whether the provider's items bypass fuzzy filtering (computed answers such as a
    /// calculator result, which never contain the query text literally).
    fn is_computed(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_trims() {
        assert_eq!(Query::new("  fire ").text(), "fire");
        assert!(Query::new("   ").is_empty());
    }

    #[test]
    fn head_and_rest_split_the_first_word() {
        let query = Query::new("  G  rust  lang ");
        assert_eq!(query.head(), "g");
        assert_eq!(query.rest(), "rust  lang");
        assert_eq!(Query::new("solo").rest(), "");
    }

    #[test]
    fn cancel_is_shared_between_clones() {
        let cancel = Cancel::new();
        let other = cancel.clone();
        assert!(!other.is_cancelled());
        cancel.cancel();
        assert!(other.is_cancelled());
    }
}
