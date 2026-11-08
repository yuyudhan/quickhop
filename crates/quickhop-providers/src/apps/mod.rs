//! Installed applications: XDG desktop entries on Linux and other Unixes, `.app` bundles
//! on macOS, Start Menu shortcuts and packaged (UWP) apps on Windows.
//!
//! Every scanner is plain file parsing, so all of them compile and are tested on every OS;
//! [`Sources::for_env`] picks the one that matches the running OS.

pub mod bundles;
pub mod desktop;
pub mod icons;
pub mod start_menu;

use std::path::PathBuf;

use quickhop_core::config::Apps;
use quickhop_core::{Cancel, Item, Provider, ProviderError, Query};

use crate::env::Env;

/// Provider name.
pub const NAME: &str = "apps";

/// Where applications come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sources {
    /// XDG `applications` directories, earlier ones taking precedence, and the icon theme
    /// used to resolve `Icon=` names.
    Desktop {
        /// Directories holding `.desktop` files (searched recursively).
        dirs: Vec<PathBuf>,
        /// Icon theme lookup configuration.
        icons: icons::IconSearch,
    },
    /// Directories holding `.app` bundles.
    Bundles {
        /// Searched two levels deep (e.g. `/Applications/Utilities`).
        dirs: Vec<PathBuf>,
    },
    /// Start Menu `Programs` folders, plus packaged apps from `Get-StartApps`.
    StartMenu {
        /// Searched recursively for `.lnk` files.
        dirs: Vec<PathBuf>,
        /// Whether to ask PowerShell for packaged (UWP) apps.
        packaged: bool,
    },
}

impl Sources {
    /// The sources for `env.os`: configured `dirs` replace the OS defaults.
    pub fn for_env(config: &Apps, env: &Env) -> Self {
        let configured = (!config.dirs.is_empty()).then(|| config.dirs.clone());
        match env.os.as_str() {
            "macos" => Self::Bundles {
                dirs: configured.unwrap_or_else(|| bundles::default_dirs(env)),
            },
            "windows" => Self::StartMenu {
                packaged: configured.is_none(),
                dirs: configured.unwrap_or_else(|| start_menu::default_dirs(env)),
            },
            _ => Self::Desktop {
                dirs: configured.unwrap_or_else(|| desktop::default_dirs(env)),
                icons: icons::IconSearch::for_env(&config.icon_theme, env),
            },
        }
    }
}

/// The applications found when the provider was loaded.
pub struct AppsProvider {
    items: Vec<Item>,
}

impl AppsProvider {
    /// Scans `sources` once; the problems met (unreadable files, bad entries in configured
    /// directories) are returned next to the provider instead of failing it.
    pub fn load(sources: &Sources, env: &Env) -> (Self, Vec<String>) {
        let (items, warnings) = match sources {
            Sources::Desktop { dirs, icons } => {
                desktop::scan(dirs, env, &mut icons::IconResolver::new(icons))
            }
            Sources::Bundles { dirs } => bundles::scan(dirs),
            Sources::StartMenu { dirs, packaged } => {
                let (mut items, mut warnings) = start_menu::scan(dirs);
                if *packaged {
                    match start_menu::packaged_apps() {
                        Ok(apps) => start_menu::merge_packaged(&mut items, apps),
                        Err(error) => warnings.push(format!("packaged apps: {error}")),
                    }
                }
                (items, warnings)
            }
        };
        (Self { items }, warnings)
    }

    /// A provider over already-built items.
    pub fn from_items(items: Vec<Item>) -> Self {
        Self { items }
    }

    /// Every application found.
    pub fn all(&self) -> &[Item] {
        &self.items
    }
}

impl Provider for AppsProvider {
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
    fn sources_follow_the_os() {
        let env = Env::new("macos", [("HOME", "/h")]);
        let Sources::Bundles { dirs } = Sources::for_env(&Apps::default(), &env) else {
            unreachable!("macOS uses bundles");
        };
        assert!(dirs.contains(&PathBuf::from("/h/Applications")));
        let env = Env::new("windows", [("APPDATA", "C:/a")]);
        assert!(matches!(
            Sources::for_env(&Apps::default(), &env),
            Sources::StartMenu { packaged: true, .. }
        ));
        let config = Apps {
            dirs: vec![PathBuf::from("/x")],
            icon_theme: String::new(),
        };
        let Sources::Desktop { dirs, .. } =
            Sources::for_env(&config, &Env::new("linux", [("HOME", "/h")]))
        else {
            unreachable!("Linux uses desktop entries");
        };
        assert_eq!(dirs, vec![PathBuf::from("/x")]);
        assert!(matches!(
            Sources::for_env(
                &config,
                &Env::new("windows", Vec::<(String, String)>::new())
            ),
            Sources::StartMenu {
                packaged: false,
                ..
            }
        ));
    }
}
