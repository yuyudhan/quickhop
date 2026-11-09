//! The user configuration file (TOML) and where it lives.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Largest accepted `max_results`.
pub const MAX_RESULTS_LIMIT: usize = 100;

/// Why a configuration could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid quickhop TOML.
    #[error("invalid config: {0}")]
    Parse(#[from] toml::de::Error),
    /// A value is out of range.
    #[error("invalid config: {0}")]
    Invalid(String),
    /// `[[commands]]` ids that name no item of a loaded, non-computed provider.
    #[error("invalid config: unknown command id {}", .0.join(", "))]
    UnknownCommands(Vec<String>),
    /// No location for the config file could be derived from the environment.
    #[error("cannot locate the config directory: set QUICKHOP_CONFIG or {0}")]
    NoConfigDir(&'static str),
    /// A file from the legacy macOS location could not be moved.
    #[error("cannot move {from} to {to}: {source}")]
    Migrate {
        /// The legacy file.
        from: PathBuf,
        /// Its new location.
        to: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Which built-in providers are enabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Providers {
    /// Installed applications.
    pub apps: bool,
    /// Calculator and unit conversion.
    pub calc: bool,
    /// Emoji picker.
    pub emoji: bool,
    /// System commands (lock, log out, suspend, reboot, shut down).
    pub system: bool,
    /// Quicklinks from `[[quicklinks]]`.
    pub quicklinks: bool,
    /// Raycast-compatible script commands from `[scripts] dirs`.
    pub scripts: bool,
    /// Snippets from `snippets.toml` next to the config file.
    pub snippets: bool,
}

impl Default for Providers {
    fn default() -> Self {
        Self {
            apps: true,
            calc: true,
            emoji: true,
            system: true,
            quicklinks: true,
            scripts: true,
            snippets: true,
        }
    }
}

/// Where applications are found.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Apps {
    /// Directories to scan instead of the OS defaults (XDG `applications` dirs, the
    /// macOS Applications folders, the Windows Start Menu). Empty means the defaults.
    pub dirs: Vec<PathBuf>,
    /// Icon theme looked up first on Linux; empty means `hicolor` only.
    pub icon_theme: String,
}

/// Where script commands are found.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scripts {
    /// Directories scanned (not recursively) for `# @raycast.*` script commands, after
    /// `scripts/` next to the config file (see [`scripts_dir`]). A leading `~` means the
    /// home directory.
    pub dirs: Vec<PathBuf>,
}

/// A URL template opened with the query, e.g. a web search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quicklink {
    /// Display name, e.g. `Google`.
    pub name: String,
    /// URL or path; every `{query}` is replaced with the percent-encoded argument.
    pub url: String,
    /// Optional single-word shortcut: typing `alias text` pins this link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

/// Per-command settings from a `[[commands]]` table: any item of a non-computed provider,
/// named by its id as printed by `quickhop query` (`provider:key`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSettings {
    /// The item, `provider:key`, e.g. `apps:org.mozilla.firefox.desktop`.
    pub id: String,
    /// Single-word shortcut: typing it (alone or as the first word) pins the item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// `false` removes the item from every search.
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// Global hotkey that runs the item directly, e.g. `Ctrl+Alt+T`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<String>,
    /// Position in the empty root search: lower first, before items without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<u32>,
}

fn enabled() -> bool {
    true
}

fn is_one_word(text: &str) -> bool {
    !text.is_empty() && !text.contains(char::is_whitespace)
}

/// The whole configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Global hotkey that opens the launcher, e.g. `Alt+Space`.
    pub hotkey: String,
    /// Number of results shown, 1 to [`MAX_RESULTS_LIMIT`].
    pub max_results: usize,
    /// Enabled providers.
    pub providers: Providers,
    /// Application sources.
    pub apps: Apps,
    /// Script-command sources.
    pub scripts: Scripts,
    /// Quicklinks, in display order.
    pub quicklinks: Vec<Quicklink>,
    /// Per-command aliases, switches, hotkeys and root order.
    pub commands: Vec<CommandSettings>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: "Alt+Space".into(),
            max_results: 8,
            providers: Providers::default(),
            apps: Apps::default(),
            scripts: Scripts::default(),
            quicklinks: Vec::new(),
            commands: Vec::new(),
        }
    }
}

impl Config {
    /// Parses and validates TOML. Missing keys take their defaults.
    ///
    /// # Errors
    /// On invalid TOML, unknown keys or out-of-range values.
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    /// Reads, parses and validates the file at `path`.
    ///
    /// # Errors
    /// When the file cannot be read or [`Config::from_toml`] fails.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml(&text)
    }

    /// The default configuration as TOML.
    pub fn default_toml() -> String {
        // Serializing plain structs of strings, integers and booleans cannot fail.
        toml::to_string(&Self::default()).unwrap_or_default()
    }

    /// Checks value ranges.
    ///
    /// # Errors
    /// When a value is out of range.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.hotkey.trim().is_empty() {
            return Err(ConfigError::Invalid("hotkey must not be empty".into()));
        }
        if !(1..=MAX_RESULTS_LIMIT).contains(&self.max_results) {
            return Err(ConfigError::Invalid(format!(
                "max_results must be between 1 and {MAX_RESULTS_LIMIT}, got {}",
                self.max_results
            )));
        }
        for link in &self.quicklinks {
            if link.name.trim().is_empty() || link.url.trim().is_empty() {
                return Err(ConfigError::Invalid(
                    "quicklinks need a name and a url".into(),
                ));
            }
            if link.alias.as_ref().is_some_and(|a| !is_one_word(a)) {
                return Err(ConfigError::Invalid(format!(
                    "quicklink {}: alias must be one word",
                    link.name
                )));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for command in &self.commands {
            let id = &command.id;
            if crate::ItemId::parse(id).is_none() {
                return Err(ConfigError::Invalid(format!(
                    "command id {id:?} must be provider:key"
                )));
            }
            if !seen.insert(id) {
                return Err(ConfigError::Invalid(format!(
                    "command {id} is listed twice"
                )));
            }
            if command.alias.as_ref().is_some_and(|a| !is_one_word(a)) {
                return Err(ConfigError::Invalid(format!(
                    "command {id}: alias must be one word"
                )));
            }
            if command.hotkey.as_ref().is_some_and(|h| h.trim().is_empty()) {
                return Err(ConfigError::Invalid(format!(
                    "command {id}: hotkey must not be empty"
                )));
            }
        }
        Ok(())
    }
}

/// The XDG base directory `xdg` (e.g. `XDG_CONFIG_HOME`) or `~/<fallback>` when it is unset.
fn xdg_dir(
    get: impl Fn(&str) -> Option<PathBuf>,
    xdg: &str,
    fallback: &str,
) -> Result<PathBuf, ConfigError> {
    match get(xdg) {
        Some(dir) => Ok(dir),
        None => Ok(get("HOME")
            .ok_or(ConfigError::NoConfigDir("HOME"))?
            .join(fallback)),
    }
}

/// The config file path for `os` (a [`std::env::consts::OS`] value), looking variables up
/// with `var`. `QUICKHOP_CONFIG` overrides everything; otherwise Linux, macOS and other
/// Unixes use `$XDG_CONFIG_HOME` or `~/.config`, Windows `%APPDATA%`.
///
/// # Errors
/// When the needed variables are unset or empty.
pub fn config_path(os: &str, var: impl Fn(&str) -> Option<String>) -> Result<PathBuf, ConfigError> {
    let get = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(path) = get("QUICKHOP_CONFIG") {
        return Ok(path);
    }
    let dir = match os {
        "windows" => get("APPDATA").ok_or(ConfigError::NoConfigDir("APPDATA"))?,
        _ => xdg_dir(get, "XDG_CONFIG_HOME", ".config")?,
    };
    Ok(dir.join("quickhop").join("config.toml"))
}

/// The default script-command directory: `scripts/` next to the config file, e.g.
/// `~/.config/quickhop/scripts`.
///
/// # Errors
/// As [`config_path`].
pub fn scripts_dir(os: &str, var: impl Fn(&str) -> Option<String>) -> Result<PathBuf, ConfigError> {
    let config = config_path(os, var)?;
    Ok(config
        .parent()
        .map_or_else(|| PathBuf::from("scripts"), |dir| dir.join("scripts")))
}

/// The data directory (frecency database) for `os`, looking variables up with `var`.
/// `QUICKHOP_DATA` overrides everything; otherwise Linux, macOS and other Unixes use
/// `$XDG_DATA_HOME` or `~/.local/share`, Windows `%LOCALAPPDATA%`, each with a `quickhop`
/// subdirectory.
///
/// # Errors
/// When the needed variables are unset or empty.
pub fn data_dir(os: &str, var: impl Fn(&str) -> Option<String>) -> Result<PathBuf, ConfigError> {
    let get = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(path) = get("QUICKHOP_DATA") {
        return Ok(path);
    }
    let dir = match os {
        "windows" => get("LOCALAPPDATA").ok_or(ConfigError::NoConfigDir("LOCALAPPDATA"))?,
        _ => xdg_dir(get, "XDG_DATA_HOME", ".local/share")?,
    };
    Ok(dir.join("quickhop"))
}

/// The directory quickhop 0.1 used on macOS for both the config and the data:
/// `~/Library/Application Support/quickhop`. `None` on other OSes or without `HOME`.
pub fn legacy_macos_dir(os: &str, var: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if os != "macos" {
        return None;
    }
    var("HOME")
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join("Library/Application Support/quickhop"))
}

/// Files of the frecency database, moved together.
const DATA_FILES: [&str; 4] = [
    "frecency.sqlite",
    "frecency.sqlite-wal",
    "frecency.sqlite-shm",
    "frecency.sqlite-journal",
];

fn move_file(from: &Path, to: &Path) -> Result<(), ConfigError> {
    let error = |source| ConfigError::Migrate {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    };
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(error)?;
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // Rename fails across file systems: copy, then remove the original.
    std::fs::copy(from, to).map_err(error)?;
    std::fs::remove_file(from).map_err(error)
}

/// Moves the config file and the frecency database out of `legacy` (see
/// [`legacy_macos_dir`]) to `config_file` and `data_dir`, once: a file moves only when it
/// exists in `legacy` and its new location is still free. `None` skips that part (used when
/// `QUICKHOP_CONFIG` or `QUICKHOP_DATA` overrides the location). Returns one line per
/// moved file.
///
/// # Errors
/// When a file cannot be moved; files moved before stay moved.
pub fn migrate_legacy(
    legacy: &Path,
    config_file: Option<&Path>,
    data_dir: Option<&Path>,
) -> Result<Vec<String>, ConfigError> {
    let mut planned = Vec::new();
    if let Some(to) = config_file {
        planned.push((legacy.join("config.toml"), to.to_path_buf()));
    }
    if let Some(dir) = data_dir
        && !dir.join(DATA_FILES[0]).exists()
    {
        planned.extend(DATA_FILES.map(|name| (legacy.join(name), dir.join(name))));
    }
    let mut report = Vec::new();
    for (from, to) in planned {
        if from.is_file() && !to.exists() {
            move_file(&from, &to)?;
            report.push(format!("moved {} to {}", from.display(), to.display()));
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn default_round_trips() {
        assert_eq!(
            Config::from_toml(&Config::default_toml()).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn partial_file_takes_defaults() {
        let config = Config::from_toml("max_results = 3\n[providers]\nemoji = false\n").unwrap();
        assert_eq!(config.max_results, 3);
        assert!(!config.providers.emoji && config.providers.apps);
    }

    #[test]
    fn rejects_unknown_and_out_of_range() {
        assert!(matches!(
            Config::from_toml("colour = 1"),
            Err(ConfigError::Parse(_))
        ));
        assert!(matches!(
            Config::from_toml("max_results = 0"),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            Config::from_toml("hotkey = \" \""),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn paths_per_os() {
        let home = env(&[("HOME", "/h")]);
        assert_eq!(
            config_path("linux", &home).unwrap(),
            Path::new("/h/.config/quickhop/config.toml")
        );
        assert_eq!(
            config_path("linux", env(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "/x")])).unwrap(),
            Path::new("/x/quickhop/config.toml")
        );
        assert_eq!(
            config_path("macos", &home).unwrap(),
            Path::new("/h/.config/quickhop/config.toml")
        );
        assert_eq!(
            config_path("macos", env(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "/x")])).unwrap(),
            Path::new("/x/quickhop/config.toml")
        );
        assert_eq!(
            scripts_dir("macos", &home).unwrap(),
            Path::new("/h/.config/quickhop/scripts")
        );
        assert_eq!(
            scripts_dir("linux", env(&[("QUICKHOP_CONFIG", "/c/q.toml")])).unwrap(),
            Path::new("/c/scripts")
        );
        assert_eq!(
            config_path("windows", env(&[("APPDATA", "C:/a")])).unwrap(),
            Path::new("C:/a/quickhop/config.toml")
        );
        assert_eq!(
            config_path("windows", env(&[("QUICKHOP_CONFIG", "/c.toml")])).unwrap(),
            Path::new("/c.toml")
        );
        assert!(matches!(
            config_path("windows", &home),
            Err(ConfigError::NoConfigDir("APPDATA"))
        ));
    }

    #[test]
    fn quicklinks_parse_and_validate() {
        let config = Config::from_toml(
            "[[quicklinks]]\nname = \"Google\"\nurl = \"https://google.com/search?q={query}\"\nalias = \"g\"\n",
        )
        .unwrap();
        assert_eq!(config.quicklinks[0].alias.as_deref(), Some("g"));
        assert!(Config::from_toml(&toml::to_string(&config).unwrap()).is_ok());
        let bad = "[[quicklinks]]\nname = \"G\"\nurl = \"u\"\nalias = \"two words\"\n";
        assert!(matches!(
            Config::from_toml(bad),
            Err(ConfigError::Invalid(_))
        ));
        let unnamed = "[[quicklinks]]\nname = \" \"\nurl = \"u\"\n";
        assert!(matches!(
            Config::from_toml(unnamed),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn commands_parse_and_validate() {
        let text = "[[commands]]\nid = \"apps:firefox.desktop\"\nalias = \"ff\"\nhotkey = \"Ctrl+Alt+F\"\norder = 2\n\n\
                    [[commands]]\nid = \"system:lock\"\nenabled = false\n";
        let config = Config::from_toml(text).unwrap();
        assert_eq!(config.commands[0].alias.as_deref(), Some("ff"));
        assert!(config.commands[0].enabled);
        assert_eq!(config.commands[0].order, Some(2));
        assert!(!config.commands[1].enabled);
        assert_eq!(
            Config::from_toml(&toml::to_string(&config).unwrap()).unwrap(),
            config
        );
        for bad in [
            "[[commands]]\nid = \"no-provider\"\n",
            "[[commands]]\nid = \"apps:x\"\nalias = \"two words\"\n",
            "[[commands]]\nid = \"apps:x\"\nhotkey = \" \"\n",
            "[[commands]]\nid = \"apps:x\"\n[[commands]]\nid = \"apps:x\"\n",
        ] {
            assert!(
                matches!(Config::from_toml(bad), Err(ConfigError::Invalid(_))),
                "{bad}"
            );
        }
        assert!(matches!(
            Config::from_toml("[[commands]]\nid = \"apps:x\"\ncolour = 1\n"),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn data_dirs_per_os() {
        let home = env(&[("HOME", "/h")]);
        assert_eq!(
            data_dir("linux", &home).unwrap(),
            Path::new("/h/.local/share/quickhop")
        );
        assert_eq!(
            data_dir("linux", env(&[("XDG_DATA_HOME", "/d")])).unwrap(),
            Path::new("/d/quickhop")
        );
        assert_eq!(
            data_dir("macos", &home).unwrap(),
            Path::new("/h/.local/share/quickhop")
        );
        assert_eq!(
            data_dir("windows", env(&[("LOCALAPPDATA", "C:/l")])).unwrap(),
            Path::new("C:/l/quickhop")
        );
        assert_eq!(
            data_dir("linux", env(&[("QUICKHOP_DATA", "/q")])).unwrap(),
            Path::new("/q")
        );
        assert!(matches!(
            data_dir("windows", &home),
            Err(ConfigError::NoConfigDir("LOCALAPPDATA"))
        ));
    }

    #[test]
    fn legacy_dir_is_macos_only() {
        let home = env(&[("HOME", "/h")]);
        assert_eq!(
            legacy_macos_dir("macos", &home).unwrap(),
            Path::new("/h/Library/Application Support/quickhop")
        );
        assert_eq!(legacy_macos_dir("linux", &home), None);
        assert_eq!(legacy_macos_dir("macos", env(&[])), None);
    }

    #[test]
    fn migrates_legacy_files_once() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("legacy");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("config.toml"), "max_results = 3\n").unwrap();
        std::fs::write(legacy.join("frecency.sqlite"), "db").unwrap();
        std::fs::write(legacy.join("frecency.sqlite-wal"), "wal").unwrap();
        let config = root.path().join("config/quickhop/config.toml");
        let data = root.path().join("data/quickhop");

        let moved = migrate_legacy(&legacy, Some(&config), Some(&data)).unwrap();
        assert_eq!(moved.len(), 3, "{moved:?}");
        assert_eq!(Config::load(&config).unwrap().max_results, 3);
        assert_eq!(
            std::fs::read_to_string(data.join("frecency.sqlite")).unwrap(),
            "db"
        );
        assert!(data.join("frecency.sqlite-wal").is_file());
        assert!(!legacy.join("config.toml").exists());

        // A second run, or a legacy file reappearing, never overwrites the new location.
        std::fs::write(legacy.join("config.toml"), "max_results = 9\n").unwrap();
        let none: Vec<String> = Vec::new();
        assert_eq!(
            migrate_legacy(&legacy, Some(&config), Some(&data)).unwrap(),
            none
        );
        assert_eq!(Config::load(&config).unwrap().max_results, 3);
    }

    #[test]
    fn migration_skips_overridden_locations() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("legacy");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("config.toml"), "").unwrap();
        std::fs::write(legacy.join("frecency.sqlite"), "db").unwrap();
        let none: Vec<String> = Vec::new();
        assert_eq!(migrate_legacy(&legacy, None, None).unwrap(), none);
        assert!(legacy.join("config.toml").is_file());
        let missing = root.path().join("nothing-here");
        let config = root.path().join("c.toml");
        assert_eq!(migrate_legacy(&missing, Some(&config), None).unwrap(), none);
    }
}
