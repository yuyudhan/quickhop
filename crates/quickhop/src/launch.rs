//! Performs an item's action for `quickhop launch`.

use std::process::{Command, Stdio};
use std::time::Duration;

use quickhop_core::Action;
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
