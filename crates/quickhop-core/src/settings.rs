//! The changes the settings window makes to `config.toml` (CFG-01): each [`Setting`] sets one
//! key and keeps everything else in the file as the user wrote it (comments, order, quicklinks,
//! per-command settings). A change that would leave an invalid file is refused before
//! anything is written.

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table, value};

use crate::config::{Config, ConfigError, Theme};
use crate::hotkey::Hotkey;

/// The `[providers]` switches, in the order the settings list them, with their labels.
pub const PROVIDERS: [(&str, &str); 8] = [
    ("apps", "Applications"),
    ("calc", "Calculator"),
    ("emoji", "Emoji"),
    ("system", "System commands"),
    ("quicklinks", "Quicklinks"),
    ("scripts", "Script commands"),
    ("snippets", "Snippets"),
    ("files", "Files"),
];

/// One setting changed in the settings window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Setting {
    /// `hotkey`: the global shortcut that shows the launcher.
    Hotkey(Hotkey),
    /// `max_results`.
    MaxResults(usize),
    /// `theme`.
    Theme(Theme),
    /// `menu_bar_icon`.
    MenuBarIcon(bool),
    /// A `[providers]` switch, named as in [`PROVIDERS`].
    Provider(String, bool),
    /// `clipboard.enabled`.
    ClipboardHistory(bool),
    /// `files.hidden`.
    HiddenFiles(bool),
}

impl Setting {
    /// The key it sets, as a TOML path (`providers.apps`).
    pub fn key(&self) -> String {
        match self {
            Self::Hotkey(_) => "hotkey".into(),
            Self::MaxResults(_) => "max_results".into(),
            Self::Theme(_) => "theme".into(),
            Self::MenuBarIcon(_) => "menu_bar_icon".into(),
            Self::Provider(name, _) => format!("providers.{name}"),
            Self::ClipboardHistory(_) => "clipboard.enabled".into(),
            Self::HiddenFiles(_) => "files.hidden".into(),
        }
    }

    /// The setting's value on `config`: whether `config` already has it.
    pub fn is_set_in(&self, config: &Config) -> bool {
        match self {
            Self::Hotkey(hotkey) => Hotkey::parse(&config.hotkey).ok() == Some(*hotkey),
            Self::MaxResults(count) => config.max_results == *count,
            Self::Theme(theme) => config.theme == *theme,
            Self::MenuBarIcon(on) => config.menu_bar_icon == *on,
            Self::Provider(name, on) => provider(config, name) == Some(*on),
            Self::ClipboardHistory(on) => config.clipboard.enabled == *on,
            Self::HiddenFiles(on) => config.files.hidden == *on,
        }
    }
}

/// Whether the `[providers]` switch `name` is on in `config`; `None` for an unknown name.
pub fn provider(config: &Config, name: &str) -> Option<bool> {
    let providers = &config.providers;
    Some(match name {
        "apps" => providers.apps,
        "calc" => providers.calc,
        "emoji" => providers.emoji,
        "system" => providers.system,
        "quicklinks" => providers.quicklinks,
        "scripts" => providers.scripts,
        "snippets" => providers.snippets,
        "files" => providers.files,
        _ => return None,
    })
}

/// The table `name` of `document`, created when missing; `None` when `name` holds something
/// else than a table (the file would not load then anyway).
fn table<'a>(
    document: &'a mut DocumentMut,
    name: &str,
) -> Option<&'a mut dyn toml_edit::TableLike> {
    let item = document
        .as_table_mut()
        .entry(name)
        .or_insert_with(|| Item::Table(Table::new()));
    item.as_table_like_mut()
}

/// `text`, a `config.toml`, with `setting` applied; the rest of the text is kept as written.
///
/// # Errors
/// When `text` is not TOML, or the result is not a valid configuration (an unknown provider,
/// a hotkey another command uses, a value out of range, or a problem the file had before).
pub fn apply(text: &str, setting: &Setting) -> Result<String, ConfigError> {
    let mut document: DocumentMut = text
        .parse()
        .map_err(|error: toml_edit::TomlError| ConfigError::Invalid(error.to_string()))?;
    let (section, key, new) = match setting {
        Setting::Hotkey(hotkey) => (None, "hotkey", value(hotkey.to_string())),
        Setting::MaxResults(count) => {
            let count = i64::try_from(*count).map_err(|_| {
                ConfigError::Invalid(format!("max_results {count} is out of range"))
            })?;
            (None, "max_results", value(count))
        }
        Setting::Theme(theme) => (None, "theme", value(theme.as_str())),
        Setting::MenuBarIcon(on) => (None, "menu_bar_icon", value(*on)),
        Setting::Provider(name, on) => {
            if !PROVIDERS.iter().any(|(known, _)| known == name) {
                return Err(ConfigError::Invalid(format!("unknown provider {name:?}")));
            }
            (Some("providers"), name.as_str(), value(*on))
        }
        Setting::ClipboardHistory(on) => (Some("clipboard"), "enabled", value(*on)),
        Setting::HiddenFiles(on) => (Some("files"), "hidden", value(*on)),
    };
    // A file holding only comments keeps them all in the document's trailing text, which is
    // written after every key: they move in front of the first key instead, where they were.
    let leading = if section.is_none() && document.as_table().is_empty() {
        let trailing = document.trailing().as_str().unwrap_or_default().to_owned();
        document.set_trailing("");
        trailing
    } else {
        String::new()
    };
    let target = match section {
        None => document.as_table_mut() as &mut dyn toml_edit::TableLike,
        Some(name) => table(&mut document, name)
            .ok_or_else(|| ConfigError::Invalid(format!("{name} must be a table")))?,
    };
    match target.get_mut(key) {
        // Replacing only the value keeps the key, the comments above it and the one after it
        // on the same line.
        Some(Item::Value(old)) => {
            let decor = old.decor().clone();
            if let Item::Value(mut fresh) = new {
                *fresh.decor_mut() = decor;
                *old = fresh;
            }
        }
        Some(other) => *other = new,
        None => {
            target.insert(key, new);
            if !leading.is_empty()
                && let Some(mut inserted) = target.key_mut(key)
            {
                inserted.leaf_decor_mut().set_prefix(leading);
            }
        }
    }
    let edited = document.to_string();
    Config::from_toml(&edited)?;
    Ok(edited)
}

/// The file a write to `path` changes: the file a symlink points to (a config kept in a
/// dotfiles repository stays a link), else `path` itself.
fn write_target(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Applies `setting` to the config file at `path` (a missing file starts empty) and returns
/// the configuration it holds afterwards. The file is replaced in one step, so a running
/// daemon never reads half of it.
///
/// # Errors
/// As [`apply`]; when the file cannot be read or written.
pub fn save(path: &Path, setting: &Setting) -> Result<Config, ConfigError> {
    let target = write_target(path);
    let text = match std::fs::read_to_string(&target) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(ConfigError::Read {
                path: target,
                source,
            });
        }
    };
    let edited = apply(&text, setting)?;
    let write_error = |source| ConfigError::Write {
        path: target.clone(),
        source,
    };
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(write_error)?;
    let name = target.file_name().map_or_else(
        || "config.toml".into(),
        |name| name.to_string_lossy().into_owned(),
    );
    let temporary = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, &edited).map_err(write_error)?;
    if let Err(source) = std::fs::rename(&temporary, &target) {
        let _ = std::fs::remove_file(&temporary);
        return Err(write_error(source));
    }
    Config::from_toml(&edited)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hotkey(text: &str) -> Hotkey {
        Hotkey::parse(text).unwrap()
    }

    #[test]
    fn an_empty_file_gets_just_the_key() {
        let text = apply("", &Setting::Theme(Theme::Light)).unwrap();
        assert_eq!(text, "theme = \"light\"\n");
        assert_eq!(Config::from_toml(&text).unwrap().theme, Theme::Light);
    }

    #[test]
    fn comments_order_and_other_keys_are_kept() {
        let before = "\
# my quickhop
hotkey = \"Alt+Space\" # muscle memory
max_results = 8

[providers]
emoji = false # never

[[quicklinks]]
name = \"GitHub\"
url = \"https://github.com/search?q={query}\"
";
        let text = apply(before, &Setting::Hotkey(hotkey("ctrl+shift+k"))).unwrap();
        assert_eq!(
            text,
            before.replace("\"Alt+Space\"", "\"Ctrl+Shift+K\""),
            "only the value changes, its comment stays"
        );
        let text = apply(&text, &Setting::Provider("calc".into(), false)).unwrap();
        assert!(
            text.contains("emoji = false # never\ncalc = false\n"),
            "{text}"
        );
        let config = Config::from_toml(&text).unwrap();
        assert!(!config.providers.calc && !config.providers.emoji);
        assert_eq!(config.quicklinks.len(), 1);
    }

    #[test]
    fn missing_tables_are_created() {
        let text = apply("max_results = 5\n", &Setting::ClipboardHistory(false)).unwrap();
        let text = apply(&text, &Setting::HiddenFiles(true)).unwrap();
        let config = Config::from_toml(&text).unwrap();
        assert!(!config.clipboard.enabled);
        assert!(config.files.hidden);
        assert_eq!(config.max_results, 5);
    }

    #[test]
    fn every_setting_lands_on_its_key() {
        let settings = [
            Setting::Hotkey(hotkey("F13")),
            Setting::MaxResults(12),
            Setting::Theme(Theme::Dark),
            Setting::MenuBarIcon(false),
            Setting::Provider("files".into(), false),
            Setting::ClipboardHistory(false),
            Setting::HiddenFiles(true),
        ];
        let mut text = String::new();
        for setting in &settings {
            text = apply(&text, setting).unwrap();
        }
        let config = Config::from_toml(&text).unwrap();
        for setting in &settings {
            assert!(
                setting.is_set_in(&config),
                "{} not set: {text}",
                setting.key()
            );
        }
    }

    #[test]
    fn invalid_results_are_refused() {
        assert!(matches!(
            apply("", &Setting::MaxResults(0)),
            Err(ConfigError::Invalid(_))
        ));
        assert!(apply("", &Setting::Provider("nope".into(), true)).is_err());
        assert!(apply("not toml = = 1", &Setting::MenuBarIcon(false)).is_err());
        let taken = "[[commands]]\nid = \"system:lock\"\nhotkey = \"Ctrl+Alt+L\"\n";
        let error = apply(taken, &Setting::Hotkey(hotkey("Ctrl+Alt+L"))).unwrap_err();
        assert!(error.to_string().contains("already the hotkey"), "{error}");
    }

    #[test]
    fn save_writes_the_file_and_returns_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quickhop/config.toml");
        let config = save(&path, &Setting::MenuBarIcon(false)).unwrap();
        assert!(!config.menu_bar_icon);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "menu_bar_icon = false\n"
        );
        let config = save(&path, &Setting::MaxResults(3)).unwrap();
        assert_eq!((config.max_results, config.menu_bar_icon), (3, false));
        let left: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from("config.toml")]);
    }

    #[test]
    fn save_refuses_without_touching_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "max_results = 4\n").unwrap();
        assert!(save(&path, &Setting::MaxResults(1000)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "max_results = 4\n");
    }

    #[cfg(unix)]
    #[test]
    fn save_keeps_a_symlinked_config_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles.toml");
        std::fs::write(&real, "# kept in git\n").unwrap();
        let link = dir.path().join("config.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        save(&link, &Setting::Theme(Theme::Dark)).unwrap();
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(
            std::fs::read_to_string(&real).unwrap(),
            "# kept in git\ntheme = \"dark\"\n"
        );
    }
}
