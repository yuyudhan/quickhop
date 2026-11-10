//! The quickhop command-line entry point.

#![forbid(unsafe_code)]

#[cfg(target_os = "macos")]
mod app;
mod autostart;
mod clipboard;
#[cfg(unix)]
mod daemon;
mod doctor;
mod launch;
mod paste;
mod tools;
mod trash;
#[cfg(target_os = "macos")]
mod window;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use quickhop_core::config::{config_path, data_dir, legacy_macos_dir, migrate_legacy};
use quickhop_core::{Action, Boosts, Cancel, Config, Flavour, ItemId, Query, Ranked, Search, Tier};
use quickhop_providers::scripts::{self, ScriptCommand};
use quickhop_providers::{Env, Warning};
use quickhop_store::Store;
use serde::Serialize;

/// What `quickhop --version` prints after the name: the version, and ` (dev)` for a
/// development build ([`Flavour`]), which scripts/bundle-macos.sh reads to name the app.
const VERSION: &str = match Flavour::CURRENT {
    Flavour::Release => env!("CARGO_PKG_VERSION"),
    Flavour::Dev => concat!(env!("CARGO_PKG_VERSION"), " (dev)"),
};

/// Keyboard launcher for macOS and Linux (and Windows).
#[derive(Debug, Parser)]
#[command(name = "quickhop", version = VERSION, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Search every enabled provider and print the ranked results.
    Query {
        /// What to search for (words are joined with spaces; put `--` before text that
        /// starts with a hyphen).
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
        /// Print JSON instead of one line per result.
        #[arg(long)]
        json: bool,
        /// Number of results (default: `max_results` from the config).
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..=100))]
        limit: Option<u16>,
    },
    /// Run the default action of a result and remember the pick for ranking.
    Launch {
        /// The result's id, as printed by `query` (`provider:key`).
        id: String,
        /// The query the result was picked for (some results depend on it).
        #[arg(long, default_value = "", allow_hyphen_values = true)]
        query: String,
        /// A script-command argument; repeat for several.
        #[arg(long = "arg")]
        args: Vec<String>,
        /// Confirm actions that need it (system power commands).
        #[arg(long)]
        yes: bool,
        /// Print the action as JSON instead of running it.
        #[arg(long)]
        dry_run: bool,
        /// Run the result's first action of this kind (`open`, `copy`, `reveal`, `trash`, …)
        /// instead of its default one.
        #[arg(long)]
        action: Option<String>,
        /// Paste a text result (copy, snippet) into the focused application instead of
        /// printing it.
        #[arg(long)]
        paste: bool,
    },
    /// Inspect or validate the configuration file.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Run a Raycast-compatible script command and print its output for its mode.
    Run {
        /// A script file, or the file name, file stem or title of a script command in the
        /// script directories (`~/.config/quickhop/scripts` and `[scripts] dirs`).
        script: String,
        /// Values for `@raycast.argument1`..`argument3`, in order.
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
        /// Confirm scripts marked `@raycast.needsConfirmation true`.
        #[arg(long)]
        yes: bool,
        /// Seconds before the script is killed.
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=3600))]
        timeout: u64,
    },
    /// Report the binary and its other copies, the daemon, Accessibility (macOS), autostart,
    /// paths, the session's helper tools and the providers; `warn:` lines say how to fix.
    Doctor {
        /// Fail when anything warned.
        #[arg(long)]
        strict: bool,
    },
    /// Build the file index (`[files] roots`, default the home folder) for file search.
    Index {
        /// Keep running and apply file changes as they happen.
        #[arg(long)]
        watch: bool,
    },
    /// Clipboard history: watch, add, list, clear.
    Clipboard {
        #[command(subcommand)]
        action: clipboard::ClipboardAction,
    },
    /// Run the background daemon (one per user): the launcher window (macOS), clipboard
    /// history, the file index, and the `open`, `toggle` and `reload` requests. `status` and
    /// `stop` talk to a running one.
    #[cfg(unix)]
    Daemon {
        #[command(subcommand)]
        action: Option<DaemonAction>,
        /// Run without the launcher window (a session without a display).
        #[arg(long)]
        no_window: bool,
        /// Answer `snapshot` on this Unix socket with what the window shows, as JSON (the UI
        /// tests).
        #[arg(long, value_name = "PATH")]
        test_socket: Option<std::path::PathBuf>,
        /// Start the daemon in the background unless one runs, and say so in a desktop
        /// notification (the app-menu entry: Linux has no launcher window yet).
        #[cfg(not(target_os = "macos"))]
        #[arg(long, conflicts_with_all = ["no_window", "test_socket"])]
        notify: bool,
    },
    /// Show the launcher window, or hide it when shown, starting the daemon when none runs.
    #[cfg(unix)]
    Toggle,
    /// Show the launcher window, starting the daemon when none runs.
    #[cfg(unix)]
    Show,
    /// Hide the launcher window.
    #[cfg(unix)]
    Hide,
    /// Show the onboarding window: whether quickhop may paste into other apps (the macOS
    /// Accessibility permission) and the button that allows it. Starts the daemon when none
    /// runs.
    #[cfg(unix)]
    Onboarding,
    /// Run a result's action in the daemon, starting the daemon when none runs.
    #[cfg(unix)]
    Open {
        /// The result's id, as printed by `query` (`provider:key`).
        id: String,
        /// The query the result was picked for (some results depend on it).
        #[arg(long, default_value = "", allow_hyphen_values = true)]
        query: String,
        /// A script-command or snippet argument; repeat for several.
        #[arg(long = "arg")]
        args: Vec<String>,
        /// Confirm actions that need it.
        #[arg(long)]
        yes: bool,
        /// Run the result's first action of this kind instead of its default one.
        #[arg(long)]
        action: Option<String>,
        /// Paste a text result into the focused application instead of printing it.
        #[arg(long)]
        paste: bool,
    },
    /// Make the running daemon read the configuration again.
    #[cfg(unix)]
    Reload,
    /// Start the daemon at login: on, off or status.
    Autostart {
        #[command(subcommand)]
        action: autostart::AutostartAction,
    },
}

#[cfg(unix)]
#[derive(Debug, Subcommand)]
enum DaemonAction {
    /// Print whether the daemon runs and what it watches.
    Status,
    /// Stop the running daemon.
    Stop,
}

#[derive(Debug, Subcommand)]
enum ConfigAction {
    /// Print the default configuration as TOML.
    Default,
    /// Print where the configuration file is read from.
    Path,
    /// Validate a configuration file (default: the file at `config path`).
    Check {
        /// File to check instead of the default location.
        file: Option<PathBuf>,
    },
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error(transparent)]
    Config(#[from] quickhop_core::ConfigError),
    #[error("cannot write output: {0}")]
    Io(#[from] io::Error),
    #[error("cannot encode JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid id {0:?}: expected provider:key")]
    BadId(String),
    #[error("no result {0} for query {1:?}")]
    NotFound(String, String),
    #[error("{0} needs confirmation: run again with --yes")]
    NeedsConfirmation(String),
    #[error("only text results (copy, snippet) can be pasted, not {0}")]
    NotPasteable(&'static str),
    #[error(transparent)]
    Paste(#[from] paste::PasteError),
    #[error("{id} has no {kind} action (it has: {available})")]
    NoAction {
        id: String,
        kind: String,
        available: String,
    },
    #[error(transparent)]
    Launch(#[from] launch::LaunchError),
    #[error("invalid script command {path}: {source}")]
    BadScript {
        path: String,
        #[source]
        source: scripts::ParseError,
    },
    #[error("cannot read {path}: {source}")]
    ReadScript {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("no script command {0:?} in {1}")]
    ScriptNotFound(String, String),
    #[error("{0:?} matches several script commands: {1}")]
    AmbiguousScript(String, String),
    #[error(transparent)]
    ScriptRun(#[from] scripts::RunError),
    #[error("script failed: {0}")]
    ScriptFailed(String),
    #[error(transparent)]
    Index(#[from] quickhop_index::IndexError),
    #[error(transparent)]
    Clipboard(#[from] clipboard::ClipboardError),
    #[cfg(unix)]
    #[error(transparent)]
    Daemon(#[from] daemon::DaemonError),
    #[error("nothing to index: set [files] roots or HOME")]
    NoIndexRoots,
    #[error(transparent)]
    Autostart(#[from] autostart::AutostartError),
    #[error("doctor: {0} warning(s)")]
    DoctorWarned(usize),
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn default_path() -> Result<PathBuf, quickhop_core::ConfigError> {
    config_path(Flavour::CURRENT, std::env::consts::OS, var)
}

/// Moves the config and frecency database from quickhop 0.1's macOS location
/// (`~/Library/Application Support/quickhop`) to the XDG ones, once, in a release (a
/// development build leaves them to it). Locations overridden with `QUICKHOP_CONFIG` /
/// `QUICKHOP_DATA` are left alone. Problems are reported, not fatal.
fn migrate_legacy_files() {
    let Some(legacy) = legacy_macos_dir(Flavour::CURRENT, std::env::consts::OS, var) else {
        return;
    };
    if !legacy.is_dir() {
        return;
    }
    let config = var("QUICKHOP_CONFIG")
        .is_none()
        .then(default_path)
        .and_then(Result::ok);
    let data = var("QUICKHOP_DATA")
        .is_none()
        .then(|| data_dir(Flavour::CURRENT, std::env::consts::OS, var))
        .and_then(Result::ok);
    let lines = match migrate_legacy(&legacy, config.as_deref(), data.as_deref()) {
        Ok(moved) => moved,
        Err(error) => vec![error.to_string()],
    };
    for line in lines {
        let _ = writeln!(io::stderr(), "quickhop: {line}");
    }
}

/// The config file when it exists, else the defaults.
fn load_config() -> Result<Config, CliError> {
    let path = default_path()?;
    Ok(if path.exists() {
        Config::load(&path)?
    } else {
        Config::default()
    })
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Unix milliseconds: the clipboard history's clock.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The frecency store in the data directory; problems become warnings and the search
/// runs without frecency.
fn open_store(warnings: &mut Vec<String>) -> Option<Store> {
    let opened = data_dir(Flavour::CURRENT, std::env::consts::OS, |name| {
        std::env::var(name).ok()
    })
    .map_err(|e| e.to_string())
    .and_then(|dir| {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Store::open(&dir.join("frecency.sqlite")).map_err(|e| e.to_string())
    });
    match opened {
        Ok(store) => Some(store),
        Err(error) => {
            warnings.push(format!("frecency disabled: {error}"));
            None
        }
    }
}

fn warning_text(warning: &Warning) -> String {
    format!("{}: {}", warning.provider, warning.message)
}

/// A problem worth telling but not failing for, on standard error (the daemon's and the
/// window's: Unix only).
#[cfg(unix)]
fn warn(message: &str) {
    let _ = writeln!(io::stderr(), "quickhop: {message}");
}

struct Outcome {
    ranked: Vec<Ranked>,
    errors: Vec<String>,
    warnings: Vec<String>,
}

/// The enabled providers with the `[[commands]]` settings applied, plus loading warnings.
///
/// # Errors
/// When a `[[commands]]` id names no item.
fn load_search(config: &Config, limit: usize) -> Result<(Search, Vec<String>), CliError> {
    let env = Env::current();
    let (providers, loaded) = quickhop_providers::builtin(config, &env);
    let search = Search::new(limit)
        .with_providers(providers)
        .with_commands(&config.commands);
    if !config.commands.is_empty() {
        let unknown = search.unknown_commands();
        if !unknown.is_empty() {
            let ids = unknown.iter().map(ToString::to_string).collect();
            return Err(quickhop_core::ConfigError::UnknownCommands(ids).into());
        }
    }
    Ok((search, loaded.iter().map(warning_text).collect()))
}

/// Searches every enabled provider with the `[[commands]]` settings applied.
///
/// # Errors
/// When a `[[commands]]` id names no item.
fn search(config: &Config, text: &str, limit: usize) -> Result<Outcome, CliError> {
    let (search, mut warnings) = load_search(config, limit)?;
    let boosts = match open_store(&mut warnings).map(|s| s.boosts(text, now())) {
        Some(Ok(boosts)) => boosts,
        Some(Err(error)) => {
            warnings.push(error.to_string());
            Boosts::new()
        }
        None => Boosts::new(),
    };
    let results = search.run(&Query::new(text), &Cancel::new(), &boosts);
    Ok(Outcome {
        ranked: results.ranked,
        errors: results.errors.iter().map(ToString::to_string).collect(),
        warnings,
    })
}

#[derive(Serialize)]
struct JsonResult<'a> {
    id: String,
    provider: &'a str,
    title: &'a str,
    subtitle: Option<&'a str>,
    icon: Option<&'a str>,
    tier: Tier,
    score: i64,
    positions: &'a [usize],
    actions: &'a [Action],
}

#[derive(Serialize)]
struct JsonOutput<'a> {
    query: &'a str,
    results: Vec<JsonResult<'a>>,
    errors: &'a [String],
    warnings: &'a [String],
}

fn query(
    config: &Config,
    text: &str,
    json: bool,
    limit: usize,
    out: &mut impl Write,
) -> Result<(), CliError> {
    let outcome = search(config, text, limit)?;
    if json {
        let results = outcome
            .ranked
            .iter()
            .map(|r| JsonResult {
                id: r.item.id.to_string(),
                provider: &r.item.id.provider,
                title: &r.item.title,
                subtitle: r.item.subtitle.as_deref(),
                icon: r.item.icon.as_deref(),
                tier: r.tier,
                score: r.score,
                positions: &r.positions,
                actions: &r.item.actions,
            })
            .collect();
        let output = JsonOutput {
            query: text,
            results,
            errors: &outcome.errors,
            warnings: &outcome.warnings,
        };
        serde_json::to_writer_pretty(&mut *out, &output)?;
        writeln!(out)?;
        return Ok(());
    }
    for r in &outcome.ranked {
        match &r.item.subtitle {
            Some(subtitle) => writeln!(out, "{}  ({subtitle})  [{}]", r.item.title, r.item.id)?,
            None => writeln!(out, "{}  [{}]", r.item.title, r.item.id)?,
        }
    }
    for line in outcome.errors.iter().chain(&outcome.warnings) {
        // Diagnostics go to stderr so stdout stays the result list.
        let _ = writeln!(io::stderr(), "quickhop: {line}");
    }
    Ok(())
}

struct LaunchRequest<'a> {
    id: &'a str,
    query: &'a str,
    args: &'a [String],
    yes: bool,
    dry_run: bool,
    action: Option<&'a str>,
    paste: bool,
}

fn launch(
    config: &Config,
    request: &LaunchRequest<'_>,
    out: &mut impl Write,
) -> Result<(), CliError> {
    let id = ItemId::parse(request.id).ok_or_else(|| CliError::BadId(request.id.to_owned()))?;
    let outcome = search(config, request.query, usize::MAX)?;
    let item = outcome
        .ranked
        .into_iter()
        .map(|r| r.item)
        .find(|item| item.id == id)
        .ok_or_else(|| CliError::NotFound(request.id.to_owned(), request.query.to_owned()))?;
    let action = match request.action {
        None => item.default_action(),
        Some(kind) => {
            let found = item.actions.iter().find(|a| a.kind() == kind);
            if found.is_none() {
                let kinds: Vec<&str> = item.actions.iter().map(Action::kind).collect();
                return Err(CliError::NoAction {
                    id: request.id.to_owned(),
                    kind: kind.to_owned(),
                    available: kinds.join(", "),
                });
            }
            found
        }
    };
    let Some(action) = action else {
        return Err(CliError::NotFound(
            request.id.to_owned(),
            request.query.to_owned(),
        ));
    };
    let pasteable = matches!(
        action,
        Action::Copy { .. } | Action::Paste { .. } | Action::Snippet { .. }
    );
    if request.paste && !pasteable {
        return Err(CliError::NotPasteable(action.kind()));
    }
    if request.dry_run {
        serde_json::to_writer_pretty(&mut *out, action)?;
        writeln!(out)?;
        return Ok(());
    }
    if action.needs_confirmation() && !request.yes {
        return Err(CliError::NeedsConfirmation(item.title.clone()));
    }
    let message = launch::perform(action, request.args)?;
    if request.paste || matches!(action, Action::Paste { .. }) {
        paste::paste_and_report(&message)?;
    } else if !message.is_empty() {
        writeln!(out, "{message}")?;
    }
    let mut warnings = Vec::new();
    if let Some(Err(error)) =
        open_store(&mut warnings).map(|mut store| store.record(request.query, &id, now()))
    {
        warnings.push(error.to_string());
    }
    for line in warnings {
        let _ = writeln!(io::stderr(), "quickhop: {line}");
    }
    Ok(())
}

/// The commands that run or talk to the daemon: `daemon`, `toggle`, `show`, `hide`, `open`
/// and `reload`. `toggle`, `show` and `open` start a daemon when none runs.
#[cfg(unix)]
fn daemon_command(command: Command, out: &mut impl Write) -> Result<(), CliError> {
    let (request, start) = match command {
        Command::Daemon {
            action,
            no_window,
            test_socket,
            #[cfg(not(target_os = "macos"))]
            notify,
        } => match action {
            #[cfg(not(target_os = "macos"))]
            None if notify => return menu_start(out),
            None => {
                let options = daemon::Options {
                    window: !no_window,
                    test_socket,
                    #[cfg(target_os = "macos")]
                    app: false,
                };
                return Ok(daemon::serve(load_config()?, &options, out)?);
            }
            Some(DaemonAction::Status) => (daemon::Request::Status, false),
            Some(DaemonAction::Stop) => (daemon::Request::Stop, false),
        },
        Command::Toggle => (daemon::Request::Toggle, true),
        Command::Show => (daemon::Request::Show, true),
        Command::Hide => (daemon::Request::Hide, false),
        Command::Onboarding => (daemon::Request::Onboarding, true),
        Command::Reload => (daemon::Request::Reload, false),
        Command::Open {
            id,
            query,
            args,
            yes,
            action,
            paste,
        } => (
            daemon::Request::Open {
                id,
                query,
                args,
                yes,
                action,
                paste,
            },
            true,
        ),
        _ => unreachable!("not a daemon command"),
    };
    let response = if start {
        daemon::send_starting(&request)?
    } else {
        daemon::send(&request)?
    };
    let message = daemon::message(response)?;
    if !message.is_empty() {
        writeln!(out, "{message}")?;
    }
    Ok(())
}

fn run(cli: Cli, out: &mut impl Write) -> Result<(), CliError> {
    match cli.command {
        Command::Query { text, json, limit } => {
            let config = load_config()?;
            let limit = limit.map_or(config.max_results, usize::from);
            query(&config, &text.join(" "), json, limit, out)?;
        }
        Command::Launch {
            id,
            query,
            args,
            yes,
            dry_run,
            action,
            paste,
        } => {
            let request = LaunchRequest {
                id: &id,
                query: &query,
                args: &args,
                yes,
                dry_run,
                action: action.as_deref(),
                paste,
            };
            launch(&load_config()?, &request, out)?;
        }
        Command::Clipboard { action } => clipboard::run(&action, &load_config()?, now_ms, out)?,
        Command::Autostart { action } => autostart::run(action, out)?,
        #[cfg(unix)]
        command @ (Command::Daemon { .. }
        | Command::Toggle
        | Command::Show
        | Command::Hide
        | Command::Onboarding
        | Command::Open { .. }
        | Command::Reload) => daemon_command(command, out)?,
        Command::Config {
            action: ConfigAction::Default,
        } => write!(out, "{}", Config::default_toml())?,
        Command::Config {
            action: ConfigAction::Path,
        } => writeln!(out, "{}", default_path()?.display())?,
        Command::Config {
            action: ConfigAction::Check { file },
        } => {
            let path = match file {
                Some(path) => path,
                None => default_path()?,
            };
            let config = Config::load(&path)?;
            // Command ids are checked against the items the providers load.
            if !config.commands.is_empty() {
                load_search(&config, 1)?;
            }
            writeln!(out, "{}: ok", path.display())?;
        }
        Command::Run {
            script,
            args,
            yes,
            timeout,
        } => {
            let config = load_config()?;
            let found = find_script(&config, &script)?;
            run_script(&found, &args, yes, Duration::from_secs(timeout), out)?;
        }
        Command::Doctor { strict } => doctor::run(strict, out)?,
        Command::Index { watch } => index(watch, out)?,
    }
    Ok(())
}

/// Rebuilds the file index; with `watch`, then keeps it current until killed.
fn index(watch: bool, out: &mut impl Write) -> Result<(), CliError> {
    let config = load_config()?;
    let env = Env::current();
    let options = quickhop_providers::index_options(&config, &env);
    if options.roots.is_empty() {
        return Err(CliError::NoIndexRoots);
    }
    let path = quickhop_providers::index_path(&env)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut index = quickhop_index::Index::open(&path)?;
    let stats = index.rebuild(&options, &Cancel::new())?;
    writeln!(
        out,
        "indexed {} files and {} folders in {} ms: {}",
        stats.files,
        stats.dirs,
        stats.millis,
        path.display()
    )?;
    if !watch {
        return Ok(());
    }
    let roots: Vec<String> = options
        .roots
        .iter()
        .map(|r| r.display().to_string())
        .collect();
    let index = std::sync::Arc::new(std::sync::Mutex::new(index));
    let watcher = quickhop_index::Watcher::start(index, options)?;
    writeln!(out, "watching {} (Ctrl+C stops)", roots.join(", "))?;
    out.flush()?;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        for error in watcher.take_errors() {
            let _ = writeln!(io::stderr(), "quickhop: {error}");
        }
    }
}

/// The script command `name`: an existing file, else the one script command in the script
/// directories whose file name, file stem or title (ignoring case) is `name`.
fn find_script(config: &Config, name: &str) -> Result<ScriptCommand, CliError> {
    let path = Path::new(name);
    if path.is_file() {
        let text = std::fs::read_to_string(path).map_err(|source| CliError::ReadScript {
            path: name.to_owned(),
            source,
        })?;
        return scripts::parse(path, &text).map_err(|source| CliError::BadScript {
            path: name.to_owned(),
            source,
        });
    }
    let dirs = quickhop_providers::script_dirs(config, &Env::current());
    let (found, _warnings) = scripts::discover(&dirs);
    let mut matches: Vec<ScriptCommand> = found
        .into_iter()
        .filter(|s| {
            let file_name = s.path.file_name().map(|n| n.to_string_lossy());
            let stem = s.path.file_stem().map(|n| n.to_string_lossy());
            file_name.as_deref() == Some(name)
                || stem.as_deref() == Some(name)
                || s.title.eq_ignore_ascii_case(name)
        })
        .collect();
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => {
            let searched: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
            let searched = if searched.is_empty() {
                "no script directories".to_owned()
            } else {
                searched.join(", ")
            };
            Err(CliError::ScriptNotFound(name.to_owned(), searched))
        }
        _ => {
            let paths: Vec<String> = matches
                .iter()
                .map(|s| s.path.display().to_string())
                .collect();
            Err(CliError::AmbiguousScript(name.to_owned(), paths.join(", ")))
        }
    }
}

/// Runs `script` with `args` and prints what its mode shows: all output for `fullOutput`,
/// the last line for `compact` and `silent`, the first for `inline`.
fn run_script(
    script: &ScriptCommand,
    args: &[String],
    yes: bool,
    timeout: Duration,
    out: &mut impl Write,
) -> Result<(), CliError> {
    if script.needs_confirmation && !yes {
        return Err(CliError::NeedsConfirmation(script.title.clone()));
    }
    let output = scripts::run(&scripts::action(script), args, timeout)?;
    if !output.success {
        return Err(CliError::ScriptFailed(output.message));
    }
    if !output.message.is_empty() {
        writeln!(out, "{}", output.message)?;
    }
    Ok(())
}

/// `quickhop daemon --notify`, the Linux app-menu entry: starts the daemon in the background
/// unless one runs, and says so in a desktop notification, as there is no window to show yet.
#[cfg(all(unix, not(target_os = "macos")))]
fn menu_start(out: &mut impl Write) -> Result<(), CliError> {
    const USE: &str = "Clipboard history and the file index run in the background. Search \
                       with `quickhop query <text>` in a terminal, or bind a key in your \
                       desktop's keyboard settings to `quickhop open <id>`; the Linux launcher \
                       window is not built yet.";
    let started = match daemon::send(&daemon::Request::Status) {
        Ok(running) => Ok(("quickhop is already running", running.pid)),
        Err(daemon::DaemonError::NotRunning) => daemon::send_starting(&daemon::Request::Status)
            .map(|running| ("quickhop now runs in the background", running.pid)),
        Err(error) => Err(error),
    };
    match started {
        Ok((summary, pid)) => {
            let body = format!("pid {pid}. {USE}");
            notify_desktop(summary, &body);
            writeln!(out, "{summary}: {body}")?;
            Ok(())
        }
        Err(error) => {
            notify_desktop("quickhop could not start", &error.to_string());
            Err(error.into())
        }
    }
}

/// Shows a desktop notification through `notify-send` (libnotify), else through the
/// notification service over `D-Bus` with `gdbus` (`GLib`); says on standard error when
/// neither works.
#[cfg(all(unix, not(target_os = "macos")))]
fn notify_desktop(summary: &str, body: &str) {
    use std::process::{Command, Stdio};

    let ran = |command: &mut Command| {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    if ran(Command::new("notify-send").args([
        "--app-name=quickhop",
        "--icon=quickhop",
        summary,
        body,
    ])) {
        return;
    }
    // gdbus parses each argument as a GVariant: strings go in single quotes.
    let quoted = |text: &str| format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"));
    if ran(Command::new("gdbus").args([
        "call",
        "--session",
        "--dest=org.freedesktop.Notifications",
        "--object-path=/org/freedesktop/Notifications",
        "--method=org.freedesktop.Notifications.Notify",
        "'quickhop'",
        "0",
        "'quickhop'",
        &quoted(summary),
        &quoted(body),
        "[]",
        "{}",
        "-1",
    ])) {
        return;
    }
    warn(&format!(
        "cannot show a desktop notification (neither notify-send nor gdbus reached a \
         notification service): {summary}: {body}"
    ));
}

fn main() -> ExitCode {
    #[cfg(target_os = "macos")]
    if app::opened() {
        migrate_legacy_files();
        return exit_code(app::run(&mut io::stdout().lock()));
    }
    let cli = Cli::parse();
    migrate_legacy_files();
    let mut stdout = io::stdout().lock();
    exit_code(run(cli, &mut stdout))
}

/// Success, or the error reported on standard error and a failure.
fn exit_code(result: Result<(), CliError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Reporting to stderr is the CLI's job; nothing else can be done if this fails.
            let _ = writeln!(io::stderr(), "quickhop: {error}");
            ExitCode::FAILURE
        }
    }
}
