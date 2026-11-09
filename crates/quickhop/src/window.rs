//! What the daemon's launcher window searches and runs: [`Launcher`] answers the window's
//! searches from providers loaded once per configuration and carries out the chosen actions
//! as `quickhop launch` does (macOS for now, plan slice U2).

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use quickhop_core::{Action, Boosts, Cancel, Config, Item, ItemId, Query, Ranked, Search};
use quickhop_platform_macos::icon::{IconRenderer as _, SystemIcons};
use quickhop_ui::window::{Backend, ICON_PIXELS};

/// The window's backend: the root search over every enabled provider.
pub(crate) struct Launcher {
    /// Rebuilt on reload; searches in flight keep the one they started with.
    search: Mutex<Arc<Search>>,
}

impl Launcher {
    /// Loads the providers `config` enables; problems loading them go to standard error.
    pub(crate) fn new(config: &Config) -> Self {
        Self {
            search: Mutex::new(Arc::new(load(config))),
        }
    }

    /// Loads the providers of `config` again.
    pub(crate) fn reload(&self, config: &Config) {
        let search = Arc::new(load(config));
        *self.search.lock().unwrap_or_else(PoisonError::into_inner) = search;
    }
}

fn load(config: &Config) -> Search {
    let (search, warnings) =
        crate::load_search(config, config.max_results).unwrap_or_else(|error| {
            // A `[[commands]]` id naming no item: search without the command settings.
            let (providers, _) =
                quickhop_providers::builtin(config, &quickhop_providers::Env::current());
            (
                Search::new(config.max_results).with_providers(providers),
                vec![error.to_string()],
            )
        });
    for warning in warnings {
        crate::warn(&warning);
    }
    search
}

impl Backend for Launcher {
    fn search(&self, source: Option<&ItemId>, query: &Query) -> Vec<Ranked> {
        if source.is_some() {
            // Pages pushed by commands (clipboard history, script items) come with their views.
            return Vec::new();
        }
        let search = Arc::clone(&self.search.lock().unwrap_or_else(PoisonError::into_inner));
        let mut warnings = Vec::new();
        let boosts =
            match crate::open_store(&mut warnings).map(|s| s.boosts(query.text(), crate::now())) {
                Some(Ok(boosts)) => boosts,
                Some(Err(error)) => {
                    warnings.push(error.to_string());
                    Boosts::new()
                }
                None => Boosts::new(),
            };
        for warning in &warnings {
            crate::warn(warning);
        }
        search.run(query, &Cancel::new(), &boosts).ranked
    }

    fn perform(&self, item: &Item, action: &Action, query: &Query) -> Result<String, String> {
        if action.needs_confirmation() {
            return Err(format!(
                "{} needs confirmation: run `quickhop launch {} --yes`",
                item.title, item.id
            ));
        }
        let output = crate::launch::perform(action, &[]).map_err(|e| e.to_string())?;
        let message = match action {
            Action::Copy { .. } | Action::Snippet { .. } => {
                crate::paste::copy(&output).map_err(|e| e.to_string())?;
                "Copied to Clipboard".to_owned()
            }
            _ => output,
        };
        let mut warnings = Vec::new();
        if let Some(Err(error)) = crate::open_store(&mut warnings)
            .map(|mut store| store.record(query.text(), &item.id, crate::now()))
        {
            warnings.push(error.to_string());
        }
        for warning in &warnings {
            crate::warn(warning);
        }
        Ok(message)
    }

    fn icon(&self, item: &Item) -> Option<Vec<u8>> {
        // Applications and files open or show a path: the system draws that path's icon.
        let path = item.actions.iter().find_map(|action| match action {
            Action::Open { target } | Action::Reveal { path: target } => {
                Some(Path::new(target)).filter(|path| path.is_absolute())
            }
            _ => None,
        })?;
        SystemIcons.render(path, ICON_PIXELS).ok()
    }
}
