//! Search results and the actions they offer.

use serde::{Deserialize, Serialize};

/// Stable identity of an item within its provider, e.g. a desktop-file path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ItemId {
    /// Name of the provider that produced the item.
    pub provider: String,
    /// Provider-local key.
    pub key: String,
}

impl ItemId {
    /// Builds an id from a provider name and a provider-local key.
    pub fn new(provider: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            key: key.into(),
        }
    }
}

/// What happens when the user picks an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Action {
    /// Open a file, directory or URL with the system handler.
    Open {
        /// Path or URL to open.
        target: String,
    },
    /// Put text on the clipboard.
    Copy {
        /// Text to copy.
        text: String,
    },
    /// Start a program without a shell.
    Run {
        /// Program name or path.
        program: String,
        /// Arguments, passed verbatim.
        args: Vec<String>,
    },
}

/// One entry a provider offers for the current query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// Identity, used to deduplicate and to remember choices.
    pub id: ItemId,
    /// Main text, matched against the query.
    pub title: String,
    /// Secondary text shown under the title; not matched.
    pub subtitle: Option<String>,
    /// Extra words matched against the query (aliases, binary names).
    pub keywords: Vec<String>,
    /// Actions in preference order; the first is the default.
    pub actions: Vec<Action>,
}

impl Item {
    /// Builds an item with a title and a default action, no subtitle or keywords.
    pub fn new(id: ItemId, title: impl Into<String>, action: Action) -> Self {
        Self {
            id,
            title: title.into(),
            subtitle: None,
            keywords: Vec::new(),
            actions: vec![action],
        }
    }

    /// Sets the subtitle.
    #[must_use]
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Adds keywords.
    #[must_use]
    pub fn with_keywords<I, S>(mut self, keywords: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.keywords.extend(keywords.into_iter().map(Into::into));
        self
    }

    /// The default action, if the item has any.
    pub fn default_action(&self) -> Option<&Action> {
        self.actions.first()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_action_is_first() {
        let item = Item::new(
            ItemId::new("apps", "firefox"),
            "Firefox",
            Action::Run {
                program: "firefox".into(),
                args: vec![],
            },
        );
        assert!(
            matches!(item.default_action(), Some(Action::Run { program, .. }) if program == "firefox")
        );
    }

    #[test]
    fn action_serializes_tagged() {
        let text = toml::to_string(&Action::Copy { text: "42".into() }).unwrap();
        assert!(text.contains("kind = \"copy\""), "{text}");
    }
}
