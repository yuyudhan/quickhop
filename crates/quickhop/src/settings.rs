//! quickhop's settings outside `config.toml`'s text (CFG-01): what the settings window shows
//! and changes ([`values`], [`change`], macOS), opening the file in an editor where no window
//! exists yet ([`open_in_editor`], Linux), and About quickhop ([`about`]).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use quickhop_core::Flavour;
#[cfg(target_os = "macos")]
use quickhop_core::{Config, settings::save};
#[cfg(target_os = "macos")]
use quickhop_ui::settings::{Change, Values};

#[cfg(target_os = "macos")]
use crate::autostart::{self, AutostartAction};
use crate::{default_path, var};

/// What a new config file says, so it is not empty when an editor opens it.
const NEW_FILE: &str = "\
# quickhop settings. Every key, with its default: `quickhop config default`.
# What each key does: https://github.com/yuyudhan/quickhop/blob/nightly/docs/configuration.md
# A running quickhop reads this file again when you save it.
";

/// The config file in use.
///
/// # Errors
/// When no location can be derived from the environment.
pub(crate) fn config_file() -> Result<PathBuf, String> {
    default_path().map_err(|e| e.to_string())
}

/// Whether quickhop starts at login, or why that cannot be told.
#[cfg(target_os = "macos")]
fn launch_at_login() -> Result<bool, String> {
    let expected = autostart::this_exe().map_err(|e| e.to_string())?;
    autostart::report(Flavour::CURRENT, std::env::consts::OS, var, &expected)
        .map(|report| report.on)
        .map_err(|e| e.to_string())
}

/// What the settings window shows: the config file's values (the defaults when it does not
/// exist), launch at login and the file's path.
///
/// # Errors
/// When the config file cannot be read or is invalid: the message says which key.
#[cfg(target_os = "macos")]
pub(crate) fn values() -> Result<Values, String> {
    let path = config_file()?;
    let config = if path.exists() {
        Config::load(&path).map_err(|e| format!("{}: {e}", path.display()))?
    } else {
        Config::default()
    };
    Ok(Values {
        config,
        launch_at_login: launch_at_login(),
        config_path: path.display().to_string(),
    })
}

/// Carries out `change`: writes the key into the config file and has the running daemon read
/// it again, registers or removes the login item, or opens the file. Returns the values read
/// back.
///
/// # Errors
/// What failed, for the user; nothing is written when the change would make the file invalid.
#[cfg(target_os = "macos")]
pub(crate) fn change(change: &Change) -> Result<Values, String> {
    let path = config_file()?;
    match change {
        Change::Setting(setting) => {
            save(&path, setting).map_err(|e| e.to_string())?;
            reload()?;
        }
        Change::LaunchAtLogin(on) => {
            let action = if *on {
                // Registered for the next login; this daemon already runs.
                AutostartAction::On {
                    no_start: true,
                    xdg: false,
                }
            } else {
                AutostartAction::Off
            };
            autostart::run(action, &mut std::io::sink()).map_err(|e| e.to_string())?;
        }
        Change::OpenConfigFile => open_in_editor(&path)?,
    }
    values()
}

/// Asks the running daemon to read the config again; a daemon that is not running reads it
/// when it starts.
#[cfg(target_os = "macos")]
fn reload() -> Result<(), String> {
    use crate::daemon::{self, DaemonError, Request};

    match daemon::send(&Request::Reload).map(daemon::message) {
        Ok(Ok(_)) | Err(DaemonError::NotRunning) => Ok(()),
        Ok(Err(error)) | Err(error) => Err(error.to_string()),
    }
}

/// Opens `path` in the default text editor, first writing a short header when it does not
/// exist yet (an editor opens an existing file).
///
/// # Errors
/// When the file cannot be created or no editor starts.
pub(crate) fn open_in_editor(path: &Path) -> Result<(), String> {
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(path, NEW_FILE).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let (program, args) = editor_command(std::env::consts::OS, path);
    let status = Command::new(program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("cannot start {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} could not open {}: open it in your editor",
            path.display()
        ))
    }
}

/// The command that opens `path` in the default text editor on `os`: `open -t` on macOS (a
/// `.toml` file may have no other handler), the desktop's handler elsewhere.
fn editor_command(os: &str, path: &Path) -> (&'static str, Vec<String>) {
    let path = path.display().to_string();
    match os {
        "macos" => ("open", vec!["-t".into(), path]),
        "windows" => (
            "cmd.exe",
            vec!["/C".into(), "start".into(), String::new(), path],
        ),
        _ => ("xdg-open", vec![path]),
    }
}

/// About quickhop: the version, where the settings and data live, the licence and the source.
pub(crate) fn about() -> String {
    let config = config_file().map_or_else(|error| error, |path| path.display().to_string());
    let data = quickhop_core::config::data_dir(Flavour::CURRENT, std::env::consts::OS, var)
        .map_or_else(|error| error.to_string(), |dir| dir.display().to_string());
    format!(
        "quickhop {}\n\nAn open-source Raycast alternative for macOS and Linux.\n\n\
         Settings: {config}\nData:     {data}\n\n\
         MIT OR Apache-2.0 · https://github.com/yuyudhan/quickhop\n",
        crate::VERSION
    )
}

#[cfg(test)]
mod tests {
    use quickhop_core::Config;

    use super::*;

    #[test]
    fn the_editor_is_the_text_editor_on_macos_and_the_handler_elsewhere() {
        let path = Path::new("/h/.config/quickhop/config.toml");
        assert_eq!(
            editor_command("macos", path),
            ("open", vec!["-t".to_owned(), path.display().to_string()])
        );
        assert_eq!(
            editor_command("linux", path),
            ("xdg-open", vec![path.display().to_string()])
        );
    }

    #[test]
    fn about_names_the_version_and_the_files() {
        let text = about();
        assert!(text.starts_with(&format!("quickhop {}", crate::VERSION)));
        assert!(text.contains("Settings: "));
        assert!(text.contains("MIT OR Apache-2.0"));
    }

    #[test]
    fn a_new_config_file_header_is_valid_toml() {
        assert_eq!(Config::from_toml(NEW_FILE).unwrap(), Config::default());
    }
}
