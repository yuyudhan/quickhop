//! The provider interface every source of items implements.

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
    /// Any other provider-specific failure.
    #[error("{provider}: {message}")]
    Other {
        /// Provider name.
        provider: String,
        /// Human-readable cause.
        message: String,
    },
}

/// A source of items: applications, files, a calculator, commands.
pub trait Provider {
    /// Short unique name, used in [`crate::ItemId::provider`] and in config.
    fn name(&self) -> &str;

    /// Candidate items for `query`. Ranking is done by the caller, so a provider may return
    /// every item it has; providers that compute results (a calculator) return only those.
    ///
    /// # Errors
    /// When the provider's source cannot be read.
    fn items(&self, query: &Query) -> Result<Vec<Item>, ProviderError>;

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
}
