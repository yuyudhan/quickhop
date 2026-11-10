//! Built-in quickhop providers, each implementing [`quickhop_core::Provider`]:
//!
//! | name         | module         | source                                                        |
//! |--------------|----------------|---------------------------------------------------------------|
//! | `apps`       | [`apps`]       | XDG desktop entries, macOS `.app` bundles, Windows Start Menu  |
//! | `calc`       | [`calc`]       | fend-core arithmetic and unit conversion                      |
//! | `emoji`      | [`emoji`]      | Unicode emoji with CLDR keywords and GitHub shortcodes        |
//! | `system`     | [`system`]     | lock, log out, suspend, reboot, shut down, per OS             |
//! | `quicklinks` | [`quicklinks`] | `[[quicklinks]]` URL templates with `{query}`                  |
//! | `scripts`    | [`scripts`]    | Raycast-compatible script commands from `~/.config/quickhop/scripts` and `[scripts] dirs` |
//! | `snippets`   | [`snippets`]   | `snippets.toml` next to the config file, with Raycast placeholders |
//! | `files`      | [`files`]      | files and folders by name: Spotlight (`mdfind`) on macOS, else the index built by `quickhop index` |
//! | `views`      | [`views`]      | the commands that open a list in the launcher window: clipboard history, snippets, files |
//!
//! `views` has no `[providers]` switch: each of its commands follows the switch of what it
//! lists.
//!
//! Every provider takes its environment explicitly ([`Env`]) so tests can point it at
//! fixture directories and a fixed OS.
#![forbid(unsafe_code)]

pub mod apps;
pub mod calc;
pub mod emoji;
mod env;
pub mod files;
pub mod quicklinks;
pub mod scripts;
pub mod snippets;
pub mod system;
pub mod views;

use std::path::PathBuf;
use std::time::Duration;

pub use env::Env;
use quickhop_core::{Config, ConfigError, Flavour, Provider};

/// Names of the built-in providers, matching the keys of `[providers]` in the config.
pub const BUILTIN: [&str; 8] = [
    "apps",
    "calc",
    "emoji",
    "system",
    "quicklinks",
    "scripts",
    "snippets",
    "files",
];

/// How long an inline script command may run while its subtitle is computed.
pub const INLINE_SCRIPT_TIMEOUT: Duration = Duration::from_secs(2);

/// The file index database in `env`'s data directory: `index.sqlite`.
///
/// # Errors
/// When no data directory can be derived from the environment.
pub fn index_path(env: &Env) -> Result<PathBuf, ConfigError> {
    let dir = quickhop_core::config::data_dir(env.flavour, &env.os, |name| {
        env.var(name).map(str::to_owned)
    })?;
    Ok(dir.join("index.sqlite"))
}

/// What `quickhop index` covers for `config` in `env`: `[files] roots` (`~` expanded) or the
/// home folder, the default exclusions plus `[files] exclude`, never a data directory (of
/// either flavour).
pub fn index_options(config: &Config, env: &Env) -> quickhop_index::Options {
    let home = env.home();
    let roots = if config.files.roots.is_empty() {
        home.iter().cloned().collect()
    } else {
        config
            .files
            .roots
            .iter()
            .map(|root| scripts::expand_home(root, home.as_deref()))
            .collect()
    };
    let mut options = quickhop_index::Options::new(roots);
    options.hidden = config.files.hidden;
    options.exclude.extend(config.files.exclude.iter().cloned());
    // Both flavours' data directories: neither indexes the other's index either.
    options.skip = [Flavour::Release, Flavour::Dev]
        .into_iter()
        .filter_map(|flavour| {
            quickhop_core::config::data_dir(flavour, &env.os, |name| {
                env.var(name).map(str::to_owned)
            })
            .ok()
        })
        .collect();
    options
}

/// Something that went wrong while loading providers; loading continues without it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Provider name.
    pub provider: &'static str,
    /// Human-readable cause.
    pub message: String,
}

/// The script-command directories for `config` in `env`: `scripts/` next to the config file
/// (when it exists), then `[scripts] dirs` with a leading `~` expanded, without duplicates.
pub fn script_dirs(config: &Config, env: &Env) -> Vec<PathBuf> {
    let default = quickhop_core::config::scripts_dir(env.flavour, &env.os, |name| {
        env.var(name).map(str::to_owned)
    })
    .ok()
    .filter(|dir| dir.is_dir());
    let home = env.home();
    let mut dirs: Vec<PathBuf> = default.into_iter().collect();
    for dir in &config.scripts.dirs {
        let dir = scripts::expand_home(dir, home.as_deref());
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// The file source for `env`: the file index in the data directory ([`index_path`]) when one
/// has been built, else Spotlight over the home directory on macOS; `None` elsewhere.
fn files_source(env: &Env) -> Result<Option<Box<dyn files::FileSource>>, String> {
    let path = index_path(env).map_err(|error| error.to_string())?;
    if let Some(index) = files::Indexed::open(&path)? {
        return Ok(Some(Box::new(index)));
    }
    if env.os != "macos" {
        return Ok(None);
    }
    match env.home() {
        Some(home) if home.is_dir() => Ok(Some(Box::new(files::Spotlight::new(
            home,
            files::SPOTLIGHT_TIMEOUT,
        )))),
        Some(home) => Err(format!("home directory {} not found", home.display())),
        None => Err("HOME is not set".into()),
    }
}

/// The providers enabled in `config`, loaded for `env`, in display-priority order, plus
/// the problems met while loading them.
pub fn builtin(config: &Config, env: &Env) -> (Vec<Box<dyn Provider>>, Vec<Warning>) {
    let enabled = &config.providers;
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();
    let mut warnings = Vec::new();
    if enabled.quicklinks {
        providers.push(Box::new(quicklinks::QuicklinksProvider::new(
            config.quicklinks.clone(),
        )));
    }
    if enabled.calc {
        providers.push(Box::new(calc::CalcProvider::new()));
    }
    if enabled.apps {
        let (apps, problems) =
            apps::AppsProvider::load(&apps::Sources::for_env(&config.apps, env), env);
        warnings.extend(problems.into_iter().map(|message| Warning {
            provider: "apps",
            message,
        }));
        providers.push(Box::new(apps));
    }
    if enabled.scripts {
        let (scripts, problems) =
            scripts::ScriptsProvider::load(&script_dirs(config, env), Some(INLINE_SCRIPT_TIMEOUT));
        warnings.extend(problems.into_iter().map(|message| Warning {
            provider: "scripts",
            message,
        }));
        providers.push(Box::new(scripts));
    }
    if enabled.snippets {
        let path = quickhop_core::snippets::snippets_path(env.flavour, &env.os, |name| {
            env.var(name).map(str::to_owned)
        });
        match path {
            Ok(path) => {
                let (snippets, problems) = snippets::SnippetsProvider::load(&path);
                warnings.extend(problems.into_iter().map(|message| Warning {
                    provider: "snippets",
                    message,
                }));
                providers.push(Box::new(snippets));
            }
            Err(error) => warnings.push(Warning {
                provider: "snippets",
                message: error.to_string(),
            }),
        }
    }
    if enabled.system {
        providers.push(Box::new(system::SystemProvider::for_os(&env.os)));
    }
    providers.push(Box::new(views::ViewsProvider::new(config)));
    if enabled.emoji {
        providers.push(Box::new(emoji::EmojiProvider::new()));
    }
    if enabled.files {
        match files_source(env) {
            Ok(Some(source)) => providers.push(Box::new(files::FilesProvider::new(
                source,
                &env.os,
                env.home(),
            ))),
            // No index built yet: nothing to search, and nothing wrong.
            Ok(None) => {}
            Err(message) => warnings.push(Warning {
                provider: files::NAME,
                message,
            }),
        }
    }
    (providers, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_dirs_expand_home_and_skip_duplicates() {
        let mut config = Config::default();
        config.scripts.dirs = vec![
            PathBuf::from("~/raycast"),
            PathBuf::from("/h/raycast"),
            PathBuf::from("/abs"),
        ];
        // The default directory does not exist under this fake home, so it is left out.
        let env = Env::new("linux", [("HOME", "/h")]);
        assert_eq!(
            script_dirs(&config, &env),
            vec![PathBuf::from("/h/raycast"), PathBuf::from("/abs")]
        );
    }
}
