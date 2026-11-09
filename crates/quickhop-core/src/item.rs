//! Search results and the actions they offer.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable identity of an item within its provider, e.g. a desktop-file id.
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

    /// Parses the `provider:key` form written by [`fmt::Display`].
    pub fn parse(text: &str) -> Option<Self> {
        let (provider, key) = text.split_once(':')?;
        (!provider.is_empty() && !key.is_empty()).then(|| Self::new(provider, key))
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.provider, self.key)
    }
}

/// How a Raycast script command presents its output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScriptMode {
    /// Show the whole standard output.
    FullOutput,
    /// Show the last line of output as a short message.
    Compact,
    /// Close the launcher; show the last line as a notification.
    Silent,
    /// Show the first line of output as the item's subtitle, refreshed periodically.
    Inline,
}

/// Type of a script-command argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScriptArgumentKind {
    /// Free text.
    Text,
    /// Free text that is not echoed.
    Password,
    /// One value from a fixed list.
    Dropdown,
}

/// One argument a script command asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptArgument {
    /// Input kind.
    pub kind: ScriptArgumentKind,
    /// Prompt shown in the empty input.
    pub placeholder: String,
    /// Whether the argument may be left empty.
    pub optional: bool,
    /// Whether the value is percent-encoded before it is passed.
    pub percent_encoded: bool,
    /// Allowed values for [`ScriptArgumentKind::Dropdown`].
    pub choices: Vec<String>,
}

/// What happens when the user picks an item. Every action is plain data: describing it
/// never runs anything.
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
    /// Put text on the clipboard, then paste it into the app the user came from.
    Paste {
        /// Text to paste.
        text: String,
    },
    /// Start a program without a shell.
    Run {
        /// Program name or path.
        program: String,
        /// Arguments, passed verbatim.
        args: Vec<String>,
        /// Whether the user must confirm before it runs (power commands).
        confirm: bool,
    },
    /// Run a Raycast-compatible script command.
    Script {
        /// The script file.
        path: String,
        /// How its output is shown.
        mode: ScriptMode,
        /// Working directory; the script's directory when absent.
        cwd: Option<String>,
        /// Arguments the user is asked for, at most three.
        arguments: Vec<ScriptArgument>,
        /// Whether the user must confirm before it runs.
        confirm: bool,
    },
    /// Expand a snippet's placeholders and copy (later: paste) the result.
    Snippet {
        /// The snippet text, placeholders unexpanded (see [`crate::placeholders`]).
        text: String,
    },
    /// Show a file or folder selected in the file manager.
    Reveal {
        /// Absolute path.
        path: String,
    },
    /// Move a file or folder to the trash (always confirmed first).
    Trash {
        /// Absolute path.
        path: String,
    },
    /// Open a searchable list in the launcher window (the window searches it with the item
    /// as its source).
    View {
        /// Which list.
        view: View,
    },
}

/// A searchable list a command opens in the launcher window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum View {
    /// The clipboard history, newest first.
    Clipboard,
    /// Every snippet.
    Snippets,
    /// Files and folders by name.
    Files,
}

impl Action {
    /// Whether running this action needs an explicit confirmation first.
    pub fn needs_confirmation(&self) -> bool {
        match self {
            Self::Run { confirm, .. } | Self::Script { confirm, .. } => *confirm,
            Self::Trash { .. } => true,
            Self::Open { .. }
            | Self::Copy { .. }
            | Self::Paste { .. }
            | Self::Snippet { .. }
            | Self::Reveal { .. }
            | Self::View { .. } => false,
        }
    }

    /// The action's kind as serialized (`open`, `copy`, `reveal`, …), to pick it by name.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Open { .. } => "open",
            Self::Copy { .. } => "copy",
            Self::Paste { .. } => "paste",
            Self::Run { .. } => "run",
            Self::Script { .. } => "script",
            Self::Snippet { .. } => "snippet",
            Self::Reveal { .. } => "reveal",
            Self::Trash { .. } => "trash",
            Self::View { .. } => "view",
        }
    }
}

/// One entry a provider offers for the current query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// Identity, used to deduplicate and to remember choices.
    pub id: ItemId,
    /// Main text, matched against the query.
    pub title: String,
    /// Secondary text shown under the title: not matched.
    pub subtitle: Option<String>,
    /// Extra words matched against the query (generic names, keywords).
    pub keywords: Vec<String>,
    /// Exact shortcuts: an alias equal to the query, or to its first word, pins the item.
    pub aliases: Vec<String>,
    /// Icon file path, or a symbolic name when no file was found.
    pub icon: Option<String>,
    /// Actions in preference order: the first is the default.
    pub actions: Vec<Action>,
}

impl Item {
    /// Builds an item with a title and a default action, no subtitle, keywords or aliases.
    pub fn new(id: ItemId, title: impl Into<String>, action: Action) -> Self {
        Self {
            id,
            title: title.into(),
            subtitle: None,
            keywords: Vec::new(),
            aliases: Vec::new(),
            icon: None,
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

    /// Adds aliases.
    #[must_use]
    pub fn with_aliases<I, S>(mut self, aliases: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.aliases.extend(aliases.into_iter().map(Into::into));
        self
    }

    /// Sets the icon.
    #[must_use]
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
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

    fn run(program: &str) -> Action {
        Action::Run {
            program: program.into(),
            args: vec![],
            confirm: false,
        }
    }

    #[test]
    fn default_action_is_first() {
        let item = Item::new(ItemId::new("apps", "firefox"), "Firefox", run("firefox"));
        assert!(
            matches!(item.default_action(), Some(Action::Run { program, .. }) if program == "firefox")
        );
    }

    #[test]
    fn action_serializes_tagged() {
        let text = toml::to_string(&Action::Copy { text: "42".into() }).unwrap();
        assert!(text.contains("kind = \"copy\""), "{text}");
    }

    #[test]
    fn id_round_trips_through_display() {
        let id = ItemId::new("apps", "org.mozilla.firefox.desktop");
        assert_eq!(ItemId::parse(&id.to_string()), Some(id));
        assert_eq!(ItemId::parse("nokey:"), None);
        assert_eq!(ItemId::parse("plain"), None);
    }

    #[test]
    fn confirmation_only_for_flagged_commands() {
        assert!(!run("ls").needs_confirmation());
        let reboot = Action::Run {
            program: "systemctl".into(),
            args: vec!["reboot".into()],
            confirm: true,
        };
        assert!(reboot.needs_confirmation());
        assert!(!Action::Open { target: "x".into() }.needs_confirmation());
    }
}
