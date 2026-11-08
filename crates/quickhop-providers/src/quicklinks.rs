//! Quicklinks: URL templates from `[[quicklinks]]`. Typing the alias (or the one-word
//! name) followed by text pins the link with `{query}` replaced by that text,
//! percent-encoded; otherwise the link matches by name and opens with `{query}` empty.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use quickhop_core::{Action, Cancel, Item, ItemId, Provider, ProviderError, Query, Quicklink};

/// Provider name.
pub const NAME: &str = "quicklinks";

/// Placeholder replaced with the argument.
pub const PLACEHOLDER: &str = "{query}";

/// RFC 3986 unreserved characters stay as they are.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// `template` with every `{query}` replaced by `argument`, percent-encoded.
pub fn expand(template: &str, argument: &str) -> String {
    template.replace(
        PLACEHOLDER,
        &utf8_percent_encode(argument, COMPONENT).to_string(),
    )
}

/// The configured links.
pub struct QuicklinksProvider {
    links: Vec<Quicklink>,
}

impl QuicklinksProvider {
    /// Links in display order.
    pub fn new(links: Vec<Quicklink>) -> Self {
        Self { links }
    }
}

fn triggers(link: &Quicklink) -> Vec<String> {
    let mut triggers: Vec<String> = link.alias.iter().map(|a| a.to_lowercase()).collect();
    if !link.name.contains(char::is_whitespace) {
        triggers.push(link.name.to_lowercase());
    }
    triggers
}

impl Provider for QuicklinksProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn items(&self, query: &Query, cancel: &Cancel) -> Result<Vec<Item>, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled {
                provider: NAME.into(),
            });
        }
        let head = query.head();
        Ok(self
            .links
            .iter()
            .map(|link| {
                let triggers = triggers(link);
                let takes_argument = link.url.contains(PLACEHOLDER);
                let argument = if takes_argument && triggers.contains(&head) {
                    query.rest()
                } else {
                    ""
                };
                let target = expand(&link.url, argument);
                let title = if argument.is_empty() {
                    link.name.clone()
                } else {
                    format!("{}: {argument}", link.name)
                };
                let mut item = Item::new(
                    ItemId::new(NAME, link.name.to_lowercase()),
                    title,
                    Action::Open {
                        target: target.clone(),
                    },
                )
                .with_subtitle(target)
                .with_keywords(link.alias.iter().cloned());
                if !argument.is_empty() || !takes_argument {
                    item = item.with_aliases(triggers);
                }
                item
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn google() -> Quicklink {
        Quicklink {
            name: "Google".into(),
            url: "https://www.google.com/search?q={query}".into(),
            alias: Some("g".into()),
        }
    }

    fn items(text: &str) -> Vec<Item> {
        QuicklinksProvider::new(vec![google()])
            .items(&Query::new(text), &Cancel::new())
            .unwrap()
    }

    #[test]
    fn alias_fills_the_template() {
        let item = &items("g rust & c++")[0];
        assert_eq!(item.title, "Google: rust & c++");
        assert_eq!(
            item.default_action(),
            Some(&Action::Open {
                target: "https://www.google.com/search?q=rust%20%26%20c%2B%2B".into()
            })
        );
        assert_eq!(item.aliases, vec!["g", "google"]);
        assert_eq!(items("google x")[0].title, "Google: x");
    }

    #[test]
    fn without_alias_the_link_opens_empty() {
        let item = &items("goo")[0];
        assert_eq!(item.title, "Google");
        assert_eq!(item.aliases.len(), 0);
        assert_eq!(
            item.subtitle.as_deref(),
            Some("https://www.google.com/search?q=")
        );
    }

    #[test]
    fn unreserved_characters_stay() {
        assert_eq!(
            expand("x/{query}/{query}", "a-b_c.d~é"),
            "x/a-b_c.d~%C3%A9/a-b_c.d~%C3%A9"
        );
    }
}
