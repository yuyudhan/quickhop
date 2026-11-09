//! The commands that open a list in the launcher window ([`Action::View`]): Clipboard History,
//! Search Snippets and Search Files, each offered while what it lists is enabled. The window
//! searches the list itself; on the command line these commands only say so. The clipboard
//! history's entries are made here too ([`clipboard_items`]), so every OS tests them.

use quickhop_core::{Action, Cancel, Config, Item, ItemId, Provider, ProviderError, Query, View};
use quickhop_store::state::Clip;

/// Provider name.
pub const NAME: &str = "views";

/// The list commands enabled in a configuration.
pub struct ViewsProvider {
    items: Vec<Item>,
}

impl ViewsProvider {
    /// The commands of the lists `config` enables: the clipboard history when `[clipboard]
    /// enabled`, snippets and files while their providers are on.
    pub fn new(config: &Config) -> Self {
        let enabled = [
            (View::Clipboard, config.clipboard.enabled),
            (View::Snippets, config.providers.snippets),
            (View::Files, config.providers.files),
        ];
        Self {
            items: enabled
                .into_iter()
                .filter(|(_, on)| *on)
                .map(|(view, _)| command(view))
                .collect(),
        }
    }
}

/// The id of the command that opens `view`: `views:clipboard`, `views:snippets`,
/// `views:files`.
pub fn id(view: View) -> ItemId {
    ItemId::new(NAME, key(view))
}

/// The view the command `id` opens, if it is one of these commands.
pub fn view(id: &ItemId) -> Option<View> {
    if id.provider != NAME {
        return None;
    }
    [View::Clipboard, View::Snippets, View::Files]
        .into_iter()
        .find(|view| key(*view) == id.key)
}

fn key(view: View) -> &'static str {
    match view {
        View::Clipboard => "clipboard",
        View::Snippets => "snippets",
        View::Files => "files",
    }
}

fn command(view: View) -> Item {
    let (title, subtitle, icon, keywords): (&str, &str, &str, &[&str]) = match view {
        View::Clipboard => (
            "Clipboard History",
            "Search what you copied, paste it again",
            "📋",
            &["clipboard", "history", "paste", "copied"],
        ),
        View::Snippets => (
            "Search Snippets",
            "Every snippet in snippets.toml",
            "📝",
            &["snippets", "text", "expand"],
        ),
        View::Files => (
            "Search Files",
            "Files and folders by name",
            "🔎",
            &["files", "folders", "find", "documents"],
        ),
    };
    Item::new(id(view), title, Action::View { view })
        .with_subtitle(subtitle)
        .with_icon(icon)
        .with_keywords(keywords.iter().copied())
}

/// The first line of `text`, at most 100 characters, with `…` when anything was cut: how a
/// clipboard entry is listed.
pub fn clip_title(text: &str) -> String {
    let first = text.trim_start().lines().next().unwrap_or_default();
    let mut line: String = first.chars().take(100).collect();
    if line.len() < text.trim().len() {
        line.push('…');
    }
    line
}

/// The clipboard history as the launcher window lists it, in the order given (newest
/// first): `clipboard:<id>` items titled by [`clip_title`], found by any part of the text,
/// under the app it was copied from, pasted by default or copied.
pub fn clipboard_items(clips: Vec<Clip>) -> Vec<Item> {
    clips
        .into_iter()
        .map(|clip| {
            let title = clip_title(&clip.text);
            let whole = title != clip.text.trim();
            let mut item = Item::new(
                ItemId::new("clipboard", clip.id.to_string()),
                title,
                Action::Paste {
                    text: clip.text.clone(),
                },
            );
            if let Some(app) = clip.source_app {
                item = item.with_subtitle(app);
            }
            if whole {
                item = item.with_keywords([clip.text.clone()]);
            }
            item.actions.push(Action::Copy { text: clip.text });
            item
        })
        .collect()
}

impl Provider for ViewsProvider {
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
    use super::*;

    fn titles(provider: &ViewsProvider) -> Vec<String> {
        provider
            .items(&Query::default(), &Cancel::new())
            .unwrap()
            .into_iter()
            .map(|item| item.title)
            .collect()
    }

    #[test]
    fn each_list_is_offered_only_while_it_is_enabled() {
        let mut config = Config::default();
        assert_eq!(
            titles(&ViewsProvider::new(&config)),
            ["Clipboard History", "Search Snippets", "Search Files"]
        );
        config.clipboard.enabled = false;
        config.providers.files = false;
        assert_eq!(titles(&ViewsProvider::new(&config)), ["Search Snippets"]);
        config.providers.snippets = false;
        assert_eq!(titles(&ViewsProvider::new(&config)), Vec::<String>::new());
    }

    #[test]
    fn every_command_opens_its_own_view_under_its_id() {
        let items = ViewsProvider::new(&Config::default())
            .items(&Query::default(), &Cancel::new())
            .unwrap();
        for (item, view) in items
            .iter()
            .zip([View::Clipboard, View::Snippets, View::Files])
        {
            assert_eq!(item.id, id(view));
            assert_eq!(super::view(&item.id), Some(view));
            assert_eq!(item.actions, [Action::View { view }]);
        }
    }

    #[test]
    fn other_ids_are_no_views() {
        for id in ["views:history", "clipboard:clipboard", "snippets:files"] {
            assert_eq!(view(&ItemId::parse(id).unwrap()), None, "{id}");
        }
    }

    #[test]
    fn clip_titles_cut_long_and_multi_line_texts() {
        assert_eq!(clip_title("  hello\n"), "hello");
        assert_eq!(clip_title("first\nsecond"), "first…");
        assert_eq!(
            clip_title(&"é".repeat(150)),
            format!("{}…", "é".repeat(100))
        );
    }

    fn clip(id: i64, text: &str, app: Option<&str>) -> Clip {
        Clip {
            id,
            text: text.into(),
            source_app: app.map(Into::into),
            first_copied: 1,
            last_copied: 2,
            copies: 1,
        }
    }

    #[test]
    fn clipboard_entries_paste_by_default_and_keep_their_order() {
        let items = clipboard_items(vec![
            clip(7, "ssh deploy@10.0.0.7", Some("com.apple.Terminal")),
            clip(3, "first line\nsecond line", None),
        ]);
        let ids: Vec<String> = items.iter().map(|item| item.id.to_string()).collect();
        assert_eq!(ids, ["clipboard:7", "clipboard:3"]);
        assert_eq!(items[0].title, "ssh deploy@10.0.0.7");
        assert_eq!(items[0].subtitle.as_deref(), Some("com.apple.Terminal"));
        assert_eq!(
            items[0].actions,
            [
                Action::Paste {
                    text: "ssh deploy@10.0.0.7".into()
                },
                Action::Copy {
                    text: "ssh deploy@10.0.0.7".into()
                },
            ]
        );
        assert!(items[0].keywords.is_empty(), "the title is the whole text");
        assert_eq!(items[1].title, "first line…");
        assert_eq!(items[1].subtitle, None);
        assert_eq!(
            items[1].keywords,
            ["first line\nsecond line"],
            "found by its second line too"
        );
    }

    #[test]
    fn the_whole_text_ranks_entries_and_an_empty_query_keeps_newest_first() {
        let items = clipboard_items(vec![
            clip(3, "alpha\nfind me here", None),
            clip(2, "beta", None),
            clip(1, "gamma", None),
        ]);
        let rank = |text: &str| -> Vec<String> {
            quickhop_core::rank(
                &Query::new(text),
                items.iter().cloned().map(|item| (item, false)),
                &quickhop_core::Boosts::new(),
                10,
            )
            .into_iter()
            .map(|ranked| ranked.item.id.key)
            .collect()
        };
        assert_eq!(rank(""), ["3", "2", "1"]);
        assert_eq!(rank("find me"), ["3"]);
        assert_eq!(rank("gamm"), ["1"]);
    }
}
