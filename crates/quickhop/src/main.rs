//! The quickhop command-line entry point.

#![forbid(unsafe_code)]

mod launch;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use quickhop_core::config::{config_path, data_dir, legacy_macos_dir, migrate_legacy};
use quickhop_core::{Action, Boosts, Cancel, Config, ItemId, Query, Ranked, Search, Tier};
use quickhop_providers::scripts::{self, ScriptCommand};
use quickhop_providers::{Env, Warning};
use quickhop_store::Store;
use serde::Serialize;

/// Keyboard launcher for macOS and Linux (and Windows).
#[derive(Debug, Parser)]
#[command(name = "quickhop", version, about)]
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
    /// Report the platform, paths, session, helper tools and enabled providers.
    Doctor,
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
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn default_path() -> Result<PathBuf, quickhop_core::ConfigError> {
    config_path(std::env::consts::OS, var)
}

/// Moves the config and frecency database from quickhop 0.1's macOS location
/// (`~/Library/Application Support/quickhop`) to the XDG ones, once. Locations overridden
/// with `QUICKHOP_CONFIG` / `QUICKHOP_DATA` are left alone. Problems are reported, not fatal.
fn migrate_legacy_files() {
    let Some(legacy) = legacy_macos_dir(std::env::consts::OS, var) else {
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
        .then(|| data_dir(std::env::consts::OS, var))
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

/// The frecency store in the data directory; problems become warnings and the search
/// runs without frecency.
fn open_store(warnings: &mut Vec<String>) -> Option<Store> {
    let opened = data_dir(std::env::consts::OS, |name| std::env::var(name).ok())
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

struct Outcome {
    ranked: Vec<Ranked>,
    errors: Vec<String>,
    warnings: Vec<String>,
}

fn search(config: &Config, text: &str, limit: usize) -> Outcome {
    let env = Env::current();
    let (providers, loaded) = quickhop_providers::builtin(config, &env);
    let mut warnings: Vec<String> = loaded.iter().map(warning_text).collect();
    let boosts = match open_store(&mut warnings).map(|s| s.boosts(text, now())) {
        Some(Ok(boosts)) => boosts,
        Some(Err(error)) => {
            warnings.push(error.to_string());
            Boosts::new()
        }
        None => Boosts::new(),
    };
    let results = Search::new(limit).with_providers(providers).run(
        &Query::new(text),
        &Cancel::new(),
        &boosts,
    );
    Outcome {
        ranked: results.ranked,
        errors: results.errors.iter().map(ToString::to_string).collect(),
        warnings,
    }
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
    let outcome = search(config, text, limit);
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
}

fn launch(
    config: &Config,
    request: &LaunchRequest<'_>,
    out: &mut impl Write,
) -> Result<(), CliError> {
    let id = ItemId::parse(request.id).ok_or_else(|| CliError::BadId(request.id.to_owned()))?;
    let outcome = search(config, request.query, usize::MAX);
    let item = outcome
        .ranked
        .into_iter()
        .map(|r| r.item)
        .find(|item| item.id == id)
        .ok_or_else(|| CliError::NotFound(request.id.to_owned(), request.query.to_owned()))?;
    let Some(action) = item.default_action() else {
        return Err(CliError::NotFound(
            request.id.to_owned(),
            request.query.to_owned(),
        ));
    };
    if request.dry_run {
        serde_json::to_writer_pretty(&mut *out, action)?;
        writeln!(out)?;
        return Ok(());
    }
    if action.needs_confirmation() && !request.yes {
        return Err(CliError::NeedsConfirmation(item.title.clone()));
    }
    let message = launch::perform(action, request.args)?;
    if !message.is_empty() {
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
        } => {
            let request = LaunchRequest {
                id: &id,
                query: &query,
                args: &args,
                yes,
                dry_run,
            };
            launch(&load_config()?, &request, out)?;
        }
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
            Config::load(&path)?;
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
        Command::Doctor => doctor(out)?,
    }
    Ok(())
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

/// Whether `program` is an executable file in one of `env`'s `PATH` directories.
fn on_path(env: &Env, program: &str) -> bool {
    env.search_path()
        .iter()
        .any(|dir| dir.join(program).is_file())
}

/// The Linux graphical session: Wayland, X11 or none, from the standard variables.
fn session(env: &Env) -> &'static str {
    match (env.var("WAYLAND_DISPLAY"), env.var("DISPLAY")) {
        (Some(_), _) => "wayland",
        (None, Some(_)) => "x11",
        (None, None) => "none (no WAYLAND_DISPLAY or DISPLAY)",
    }
}

/// External programs quickhop uses on `os`: opening targets, the clipboard, power commands.
fn helper_tools(os: &str) -> &'static [&'static str] {
    match os {
        "macos" => &["open", "pbcopy", "pbpaste", "osascript", "pmset"],
        "windows" => &["powershell.exe"],
        _ => &[
            "xdg-open",
            "wl-copy",
            "wl-paste",
            "xclip",
            "loginctl",
            "systemctl",
        ],
    }
}

fn doctor(out: &mut impl Write) -> Result<(), CliError> {
    writeln!(out, "quickhop {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(
        out,
        "platform: {}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )?;
    let config = match default_path() {
        Ok(path) if path.exists() => match Config::load(&path) {
            Ok(config) => {
                writeln!(out, "config: {} (ok)", path.display())?;
                config
            }
            Err(error) => {
                writeln!(out, "config: {} ({error})", path.display())?;
                Config::default()
            }
        },
        Ok(path) => {
            writeln!(out, "config: {} (absent, using defaults)", path.display())?;
            Config::default()
        }
        Err(error) => {
            writeln!(out, "config: {error} (using defaults)")?;
            Config::default()
        }
    };
    match data_dir(std::env::consts::OS, var) {
        Ok(dir) => writeln!(out, "data: {}", dir.display())?,
        Err(error) => writeln!(out, "data: {error} (frecency disabled)")?,
    }
    let env = Env::current();
    let dirs = quickhop_providers::script_dirs(&config, &env);
    if dirs.is_empty() {
        match quickhop_core::config::scripts_dir(&env.os, var) {
            Ok(dir) => writeln!(out, "scripts: none (create {})", dir.display())?,
            Err(error) => writeln!(out, "scripts: {error}")?,
        }
    }
    for dir in &dirs {
        writeln!(out, "scripts: {}", dir.display())?;
    }
    if env.os != "macos" && env.os != "windows" {
        writeln!(out, "session: {}", session(&env))?;
    }
    for tool in helper_tools(&env.os) {
        let state = if on_path(&env, tool) {
            "found"
        } else {
            "missing"
        };
        writeln!(out, "tool {tool}: {state}")?;
    }
    writeln!(out, "hotkey: {}", config.hotkey)?;
    let (providers, warnings) = quickhop_providers::builtin(&config, &env);
    let cancel = Cancel::new();
    for name in quickhop_providers::BUILTIN {
        match providers.iter().find(|p| p.name() == name) {
            Some(provider) => {
                // A one-letter query: providers that hide everything for an empty query
                // (emoji) still report their size.
                let count = provider
                    .items(&Query::new("a"), &cancel)
                    .map_or(0, |items| items.len());
                writeln!(out, "provider {name}: enabled ({count} items)")?;
            }
            None => writeln!(out, "provider {name}: disabled")?,
        }
    }
    for warning in &warnings {
        writeln!(out, "warning: {}", warning_text(warning))?;
    }
    writeln!(
        out,
        "ui: launcher window not built yet (gpui, see docs/adr/0001-ui-toolkit.md)"
    )?;
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    migrate_legacy_files();
    let mut stdout = io::stdout().lock();
    match run(cli, &mut stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Reporting to stderr is the CLI's job; nothing else can be done if this fails.
            let _ = writeln!(io::stderr(), "quickhop: {error}");
            ExitCode::FAILURE
        }
    }
}
