//! The quickhop command-line entry point.

#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use quickhop_core::Config;
use quickhop_core::config::config_path;

/// Keyboard launcher for Linux, macOS and Windows.
#[derive(Debug, Parser)]
#[command(name = "quickhop", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect or validate the configuration file.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Report the platform, config location and enabled providers.
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
}

fn default_path() -> Result<PathBuf, quickhop_core::ConfigError> {
    config_path(std::env::consts::OS, |name| std::env::var(name).ok())
}

fn run(cli: Cli, out: &mut impl Write) -> Result<(), CliError> {
    match cli.command {
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
        Command::Doctor => doctor(out)?,
    }
    Ok(())
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
    writeln!(out, "hotkey: {}", config.hotkey)?;
    let p = &config.providers;
    let enabled = [
        ("apps", p.apps),
        ("files", p.files),
        ("calc", p.calc),
        ("commands", p.commands),
    ];
    for name in quickhop_providers::BUILTIN {
        let on = enabled.iter().any(|(n, e)| *n == name && *e);
        writeln!(
            out,
            "provider {name}: {}",
            if on { "enabled" } else { "disabled" }
        )?;
    }
    writeln!(out, "ui: toolkit not chosen yet")?;
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
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
