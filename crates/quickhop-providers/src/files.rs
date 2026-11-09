//! Files and folders from the file index (`quickhop index`, `quickhop-index`), for queries
//! of at least [`MIN_QUERY_CHARS`] characters.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use quickhop_core::{Action, Cancel, Item, ItemId, Provider, ProviderError, Query};
use quickhop_index::{Hit, Index};

const NAME: &str = "files";

/// Shorter queries return nothing: every path would match.
pub const MIN_QUERY_CHARS: usize = 3;

/// Most files offered per query.
pub const MAX_FILES: usize = 50;

/// The file index, when one has been built.
pub struct FilesProvider {
    index: Option<Mutex<Index>>,
    home: Option<PathBuf>,
}

impl FilesProvider {
    /// Searches `index`; `home` is shown as `~` in subtitles.
    pub fn new(index: Option<Index>, home: Option<PathBuf>) -> Self {
        Self {
            index: index.map(Mutex::new),
            home,
        }
    }

    /// Opens the index at `path` when it exists (it is never created here); a broken file
    /// gives no items and a warning.
    pub fn open(path: &Path, home: Option<PathBuf>) -> (Self, Vec<String>) {
        if !path.is_file() {
            return (Self::new(None, home), Vec::new());
        }
        match Index::open(path) {
            Ok(index) => (Self::new(Some(index), home), Vec::new()),
            Err(error) => (Self::new(None, home), vec![error.to_string()]),
        }
    }

    /// The folder holding `path`, with the home folder shown as `~`.
    fn location(&self, path: &Path) -> String {
        let parent = path.parent().unwrap_or(path);
        if let Some(home) = &self.home
            && let Ok(rest) = parent.strip_prefix(home)
        {
            return if rest.as_os_str().is_empty() {
                "~".to_owned()
            } else {
                format!("~/{}", rest.display())
            };
        }
        parent.display().to_string()
    }

    /// An item that opens the file; further actions reveal it, copy its path and move it to
    /// the trash.
    fn item(&self, hit: Hit) -> Item {
        let path = hit.entry.path.to_string_lossy().into_owned();
        let mut item = Item::new(
            ItemId::new(NAME, path.clone()),
            hit.entry.name,
            Action::Open {
                target: path.clone(),
            },
        )
        .with_subtitle(self.location(&hit.entry.path))
        .with_keywords([path.clone()])
        .with_icon(if hit.entry.is_dir {
            "folder"
        } else {
            "text-x-generic"
        });
        item.actions.extend([
            Action::Reveal { path: path.clone() },
            Action::Copy { text: path.clone() },
            Action::Trash { path },
        ]);
        item
    }
}

impl Provider for FilesProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn items(&self, query: &Query, cancel: &Cancel) -> Result<Vec<Item>, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled {
                provider: NAME.into(),
            });
        }
        let Some(index) = &self.index else {
            return Ok(Vec::new());
        };
        if query.text().chars().count() < MIN_QUERY_CHARS {
            return Ok(Vec::new());
        }
        let hits = index
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .search(query.text(), MAX_FILES)
            .map_err(|error| ProviderError::Other {
                provider: NAME.into(),
                message: error.to_string(),
            })?;
        Ok(hits.into_iter().map(|hit| self.item(hit)).collect())
    }

    fn has_key(&self, key: &str) -> Result<bool, ProviderError> {
        let Some(index) = &self.index else {
            return Ok(false);
        };
        index
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(Path::new(key))
            .map_err(|error| ProviderError::Other {
                provider: NAME.into(),
                message: error.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitles_shorten_home() {
        let provider = FilesProvider::new(None, Some(PathBuf::from("/h")));
        assert_eq!(provider.location(Path::new("/h/a.txt")), "~");
        assert_eq!(provider.location(Path::new("/h/docs/a.txt")), "~/docs");
        assert_eq!(provider.location(Path::new("/srv/a.txt")), "/srv");
    }

    #[test]
    fn short_queries_and_missing_index_give_nothing() {
        let (provider, warnings) =
            FilesProvider::open(Path::new("/nonexistent/index.sqlite"), None);
        assert_eq!(warnings, Vec::<String>::new());
        let items = provider
            .items(&Query::new("report"), &Cancel::new())
            .unwrap();
        assert_eq!(items, Vec::<Item>::new());
        let index = FilesProvider::new(Some(Index::in_memory().unwrap()), None);
        assert_eq!(
            index.items(&Query::new("re"), &Cancel::new()).unwrap(),
            Vec::<Item>::new()
        );
    }
}
