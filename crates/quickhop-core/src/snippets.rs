//! The snippet library: `snippets.toml` next to `config.toml`, edited by hand or by the UI.
//!
//! ```toml
//! [[snippets]]
//! name = "Signature"
//! keyword = ";sig"
//! text = "Best,\n{argument name=\"who\" default=\"Ada\"}"
//! ```
//!
//! Texts may hold [`crate::placeholders`]; they are expanded when a snippet is used.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{ConfigError, config_path};

/// One snippet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snippet {
    /// Display name, unique ignoring case; `{snippet name="..."}` refers to it.
    pub name: String,
    /// The text, with placeholders.
    pub text: String,
    /// Optional word that finds it in search (and, later, expands it while typing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyword: Option<String>,
}

/// The whole `snippets.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SnippetsFile {
    /// Snippets, in display order.
    pub snippets: Vec<Snippet>,
}

impl SnippetsFile {
    /// Parses and validates TOML.
    ///
    /// # Errors
    /// On invalid TOML, unknown keys, empty or duplicate names, empty texts, or keywords
    /// with spaces.
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let file: Self = toml::from_str(text)?;
        let mut names = std::collections::HashSet::new();
        for snippet in &file.snippets {
            let name = snippet.name.trim();
            if name.is_empty() || snippet.text.is_empty() {
                return Err(ConfigError::Invalid(
                    "snippets need a name and a text".into(),
                ));
            }
            if !names.insert(name.to_lowercase()) {
                return Err(ConfigError::Invalid(format!(
                    "snippet {name:?} is defined twice"
                )));
            }
            if snippet
                .keyword
                .as_ref()
                .is_some_and(|k| k.is_empty() || k.contains(char::is_whitespace))
            {
                return Err(ConfigError::Invalid(format!(
                    "snippet {name:?}: keyword must be one word"
                )));
            }
        }
        Ok(file)
    }

    /// Reads `path`; a missing file is an empty library.
    ///
    /// # Errors
    /// When the file exists but cannot be read or [`SnippetsFile::from_toml`] fails.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// The snippet called `name`, ignoring case.
    pub fn find(&self, name: &str) -> Option<&Snippet> {
        let name = name.trim();
        self.snippets
            .iter()
            .find(|s| s.name.trim().eq_ignore_ascii_case(name))
    }
}

/// `snippets.toml` next to the config file, e.g. `~/.config/quickhop/snippets.toml`.
///
/// # Errors
/// As [`config_path`].
pub fn snippets_path(
    os: &str,
    var: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, ConfigError> {
    let config = config_path(os, var)?;
    Ok(config.parent().map_or_else(
        || PathBuf::from("snippets.toml"),
        |dir| dir.join("snippets.toml"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_finds_and_round_trips() {
        let text = "[[snippets]]\nname = \"Signature\"\nkeyword = \";sig\"\ntext = \"Best,\\n{clipboard}\"\n";
        let file = SnippetsFile::from_toml(text).unwrap();
        assert_eq!(file.find("signature").unwrap().text, "Best,\n{clipboard}");
        assert_eq!(file.find("nope"), None);
        let again = SnippetsFile::from_toml(&toml::to_string(&file).unwrap()).unwrap();
        assert_eq!(again, file);
    }

    #[test]
    fn rejects_bad_snippets() {
        for bad in [
            "[[snippets]]\nname = \" \"\ntext = \"x\"\n",
            "[[snippets]]\nname = \"a\"\ntext = \"\"\n",
            "[[snippets]]\nname = \"a\"\ntext = \"x\"\nkeyword = \"two words\"\n",
            "[[snippets]]\nname = \"A\"\ntext = \"x\"\n[[snippets]]\nname = \"a\"\ntext = \"y\"\n",
        ] {
            assert!(
                matches!(SnippetsFile::from_toml(bad), Err(ConfigError::Invalid(_))),
                "{bad}"
            );
        }
        assert!(matches!(
            SnippetsFile::from_toml("[[snippets]]\nname = \"a\"\ntext = \"x\"\ncolour = 1\n"),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn missing_file_is_empty_and_path_sits_next_to_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let file = SnippetsFile::load(&dir.path().join("snippets.toml")).unwrap();
        assert_eq!(file, SnippetsFile::default());
        let var = |name: &str| (name == "HOME").then(|| "/h".to_owned());
        assert_eq!(
            snippets_path("linux", var).unwrap(),
            Path::new("/h/.config/quickhop/snippets.toml")
        );
    }
}
