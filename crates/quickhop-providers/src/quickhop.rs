//! quickhop's own commands, found in the search as Raycast lists its own: quickhop Settings,
//! Reload quickhop config, About quickhop and Quit quickhop ([`Action::Quickhop`]). Each item
//! only names the command; the launcher window, the daemon or `quickhop launch` carries it
//! out.

use quickhop_core::{
    Action, Cancel, Item, ItemId, Provider, ProviderError, Query, QuickhopCommand,
};

/// Provider name.
pub const NAME: &str = "quickhop";

/// The id of `command`'s item: `quickhop:settings`, `quickhop:reload`, …
pub fn id(command: QuickhopCommand) -> ItemId {
    ItemId::new(NAME, command.key())
}

/// The command the item `id` runs, if it is one of these.
pub fn command(id: &ItemId) -> Option<QuickhopCommand> {
    if id.provider != NAME {
        return None;
    }
    QuickhopCommand::from_key(&id.key)
}

/// The item of `command`; `version` is shown by About quickhop.
pub fn item(command: QuickhopCommand, version: &str) -> Item {
    let (title, subtitle, icon, keywords): (&str, String, &str, &[&str]) = match command {
        QuickhopCommand::Settings => (
            "quickhop Settings",
            "Hotkey, theme, menu-bar icon, launch at login, providers".into(),
            "⚙️",
            &[
                "settings",
                "preferences",
                "prefs",
                "config",
                "configuration",
                "options",
                "hotkey",
                "theme",
            ],
        ),
        QuickhopCommand::Reload => (
            "Reload quickhop config",
            "Read config.toml and snippets.toml again".into(),
            "🔄",
            &["reload", "config", "refresh", "settings"],
        ),
        QuickhopCommand::About => (
            "About quickhop",
            format!("Version {version}"),
            "ℹ️",
            &["about", "version", "info"],
        ),
        QuickhopCommand::Quit => (
            "Quit quickhop",
            "Stop quickhop: the hotkey, the window and the menu-bar icon".into(),
            "⏻",
            &["quit", "exit", "stop", "close"],
        ),
    };
    Item::new(id(command), title, Action::Quickhop { command })
        .with_subtitle(subtitle)
        .with_icon(icon)
        .with_keywords(keywords.iter().copied().chain(["quickhop"]))
}

/// quickhop's own commands, always listed (they have no `[providers]` switch).
pub struct QuickhopProvider {
    items: Vec<Item>,
}

impl QuickhopProvider {
    /// The commands, About quickhop naming `version`.
    pub fn new(version: &str) -> Self {
        Self {
            items: QuickhopCommand::ALL
                .into_iter()
                .map(|command| item(command, version))
                .collect(),
        }
    }
}

impl Provider for QuickhopProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn items(&self, _query: &Query, cancel: &Cancel) -> Result<Vec<Item>, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled {
                provider: NAME.into(),
            });
        }
        Ok(self.items.clone())
    }
}

#[cfg(test)]
mod tests {
    use quickhop_core::{Boosts, Search};

    use super::*;

    fn top(query: &str) -> Vec<String> {
        let search = Search::new(8).with_provider(Box::new(QuickhopProvider::new("1.2.3")));
        search
            .run(&Query::new(query), &Cancel::new(), &Boosts::new())
            .ranked
            .into_iter()
            .map(|ranked| ranked.item.title)
            .collect()
    }

    #[test]
    fn settings_and_preferences_find_the_settings_first() {
        for query in [
            "settings",
            "preferences",
            "prefs",
            "sett",
            "config",
            "hotkey",
        ] {
            assert_eq!(
                top(query).first().map(String::as_str),
                Some("quickhop Settings"),
                "{query}"
            );
        }
    }

    #[test]
    fn quickhop_lists_every_command() {
        let found = top("quickhop");
        for title in [
            "quickhop Settings",
            "Reload quickhop config",
            "About quickhop",
            "Quit quickhop",
        ] {
            assert!(found.iter().any(|t| t == title), "{title} in {found:?}");
        }
    }

    #[test]
    fn each_command_answers_its_own_words() {
        assert_eq!(
            top("reload").first().map(String::as_str),
            Some("Reload quickhop config")
        );
        assert_eq!(
            top("about").first().map(String::as_str),
            Some("About quickhop")
        );
        assert_eq!(
            top("quit").first().map(String::as_str),
            Some("Quit quickhop")
        );
        assert!(top("firefox").is_empty(), "nothing unrelated matches");
    }

    #[test]
    fn ids_round_trip_and_about_names_the_version() {
        for command in QuickhopCommand::ALL {
            let item = item(command, "1.2.3");
            assert_eq!(super::command(&item.id), Some(command));
            assert_eq!(item.default_action(), Some(&Action::Quickhop { command }));
        }
        assert_eq!(
            item(QuickhopCommand::About, "1.2.3").subtitle.as_deref(),
            Some("Version 1.2.3")
        );
        assert_eq!(super::command(&ItemId::new("apps", "settings")), None);
    }
}
