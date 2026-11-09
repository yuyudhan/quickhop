//! Snippets from `snippets.toml`: one item per snippet, found by name and keyword, whose
//! action expands the placeholders and copies the text.

use std::path::Path;

use quickhop_core::snippets::SnippetsFile;
use quickhop_core::{Action, Cancel, Item, ItemId, Provider, ProviderError, Query};

/// Provider name.
pub const NAME: &str = "snippets";

/// Characters of the snippet text shown as the subtitle.
const PREVIEW_CHARS: usize = 60;

/// The snippet library.
pub struct SnippetsProvider {
    items: Vec<Item>,
}

/// The first line of `text`, shortened, with `…` when anything was left out.
fn preview(text: &str) -> String {
    let first = text.lines().next().unwrap_or_default().trim_end();
    let mut shown: String = first.chars().take(PREVIEW_CHARS).collect();
    if shown.len() < text.trim_end().len() {
        shown.push('…');
    }
    shown
}

impl SnippetsProvider {
    /// Items for every snippet in `file`, in file order.
    pub fn new(file: &SnippetsFile) -> Self {
        let items = file
            .snippets
            .iter()
            .map(|snippet| {
                let name = snippet.name.trim();
                Item::new(
                    ItemId::new(NAME, name.to_lowercase()),
                    name,
                    Action::Snippet {
                        text: snippet.text.clone(),
                    },
                )
                .with_subtitle(preview(&snippet.text))
                .with_keywords(snippet.keyword.iter().cloned())
            })
            .collect();
        Self { items }
    }

    /// Loads `path` (a missing file is an empty library); a broken file gives no items and
    /// a warning.
    pub fn load(path: &Path) -> (Self, Vec<String>) {
        match SnippetsFile::load(path) {
            Ok(file) => (Self::new(&file), Vec::new()),
            Err(error) => (
                Self { items: Vec::new() },
                vec![format!("{}: {error}", path.display())],
            ),
        }
    }
}

impl Provider for SnippetsProvider {
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

    #[test]
    fn previews_the_first_line() {
        assert_eq!(preview("Best,\nAda"), "Best,…");
        assert_eq!(preview("one line"), "one line");
        assert_eq!(preview(&"x".repeat(70)).chars().count(), PREVIEW_CHARS + 1);
    }

    #[test]
    fn broken_file_warns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snippets.toml");
        std::fs::write(&path, "[[snippets]]\nname = 1\n").unwrap();
        let (provider, warnings) = SnippetsProvider::load(&path);
        assert_eq!(provider.items, Vec::<Item>::new());
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }
}
