//! What the daemon's launcher window searches and runs: [`Launcher`] answers the window's
//! searches from providers loaded once per configuration and carries out the chosen actions
//! as `quickhop launch` does (macOS for now, plan slices U2 and U6). It also runs the first
//! [`MAX_INLINE`] inline script commands again every `refreshTime`, as Raycast does, so their
//! subtitles stay current ([`refresh_inline`]).

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use quickhop_core::paste::Pasted;
use quickhop_core::{
    Action, Boosts, Cancel, Config, Item, ItemId, Query, Ranked, ScriptMode, Search, View, rank,
};
use quickhop_platform_macos::icon::{IconRenderer as _, SystemIcons};
use quickhop_providers::{scripts, views};
use quickhop_ui::model::Outcome;
use quickhop_ui::window::{Backend, Command, Handle, ICON_PIXELS};

/// Most entries a list (clipboard history, snippets, files) shows.
const LIST_LIMIT: usize = 200;

/// How long a paste waits for the app the user came from to be in front again.
const REFOCUS_TIMEOUT: Duration = Duration::from_secs(1);

/// Inline script commands refreshed automatically, the first ones found (Raycast's number).
pub(crate) const MAX_INLINE: usize = 10;

/// How often [`refresh_inline`] looks for inline scripts due to run again.
const INLINE_TICK: Duration = Duration::from_millis(500);

/// The window's backend: the root search over every enabled provider.
pub(crate) struct Launcher {
    /// Rebuilt on reload; searches in flight keep the one they started with.
    search: Mutex<Arc<Search>>,
    /// The inline scripts refreshed in the background, rebuilt on reload.
    inline: Mutex<Vec<Inline>>,
    /// How far the UI tests moved the refresh clock ahead (`advance` on the test socket).
    ahead: Mutex<Duration>,
}

/// An inline script command and its newest first line of output.
struct Inline {
    id: ItemId,
    action: Action,
    every: Duration,
    last_run: Instant,
    subtitle: Option<String>,
}

impl Launcher {
    /// Loads the providers `config` enables; problems loading them go to standard error.
    pub(crate) fn new(config: &Config) -> Self {
        Self {
            search: Mutex::new(Arc::new(load(config))),
            inline: Mutex::new(inline_scripts(config, Instant::now())),
            ahead: Mutex::new(Duration::ZERO),
        }
    }

    /// Loads the providers of `config` again.
    pub(crate) fn reload(&self, config: &Config) {
        let search = Arc::new(load(config));
        *self.search.lock().unwrap_or_else(PoisonError::into_inner) = search;
        *self.inline.lock().unwrap_or_else(PoisonError::into_inner) =
            inline_scripts(config, self.now());
    }

    /// Moves the refresh clock `by` ahead, for the UI tests.
    pub(crate) fn advance(&self, by: Duration) {
        *self.ahead.lock().unwrap_or_else(PoisonError::into_inner) += by;
    }

    fn now(&self) -> Instant {
        Instant::now() + *self.ahead.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs every inline script whose `refreshTime` passed since it last ran. Returns whether
    /// a subtitle changed. A failing script keeps its subtitle; the failure goes to standard
    /// error.
    fn refresh_due(&self) -> bool {
        let now = self.now();
        let due: Vec<(ItemId, Action)> = self
            .inline
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|script| now.duration_since(script.last_run) >= script.every)
            .map(|script| (script.id.clone(), script.action.clone()))
            .collect();
        let mut changed = false;
        for (id, action) in due {
            let output = match scripts::run(&action, &[], quickhop_providers::INLINE_SCRIPT_TIMEOUT)
            {
                Ok(output) if output.success => Some(output.message),
                Ok(output) => {
                    crate::warn(&format!("{id}: {}", output.message));
                    None
                }
                Err(error) => {
                    crate::warn(&format!("{id}: {error}"));
                    None
                }
            };
            changed |= self.set_inline(&id, output, now);
        }
        changed
    }

    /// Records that the inline script `id` ran at `now` and printed `subtitle` (`None`: it
    /// failed). Returns whether its subtitle changed.
    fn set_inline(&self, id: &ItemId, subtitle: Option<String>, now: Instant) -> bool {
        let mut inline = self.inline.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(script) = inline.iter_mut().find(|script| &script.id == id) else {
            return false;
        };
        script.last_run = now;
        match subtitle {
            Some(subtitle) if script.subtitle.as_ref() != Some(&subtitle) => {
                script.subtitle = Some(subtitle);
                true
            }
            _ => false,
        }
    }
}

/// The first [`MAX_INLINE`] inline script commands of `config`'s script directories that
/// refresh (have a `refreshTime`) and take no arguments, as if they last ran at `now` (when
/// the providers loaded, which runs them once).
fn inline_scripts(config: &Config, now: Instant) -> Vec<Inline> {
    if !config.providers.scripts {
        return Vec::new();
    }
    let dirs = quickhop_providers::script_dirs(config, &quickhop_providers::Env::current());
    let (found, _) = scripts::discover(&dirs);
    found
        .iter()
        .filter(|script| script.mode == ScriptMode::Inline && script.arguments.is_empty())
        .filter_map(|script| {
            Some(Inline {
                id: scripts::to_item(script, None).id,
                action: scripts::action(script),
                every: script.refresh?,
                last_run: now,
                subtitle: None,
            })
        })
        .take(MAX_INLINE)
        .collect()
}

/// Runs the inline scripts of `launcher` when they are due, on a thread of its own, and asks
/// the window to search again when a subtitle changed; stops when the window is gone.
pub(crate) fn refresh_inline(launcher: Arc<Launcher>, window: Handle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(INLINE_TICK);
            if launcher.refresh_due() && window.send(Command::Refresh).is_err() {
                return;
            }
        }
    });
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

/// The frecency of the picks for `query`; problems go to standard error.
fn boosts(query: &Query) -> Boosts {
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
    boosts
}

/// Waits, at most [`REFOCUS_TIMEOUT`], until another app than quickhop is in front: once the
/// window hid, macOS gives the focus back to the app the user came from, and a paste must
/// reach that app.
fn wait_for_another_app() {
    let quickhop = i32::try_from(std::process::id()).ok();
    let deadline = Instant::now() + REFOCUS_TIMEOUT;
    while quickhop_platform_macos::input::frontmost_pid() == quickhop && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl Launcher {
    /// The entries of the list the command `source` opens that match `query`, best first:
    /// the clipboard history newest first until something is typed, or what the snippets or
    /// files provider has for `query`. Problems go to standard error.
    fn list(&self, source: &ItemId, query: &Query) -> Vec<Ranked> {
        let Some(view) = views::view(source) else {
            return Vec::new();
        };
        let (items, boosts) = match view {
            View::Clipboard => {
                let clips = crate::clipboard::open()
                    .and_then(|state| Ok(state.clips(LIST_LIMIT)?))
                    .unwrap_or_else(|error| {
                        crate::warn(&error.to_string());
                        Vec::new()
                    });
                (views::clipboard_items(clips), Boosts::new())
            }
            View::Snippets | View::Files => {
                let name = if view == View::Snippets {
                    quickhop_providers::snippets::NAME
                } else {
                    quickhop_providers::files::NAME
                };
                let search =
                    Arc::clone(&self.search.lock().unwrap_or_else(PoisonError::into_inner));
                let items = search
                    .provider(name)
                    .map(|provider| provider.items(query, &Cancel::new()))
                    .transpose()
                    .unwrap_or_else(|error| {
                        crate::warn(&error.to_string());
                        None
                    })
                    .unwrap_or_default();
                (items, boosts(query))
            }
        };
        rank(
            query,
            items.into_iter().map(|item| (item, false)),
            &boosts,
            LIST_LIMIT,
        )
    }
}

impl Backend for Launcher {
    fn search(&self, source: Option<&ItemId>, query: &Query) -> Vec<Ranked> {
        if let Some(source) = source {
            return self.list(source, query);
        }
        let search = Arc::clone(&self.search.lock().unwrap_or_else(PoisonError::into_inner));
        let mut ranked = search.run(query, &Cancel::new(), &boosts(query)).ranked;
        let inline = self.inline.lock().unwrap_or_else(PoisonError::into_inner);
        for result in &mut ranked {
            if let Some(subtitle) = inline
                .iter()
                .find(|script| script.id == result.item.id)
                .and_then(|script| script.subtitle.clone())
            {
                result.item.subtitle = Some(subtitle);
            }
        }
        ranked
    }

    fn perform(
        &self,
        item: &Item,
        action: &Action,
        args: &[String],
        query: &Query,
    ) -> Result<Outcome, String> {
        if action.needs_confirmation() {
            return Err(format!(
                "{} needs confirmation: run `quickhop launch {} --yes`",
                item.title, item.id
            ));
        }
        let output = crate::launch::perform(action, args).map_err(|e| e.to_string())?;
        let outcome = match action {
            Action::Copy { .. } | Action::Snippet { .. } => {
                crate::paste::copy(&output).map_err(|e| e.to_string())?;
                Outcome::Hud("Copied to Clipboard".to_owned())
            }
            Action::Paste { .. } => {
                wait_for_another_app();
                match crate::paste::paste(&output).map_err(|e| e.to_string())? {
                    Pasted::Pasted => Outcome::Done,
                    Pasted::CopiedOnly(why) => Outcome::Hud(format!("Copied, not pasted: {why}")),
                }
            }
            Action::Script { mode, .. } => match mode {
                ScriptMode::FullOutput => Outcome::Detail {
                    title: item.title.clone(),
                    text: output,
                },
                ScriptMode::Compact => Outcome::Toast(output),
                ScriptMode::Silent if output.is_empty() => Outcome::Done,
                ScriptMode::Silent => Outcome::Hud(output),
                ScriptMode::Inline => {
                    self.set_inline(&item.id, Some(output.clone()), self.now());
                    Outcome::Subtitle(output)
                }
            },
            Action::Trash { .. } => Outcome::Hud(output),
            // The window opens lists itself (`launch::perform` refuses a view).
            Action::Open { .. }
            | Action::Run { .. }
            | Action::Reveal { .. }
            | Action::View { .. } => Outcome::Done,
        };
        // Clipboard entries come and go; only lasting items are worth remembering.
        if item.id.provider != "clipboard" {
            let mut warnings = Vec::new();
            if let Some(Err(error)) = crate::open_store(&mut warnings)
                .map(|mut store| store.record(query.text(), &item.id, crate::now()))
            {
                warnings.push(error.to_string());
            }
            for warning in &warnings {
                crate::warn(warning);
            }
        }
        Ok(outcome)
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
