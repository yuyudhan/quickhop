//! The user configuration file (TOML) and where it lives.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Flavour, Hotkey};

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
    /// Files and folders by name: Spotlight on macOS, else the index (`quickhop index`).
    pub files: bool,
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
            files: true,
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
    /// Icon theme looked up first on Linux; empty means the GTK setting
    /// (`gtk-icon-theme-name`), else `hicolor` only.
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

/// What the file index covers.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Files {
    /// Folders indexed; empty means the home folder. A leading `~` means the home folder.
    pub roots: Vec<PathBuf>,
    /// Index hidden files and folders (names starting with `.`).
    pub hidden: bool,
    /// More folder or file names to skip, besides version control, dependency, build and
    /// cache folders.
    pub exclude: Vec<String>,
}

/// Most clipboard entries `max_entries` may keep.
pub const MAX_CLIPBOARD_ENTRIES: usize = 100_000;

/// Clipboard history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Clipboard {
    /// Whether copied text is recorded.
    pub enabled: bool,
    /// Most entries kept, 1 to [`MAX_CLIPBOARD_ENTRIES`]; the oldest go first.
    pub max_entries: usize,
    /// Entries not copied for this many days are dropped; 0 keeps them forever.
    pub max_age_days: u32,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: 1000,
            max_age_days: 90,
        }
    }
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
    /// File-index sources.
    pub files: Files,
    /// Clipboard history.
    pub clipboard: Clipboard,
    /// Quicklinks, in display order.
    pub quicklinks: Vec<Quicklink>,
    /// Per-command aliases, switches, hotkeys and root order.
    pub commands: Vec<CommandSettings>,
}

impl Default for Config {
    /// The defaults of this build's [`Flavour`].
    fn default() -> Self {
        Self::defaults(Flavour::CURRENT)
    }
}

impl Config {
    /// The defaults of `flavour`: they differ only in the hotkey (see
    /// [`Flavour::default_hotkey`]).
    #[must_use]
    pub fn defaults(flavour: Flavour) -> Self {
        Self {
            hotkey: flavour.default_hotkey().into(),
            max_results: 8,
            providers: Providers::default(),
            apps: Apps::default(),
            scripts: Scripts::default(),
            files: Files::default(),
            clipboard: Clipboard::default(),
            quicklinks: Vec::new(),
            commands: Vec::new(),
        }
    }

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
        let launcher = Hotkey::parse(&self.hotkey)
            .map_err(|error| ConfigError::Invalid(format!("hotkey: {error}")))?;
        if !(1..=MAX_RESULTS_LIMIT).contains(&self.max_results) {
            return Err(ConfigError::Invalid(format!(
                "max_results must be between 1 and {MAX_RESULTS_LIMIT}, got {}",
                self.max_results
            )));
        }

        if !(1..=MAX_CLIPBOARD_ENTRIES).contains(&self.clipboard.max_entries) {
            return Err(ConfigError::Invalid(format!(
                "clipboard.max_entries must be between 1 and {MAX_CLIPBOARD_ENTRIES}, got {}",
                self.clipboard.max_entries
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
        let mut hotkeys = std::collections::HashMap::from([(launcher, "the launcher".to_owned())]);
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
            if let Some(text) = &command.hotkey {
                let hotkey = Hotkey::parse(text)
                    .map_err(|error| ConfigError::Invalid(format!("command {id}: {error}")))?;
                if let Some(other) = hotkeys.insert(hotkey, format!("command {id}")) {
                    return Err(ConfigError::Invalid(format!(
                        "command {id}: hotkey {hotkey} is already the hotkey of {other}"
                    )));
                }
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

/// The config file path of `flavour` for `os` (a [`std::env::consts::OS`] value), looking
/// variables up with `var`. `QUICKHOP_CONFIG` overrides everything; otherwise Linux, macOS
/// and other Unixes use `$XDG_CONFIG_HOME` or `~/.config`, Windows `%APPDATA%`, each with a
/// [`Flavour::stem`] subdirectory (`quickhop`, or `quickhop-dev` for a development build).
///
/// # Errors
/// When the needed variables are unset or empty.
pub fn config_path(
    flavour: Flavour,
    os: &str,
    var: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, ConfigError> {
    let get = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(path) = get("QUICKHOP_CONFIG") {
        return Ok(path);
    }
    let dir = match os {
        "windows" => get("APPDATA").ok_or(ConfigError::NoConfigDir("APPDATA"))?,
        _ => xdg_dir(get, "XDG_CONFIG_HOME", ".config")?,
    };
    Ok(dir.join(flavour.stem()).join("config.toml"))
}

/// The default script-command directory: `scripts/` next to the config file, e.g.
/// `~/.config/quickhop/scripts`.
///
/// # Errors
/// As [`config_path`].
pub fn scripts_dir(
    flavour: Flavour,
    os: &str,
    var: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, ConfigError> {
    let config = config_path(flavour, os, var)?;
    Ok(config
        .parent()
        .map_or_else(|| PathBuf::from("scripts"), |dir| dir.join("scripts")))
}

/// The data directory (frecency database, clipboard history, file index) of `flavour` for
/// `os`, looking variables up with `var`. `QUICKHOP_DATA` overrides everything; otherwise
/// Linux, macOS and other Unixes use `$XDG_DATA_HOME` or `~/.local/share`, Windows
/// `%LOCALAPPDATA%`, each with a [`Flavour::stem`] subdirectory.
///
/// # Errors
/// When the needed variables are unset or empty.
pub fn data_dir(
    flavour: Flavour,
    os: &str,
    var: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, ConfigError> {
    let get = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(path) = get("QUICKHOP_DATA") {
        return Ok(path);
    }
    let dir = match os {
        "windows" => get("LOCALAPPDATA").ok_or(ConfigError::NoConfigDir("LOCALAPPDATA"))?,
        _ => xdg_dir(get, "XDG_DATA_HOME", ".local/share")?,
    };
    Ok(dir.join(flavour.stem()))
}

/// The directory for the daemon's socket and lock file of `flavour`: `QUICKHOP_RUNTIME` if
/// set, else `$XDG_RUNTIME_DIR/<stem>`, else `<stem>-<user>` in `$TMPDIR` (per user on
/// macOS) or `/tmp`, with the [`Flavour::stem`] `quickhop` or `quickhop-dev`: the two
/// flavours run separate daemons.
pub fn runtime_dir(flavour: Flavour, var: impl Fn(&str) -> Option<String>) -> PathBuf {
    let get = |name: &str| var(name).filter(|v| !v.is_empty());
    if let Some(path) = get("QUICKHOP_RUNTIME") {
        return PathBuf::from(path);
    }
    let stem = flavour.stem();
    if let Some(dir) = get("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join(stem);
    }
    let user = get("USER").unwrap_or_else(|| "user".into());
    PathBuf::from(get("TMPDIR").unwrap_or_else(|| "/tmp".into())).join(format!("{stem}-{user}"))
}

/// The directory quickhop 0.1 used on macOS for both the config and the data:
/// `~/Library/Application Support/quickhop`. `None` on other OSes, without `HOME`, and for a
/// development build (0.1 had none, so there is nothing of its own to move).
pub fn legacy_macos_dir(
    flavour: Flavour,
    os: &str,
    var: impl Fn(&str) -> Option<String>,
) -> Option<PathBuf> {
    if os != "macos" || flavour != Flavour::Release {
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
    use crate::Flavour::{Dev, Release};

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
    fn clipboard_limits_are_checked() {
        let config = Config::from_toml("[clipboard]\nmax_age_days = 7\n").unwrap();
        assert_eq!(config.clipboard.max_age_days, 7);
        assert_eq!(
            config.clipboard.max_entries, 1_000,
            "the rest keeps its default"
        );
        assert!(config.clipboard.enabled);
        assert!(
            Config::from_toml("[clipboard]\nmax_age_days = 0\n").is_ok(),
            "0 keeps entries forever"
        );
        for bad in [
            "max_entries = 0",
            "max_entries = 100001",
            "retention_days = 7",
        ] {
            assert!(
                Config::from_toml(&format!("[clipboard]\n{bad}\n")).is_err(),
                "{bad} must be rejected"
            );
        }
        assert!(Config::from_toml("[clipboard]\nmax_entries = 100000\n").is_ok());
    }

    #[test]
    fn paths_per_os() {
        let home = env(&[("HOME", "/h")]);
        let xdg = env(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "/x")]);
        for os in ["linux", "macos"] {
            assert_eq!(
                config_path(Release, os, &home).unwrap(),
                Path::new("/h/.config/quickhop/config.toml"),
                "{os}"
            );
            assert_eq!(
                config_path(Release, os, &xdg).unwrap(),
                Path::new("/x/quickhop/config.toml"),
                "{os}"
            );
            assert_eq!(
                scripts_dir(Release, os, &home).unwrap(),
                Path::new("/h/.config/quickhop/scripts"),
                "{os}"
            );
        }
        assert_eq!(
            scripts_dir(Release, "linux", env(&[("QUICKHOP_CONFIG", "/c/q.toml")])).unwrap(),
            Path::new("/c/scripts")
        );
        assert_eq!(
            config_path(Release, "windows", env(&[("APPDATA", "C:/a")])).unwrap(),
            Path::new("C:/a/quickhop/config.toml")
        );
        assert_eq!(
            config_path(Release, "windows", env(&[("QUICKHOP_CONFIG", "/c.toml")])).unwrap(),
            Path::new("/c.toml")
        );
        assert!(matches!(
            config_path(Release, "windows", &home),
            Err(ConfigError::NoConfigDir("APPDATA"))
        ));
    }

    #[test]
    fn dev_config_lives_in_quickhop_dev_under_dot_config() {
        let home = env(&[("HOME", "/h")]);
        let xdg = env(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "/x")]);
        for os in ["linux", "macos"] {
            assert_eq!(
                config_path(Dev, os, &home).unwrap(),
                Path::new("/h/.config/quickhop-dev/config.toml"),
                "{os}"
            );
            assert_eq!(
                config_path(Dev, os, &xdg).unwrap(),
                Path::new("/x/quickhop-dev/config.toml"),
                "{os}"
            );
            assert_eq!(
                scripts_dir(Dev, os, &home).unwrap(),
                Path::new("/h/.config/quickhop-dev/scripts"),
                "{os}"
            );
        }
        assert_eq!(
            config_path(Dev, "windows", env(&[("APPDATA", "C:/a")])).unwrap(),
            Path::new("C:/a/quickhop-dev/config.toml")
        );
        assert_eq!(
            config_path(Dev, "linux", env(&[("QUICKHOP_CONFIG", "/c.toml")])).unwrap(),
            Path::new("/c.toml"),
            "an explicit override wins for both flavours"
        );
    }

    #[test]
    fn runtime_dirs_per_flavour() {
        let xdg = env(&[("XDG_RUNTIME_DIR", "/run/user/1")]);
        assert_eq!(
            runtime_dir(Release, &xdg),
            Path::new("/run/user/1/quickhop")
        );
        assert_eq!(
            runtime_dir(Dev, &xdg),
            Path::new("/run/user/1/quickhop-dev")
        );
        let tmp = env(&[("TMPDIR", "/t"), ("USER", "me")]);
        assert_eq!(runtime_dir(Release, &tmp), Path::new("/t/quickhop-me"));
        assert_eq!(runtime_dir(Dev, &tmp), Path::new("/t/quickhop-dev-me"));
        assert_eq!(
            runtime_dir(Release, env(&[])),
            Path::new("/tmp/quickhop-user")
        );
        for flavour in [Release, Dev] {
            assert_eq!(
                runtime_dir(
                    flavour,
                    env(&[("QUICKHOP_RUNTIME", "/r"), ("TMPDIR", "/t")])
                ),
                Path::new("/r")
            );
        }
    }

    #[test]
    fn defaults_differ_only_in_the_hotkey() {
        let (release, dev) = (Config::defaults(Release), Config::defaults(Dev));
        assert_eq!(release.hotkey, "Alt+Space");
        assert_eq!(dev.hotkey, "Alt+Shift+Space");
        assert_eq!(
            Config {
                hotkey: release.hotkey.clone(),
                ..dev
            },
            release
        );
        assert_eq!(Config::default(), Config::defaults(Flavour::CURRENT));
        assert_eq!(
            Config::from_toml("").unwrap().hotkey,
            Flavour::CURRENT.default_hotkey(),
            "a file without a hotkey takes this build's"
        );
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
            "[[commands]]\nid = \"apps:x\"\nhotkey = \"Ctrl+Hyper+X\"\n",
            "[[commands]]\nid = \"apps:x\"\nhotkey = \"x\"\n",
            // The launcher's hotkey, spelled differently (its default depends on the flavour).
            "hotkey = \"Alt+Space\"\n[[commands]]\nid = \"apps:x\"\nhotkey = \"option+space\"\n",
            "[[commands]]\nid = \"apps:x\"\nhotkey = \"Ctrl+X\"\n\
             [[commands]]\nid = \"apps:y\"\nhotkey = \"ctrl+x\"\n",
            "hotkey = \"Alt+Spacebar\"\n",
            "hotkey = \"Space\"\n",
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
        for (flavour, stem) in [(Release, "quickhop"), (Dev, "quickhop-dev")] {
            for os in ["linux", "macos"] {
                assert_eq!(
                    data_dir(flavour, os, &home).unwrap(),
                    Path::new("/h/.local/share").join(stem),
                    "{flavour} {os}"
                );
                assert_eq!(
                    data_dir(flavour, os, env(&[("XDG_DATA_HOME", "/d")])).unwrap(),
                    Path::new("/d").join(stem),
                    "{flavour} {os}"
                );
            }
            assert_eq!(
                data_dir(flavour, "windows", env(&[("LOCALAPPDATA", "C:/l")])).unwrap(),
                Path::new("C:/l").join(stem)
            );
            assert_eq!(
                data_dir(flavour, "linux", env(&[("QUICKHOP_DATA", "/q")])).unwrap(),
                Path::new("/q")
            );
            assert!(matches!(
                data_dir(flavour, "windows", &home),
                Err(ConfigError::NoConfigDir("LOCALAPPDATA"))
            ));
        }
    }

    #[test]
    fn legacy_dir_is_macos_only() {
        let home = env(&[("HOME", "/h")]);
        assert_eq!(
            legacy_macos_dir(Release, "macos", &home).unwrap(),
            Path::new("/h/Library/Application Support/quickhop")
        );
        assert_eq!(legacy_macos_dir(Release, "linux", &home), None);
        assert_eq!(legacy_macos_dir(Release, "macos", env(&[])), None);
        assert_eq!(
            legacy_macos_dir(Dev, "macos", &home),
            None,
            "a development build never moves the release's 0.1 files"
        );
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
