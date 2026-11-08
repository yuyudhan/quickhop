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
    /// No location for the config file could be derived from the environment.
    #[error("cannot locate the config directory: set QUICKHOP_CONFIG or {0}")]
    NoConfigDir(&'static str),
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
    /// Directories scanned (not recursively) for `# @raycast.*` script commands. None by
    /// default.
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
            if link
                .alias
                .as_ref()
                .is_some_and(|a| a.is_empty() || a.contains(char::is_whitespace))
            {
                return Err(ConfigError::Invalid(format!(
                    "quicklink {}: alias must be one word",
                    link.name
                )));
            }
        }
        Ok(())
    }
}

/// The config file path for `os` (a [`std::env::consts::OS`] value), looking variables up
/// with `var`. `QUICKHOP_CONFIG` overrides everything; otherwise Linux and other Unixes use
/// `$XDG_CONFIG_HOME` or `~/.config`, macOS `~/Library/Application Support` and Windows
/// `%APPDATA%`.
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
        "macos" => get("HOME")
            .ok_or(ConfigError::NoConfigDir("HOME"))?
            .join("Library/Application Support"),
        _ => match get("XDG_CONFIG_HOME") {
            Some(dir) => dir,
            None => get("HOME")
                .ok_or(ConfigError::NoConfigDir("HOME"))?
                .join(".config"),
        },
    };
    Ok(dir.join("quickhop").join("config.toml"))
}

/// The data directory (frecency database) for `os`, looking variables up with `var`.
/// `QUICKHOP_DATA` overrides everything; otherwise Linux and other Unixes use
/// `$XDG_DATA_HOME` or `~/.local/share`, macOS `~/Library/Application Support` and
/// Windows `%LOCALAPPDATA%`, each with a `quickhop` subdirectory.
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
        "macos" => get("HOME")
            .ok_or(ConfigError::NoConfigDir("HOME"))?
            .join("Library/Application Support"),
        _ => match get("XDG_DATA_HOME") {
            Some(dir) => dir,
            None => get("HOME")
                .ok_or(ConfigError::NoConfigDir("HOME"))?
                .join(".local/share"),
        },
    };
    Ok(dir.join("quickhop"))
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
            Path::new("/h/Library/Application Support/quickhop/config.toml")
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
            Path::new("/h/Library/Application Support/quickhop")
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
}
