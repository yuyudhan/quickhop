//! Performs an item's action for `quickhop launch`.

use std::cell::Cell;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use quickhop_core::placeholders::{self, Context};
use quickhop_core::snippets::{SnippetsFile, snippets_path};
use quickhop_core::{Action, Flavour, QuickhopCommand};
use quickhop_providers::scripts;

use crate::trash;

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
    /// Moving a file to the trash failed.
    #[error("cannot move {path} to the trash: {reason}")]
    Trash {
        /// The file.
        path: String,
        /// What went wrong.
        reason: String,
    },
    /// The action opens a list in the launcher window, which the command line cannot show.
    #[error("this opens a list in the launcher window: run `quickhop toggle` and pick it there")]
    View,
    /// One of quickhop's own commands failed (the daemon could not be reached, …).
    #[error("{0}")]
    Quickhop(String),
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

/// Runs `program` to completion; a non-zero exit is an error.
fn run_waiting(program: &str, args: &[String]) -> Result<(), LaunchError> {
    let status = spawn(program, args)?
        .wait()
        .map_err(|source| LaunchError::Spawn {
            program: program.to_owned(),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(LaunchError::Failed(program.to_owned()))
    }
}

/// Shows `path` selected in the file manager: Finder, Explorer, or on Linux whatever owns
/// `org.freedesktop.FileManager1` on the session bus (Nautilus, Dolphin, Nemo, …), else the
/// parent folder opened with the default handler.
fn reveal(path: &str) -> Result<(), LaunchError> {
    match std::env::consts::OS {
        "macos" => run_waiting("open", &["-R".into(), path.into()]),
        // Explorer exits with 1 even when it worked.
        "windows" => spawn("explorer.exe", &[format!("/select,{path}")]).map(drop),
        _ => {
            let uri = trash::file_uri(Path::new(path));
            let args = [
                "call",
                "--session",
                "--dest",
                "org.freedesktop.FileManager1",
                "--object-path",
                "/org/freedesktop/FileManager1",
                "--method",
                "org.freedesktop.FileManager1.ShowItems",
                // The escaped URI holds no quote or backslash.
                &format!("['{uri}']"),
                // An empty startup id, in GVariant text form.
                "''",
            ];
            let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
            if run_waiting("gdbus", &args).is_ok() {
                return Ok(());
            }
            let parent = Path::new(path).parent().unwrap_or(Path::new(path));
            let (program, opener_args) = opener(&parent.to_string_lossy());
            run_waiting(&program, &opener_args)
        }
    }
}

/// Moves `path` to the trash: Finder on macOS, the Recycle Bin on Windows, the home trash
/// (FreeDesktop.org specification) elsewhere, with `gio trash` for files on other
/// filesystems.
fn move_to_trash(path: &str) -> Result<(), LaunchError> {
    let failed = |reason: String| LaunchError::Trash {
        path: path.to_owned(),
        reason,
    };
    match std::env::consts::OS {
        "macos" => {
            let quoted = path.replace('\\', "\\\\").replace('"', "\\\"");
            let script = format!("tell application \"Finder\" to delete (POSIX file \"{quoted}\")");
            run_waiting("osascript", &["-e".into(), script]).map_err(|e| failed(e.to_string()))
        }
        "windows" => {
            let quoted = path.replace('\'', "''");
            let method = if Path::new(path).is_dir() {
                "DeleteDirectory"
            } else {
                "DeleteFile"
            };
            let command = format!(
                "Add-Type -AssemblyName Microsoft.VisualBasic; \
                 [Microsoft.VisualBasic.FileIO.FileSystem]::{method}('{quoted}', \
                 'OnlyErrorDialogs', 'SendToRecycleBin')"
            );
            run_waiting(
                "powershell.exe",
                &["-NoProfile".into(), "-Command".into(), command],
            )
            .map_err(|e| failed(e.to_string()))
        }
        _ => {
            let file = Path::new(path);
            if !file.is_absolute() {
                return Err(failed("not an absolute path".into()));
            }
            let trash = trash::home_trash(var)
                .ok_or_else(|| failed("neither XDG_DATA_HOME nor HOME is set".into()))?;
            let deleted = jiff::Zoned::now().strftime("%Y-%m-%dT%H:%M:%S").to_string();
            match trash::move_to(&trash, file, &deleted) {
                Ok(_) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Err(failed(error.to_string()))
                }
                // Most likely another filesystem, which has its own trash folder.
                Err(error) => run_waiting("gio", &["trash".into(), path.into()])
                    .map_err(|_| failed(error.to_string())),
            }
        }
    }
}

/// Performs `action` and returns what to print: the copied or pasted text (the caller puts
/// it on the clipboard), or a script's output shaped for its mode.
pub(crate) fn perform(action: &Action, args: &[String]) -> Result<String, LaunchError> {
    match action {
        Action::Open { target } => {
            let (program, opener_args) = opener(target);
            run_waiting(&program, &opener_args)?;
            Ok(String::new())
        }
        Action::Reveal { path } => {
            reveal(path)?;
            Ok(String::new())
        }
        Action::Trash { path } => {
            move_to_trash(path)?;
            Ok(format!("moved {path} to the trash"))
        }
        Action::Copy { text } | Action::Paste { text } => Ok(text.clone()),
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
        Action::View { .. } => Err(LaunchError::View),
        Action::Quickhop { command } => own(*command),
    }
}

/// Runs one of quickhop's own commands from the command line: the daemon opens the settings,
/// reloads or stops; About is printed.
#[cfg(unix)]
fn own(command: QuickhopCommand) -> Result<String, LaunchError> {
    use crate::daemon::{self, DaemonError, Request};

    let failed = |error: DaemonError| LaunchError::Quickhop(error.to_string());
    let answer = match command {
        QuickhopCommand::About => return Ok(crate::settings::about()),
        QuickhopCommand::Settings => daemon::send_starting(&Request::Settings),
        QuickhopCommand::Reload => daemon::send(&Request::Reload),
        QuickhopCommand::Quit => match daemon::send(&Request::Stop) {
            Err(DaemonError::NotRunning) => return Ok("quickhop is not running".into()),
            answer => answer,
        },
    };
    answer.and_then(daemon::message).map_err(failed)
}

/// Runs one of quickhop's own commands where no daemon runs: the settings are the config file
/// in the editor; About is printed.
#[cfg(not(unix))]
fn own(command: QuickhopCommand) -> Result<String, LaunchError> {
    match command {
        QuickhopCommand::About => Ok(crate::settings::about()),
        QuickhopCommand::Settings => {
            let path = crate::settings::config_file().map_err(LaunchError::Quickhop)?;
            crate::settings::open_in_editor(&path).map_err(LaunchError::Quickhop)?;
            Ok(format!("opened {}", path.display()))
        }
        QuickhopCommand::Reload | QuickhopCommand::Quit => Err(LaunchError::Quickhop(
            "no quickhop daemon runs on this system".into(),
        )),
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The clipboard text, if the clipboard holds text: the pasteboard on macOS, else the
/// platform's helper tool, if one is installed.
pub(crate) fn clipboard_text() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use quickhop_core::{ClipboardSource as _, Contents};
        match crate::paste::pasteboard().contents() {
            Contents::Text { text, .. } if !text.is_empty() => Some(text),
            Contents::Text { .. } | Contents::Secret | Contents::Other => None,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let (program, args): (&str, &[&str]) = match std::env::consts::OS {
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
    let library = snippets_path(Flavour::CURRENT, std::env::consts::OS, var)
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
