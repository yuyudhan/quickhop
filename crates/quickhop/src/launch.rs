//! Performs an item's action for `quickhop launch`.

use std::cell::Cell;
use std::process::{Command, Stdio};
use std::time::Duration;

use quickhop_core::Action;
use quickhop_core::placeholders::{self, Context};
use quickhop_core::snippets::{SnippetsFile, snippets_path};
use quickhop_providers::scripts;

/// How long a script command may run from the CLI.
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(60);

/// Why an action could not be performed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LaunchError {
    /// The program could not be started.
    #[error("cannot start {program}: {source}")]
    Spawn {
        /// What was started.
        program: String,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The opener exited with an error.
    #[error("{0} failed")]
    Failed(String),
    /// A script command failed.
    #[error(transparent)]
    Script(#[from] scripts::RunError),
    /// A script ran but exited unsuccessfully.
    #[error("script failed: {0}")]
    ScriptFailed(String),
    /// A snippet needs argument values that were not given.
    #[error("snippet needs {0}: pass them with --arg, in order")]
    MissingArguments(String),
    /// A value is not among an argument's `options`.
    #[error("{value:?} is not one of {options}")]
    BadOption {
        /// The value given.
        value: String,
        /// The allowed values, comma separated.
        options: String,
    },
    /// The OS random source failed while making a `{uuid}`.
    #[error("cannot generate a UUID: {0}")]
    Random(String),
}

/// The command that opens `target` with the system handler.
fn opener(target: &str) -> (String, Vec<String>) {
    match std::env::consts::OS {
        "macos" => ("open".into(), vec![target.into()]),
        "windows" => (
            "cmd.exe".into(),
            vec!["/C".into(), "start".into(), String::new(), target.into()],
        ),
        _ => ("xdg-open".into(), vec![target.into()]),
    }
}

fn spawn(program: &str, args: &[String]) -> Result<std::process::Child, LaunchError> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| LaunchError::Spawn {
            program: program.to_owned(),
            source,
        })
}

/// Performs `action` and returns what to print: the copied text (the CLI has no clipboard
/// yet), or a script's output shaped for its mode.
pub(crate) fn perform(action: &Action, args: &[String]) -> Result<String, LaunchError> {
    match action {
        Action::Open { target } => {
            let (program, opener_args) = opener(target);
            let status =
                spawn(&program, &opener_args)?
                    .wait()
                    .map_err(|source| LaunchError::Spawn {
                        program: program.clone(),
                        source,
                    })?;
            if status.success() {
                Ok(String::new())
            } else {
                Err(LaunchError::Failed(program))
            }
        }
        Action::Copy { text } => Ok(text.clone()),
        Action::Snippet { text } => expand_snippet(text, args),
        Action::Run { program, args, .. } => {
            // Detached: the launched program outlives the CLI.
            spawn(program, args)?;
            Ok(String::new())
        }
        Action::Script { .. } => {
            let output = scripts::run(action, args, SCRIPT_TIMEOUT)?;
            if output.success {
                Ok(output.message)
            } else {
                Err(LaunchError::ScriptFailed(output.message))
            }
        }
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The clipboard text from the platform's helper tool, if any is installed and the
/// clipboard holds text.
fn clipboard_text() -> Option<String> {
    let (program, args): (&str, &[&str]) = match std::env::consts::OS {
        "macos" => ("pbpaste", &[]),
        "windows" => (
            "powershell.exe",
            &["-NoProfile", "-Command", "Get-Clipboard"],
        ),
        _ if var("WAYLAND_DISPLAY").is_some() => ("wl-paste", &["--no-newline"]),
        _ if var("DISPLAY").is_some() => ("xclip", &["-selection", "clipboard", "-o"]),
        _ => return None,
    };
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let text = if std::env::consts::OS == "windows" {
        text.trim_end_matches(['\r', '\n']).to_owned()
    } else {
        text
    };
    (output.status.success() && !text.is_empty()).then_some(text)
}

/// Expands a snippet's placeholders with the current time, the clipboard, the snippet
/// library next to the config and `args` as its `{argument}` values, in order.
fn expand_snippet(template: &str, args: &[String]) -> Result<String, LaunchError> {
    let wanted = placeholders::arguments(template);
    let mut missing = Vec::new();
    for (index, argument) in wanted.iter().enumerate() {
        let value = args.get(index).filter(|v| !v.is_empty());
        match value {
            None if argument.is_required() => missing.push(
                argument
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("argument {}", index + 1)),
            ),
            Some(value) if !argument.options.is_empty() && !argument.options.contains(value) => {
                return Err(LaunchError::BadOption {
                    value: value.clone(),
                    options: argument.options.join(", "),
                });
            }
            _ => {}
        }
    }
    if !missing.is_empty() {
        return Err(LaunchError::MissingArguments(missing.join(", ")));
    }
    let library = snippets_path(std::env::consts::OS, var)
        .ok()
        .and_then(|path| SnippetsFile::load(&path).ok())
        .unwrap_or_default();
    let snippet = |name: &str| library.find(name).map(|s| s.text.clone());
    let failure = Cell::new(None);
    let uuid = || {
        let mut bytes = [0u8; 16];
        if let Err(error) = getrandom::fill(&mut bytes) {
            failure.set(Some(error.to_string()));
        }
        placeholders::uuid_v4(bytes)
    };
    let now = jiff::Zoned::now();
    let clipboard: Vec<String> = clipboard_text().into_iter().collect();
    let context = Context {
        now: &now,
        clipboard: &clipboard,
        selection: None,
        arguments: args,
        snippet: &snippet,
        uuid: &uuid,
    };
    let expansion = placeholders::expand(template, &context);
    match failure.into_inner() {
        Some(error) => Err(LaunchError::Random(error)),
        None => Ok(expansion.text),
    }
}
