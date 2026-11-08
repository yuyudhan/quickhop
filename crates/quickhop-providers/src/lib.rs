//! Built-in quickhop providers, each implementing [`quickhop_core::Provider`]:
//!
//! | name         | module         | source                                                        |
//! |--------------|----------------|---------------------------------------------------------------|
//! | `apps`       | [`apps`]       | XDG desktop entries, macOS `.app` bundles, Windows Start Menu  |
//! | `calc`       | [`calc`]       | fend-core arithmetic and unit conversion                      |
//! | `emoji`      | [`emoji`]      | Unicode emoji with CLDR keywords and GitHub shortcodes        |
//! | `system`     | [`system`]     | lock, log out, suspend, reboot, shut down, per OS             |
//! | `quicklinks` | [`quicklinks`] | `[[quicklinks]]` URL templates with `{query}`                  |
//! | `scripts`    | [`scripts`]    | Raycast-compatible script commands from `[scripts] dirs`      |
//!
//! Every provider takes its environment explicitly ([`Env`]) so tests can point it at
//! fixture directories and a fixed OS.
#![forbid(unsafe_code)]

pub mod apps;
pub mod calc;
pub mod emoji;
mod env;
pub mod quicklinks;
pub mod scripts;
pub mod system;

use std::time::Duration;

pub use env::Env;
use quickhop_core::{Config, Provider};

/// Names of the built-in providers, matching the keys of `[providers]` in the config.
pub const BUILTIN: [&str; 6] = ["apps", "calc", "emoji", "system", "quicklinks", "scripts"];

/// How long an inline script command may run while its subtitle is computed.
pub const INLINE_SCRIPT_TIMEOUT: Duration = Duration::from_secs(2);

/// Something that went wrong while loading providers; loading continues without it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Provider name.
    pub provider: &'static str,
    /// Human-readable cause.
    pub message: String,
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
            scripts::ScriptsProvider::load(&config.scripts.dirs, Some(INLINE_SCRIPT_TIMEOUT));
        warnings.extend(problems.into_iter().map(|message| Warning {
            provider: "scripts",
            message,
        }));
        providers.push(Box::new(scripts));
    }
    if enabled.system {
        providers.push(Box::new(system::SystemProvider::for_os(&env.os)));
    }
    if enabled.emoji {
        providers.push(Box::new(emoji::EmojiProvider::new()));
    }
    (providers, warnings)
}
