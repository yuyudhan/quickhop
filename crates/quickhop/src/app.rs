//! quickhop.app (plan slice U4). Opened from Finder, Spotlight, Launchpad or `open`, the
//! bundle's executable runs without arguments and is then the app: the daemon with the
//! launcher window and its global hotkey, in this very process, so macOS knows it as
//! quickhop.app (its icon, and its own Accessibility permission for pasting). Opening it shows
//! the launcher, or the onboarding window while quickhop may not paste; when this same app
//! runs the daemon already, that daemon is asked to do so and this process ends. A daemon
//! running from another executable (a bare `quickhop` that `autostart` or a terminal started,
//! or another version) is stopped and this process takes over: its Accessibility grant is not
//! the app's, so pasting would fail however the user answers the onboarding window.
//!
//! The app has no terminal, Dock icon or menu, so a failure is shown in an alert and appended
//! to `~/Library/Logs/quickhop.log` (`quickhop-dev.log` for a development build), besides
//! standard error.

use std::ffi::OsString;
use std::io::{IsTerminal as _, Write};
use std::path::{Path, PathBuf};

use quickhop_core::Flavour;

use crate::daemon::{self, DaemonError, Request, Response};
use crate::{CliError, load_config};

/// The title of the alert a failure shows.
const FAILURE_TITLE: &str = "quickhop could not start";

/// Set (to anything) by the tests: a failure goes to standard error and the log only, never
/// to an alert on the screen of whoever runs them.
const NO_ALERT: &str = "QUICKHOP_NO_ALERT";

/// Whether this process is quickhop.app being opened (see [`is_app_launch`]).
pub(crate) fn opened() -> bool {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    std::env::current_exe().is_ok_and(|exe| is_app_launch(&exe, &args))
}

/// Whether a process started from the executable `exe`, as the system reports its path, with
/// `args` after the program name, is an app being opened: the executable of a bundle
/// (`<name>.app/Contents/MacOS/<file>`) started without arguments. A symlink to that
/// executable on the `PATH` keeps its own path, so a bare `quickhop` there still prints the
/// usage.
pub(crate) fn is_app_launch(exe: &Path, args: &[OsString]) -> bool {
    args.is_empty() && in_bundle(exe)
}

/// Whether `exe` is the executable of a bundle: `<name>.app/Contents/MacOS/<file>`.
fn in_bundle(exe: &Path) -> bool {
    let Some(macos) = exe.parent() else {
        return false;
    };
    let Some(contents) = macos.parent() else {
        return false;
    };
    macos.file_name().is_some_and(|name| name == "MacOS")
        && contents.file_name().is_some_and(|name| name == "Contents")
        && contents
            .parent()
            .and_then(Path::extension)
            .is_some_and(|extension| extension == "app")
}

/// The bundle identifier macOS checks this process's Accessibility grant against: the app's,
/// when this process runs quickhop.app's executable on its own (opened, or started by its
/// `LaunchAgent`). `None` when a terminal started it (the terminal's grant covers it) or it
/// runs outside a bundle (macOS checks that binary instead).
pub(crate) fn own_bundle_id() -> Option<&'static str> {
    if std::io::stderr().is_terminal() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    in_bundle(&exe).then_some(crate::autostart::LABEL)
}

/// Whether the app, running from `exe` at `version`, takes over from the daemon that answered
/// `running`: unless that daemon is this very program, same executable and version. A daemon
/// too old to say counts as another one. Paths are compared as given: the caller resolves
/// symlinks.
fn takes_over(running: &Response, exe: &Path, version: &str) -> bool {
    running.exe.as_deref() != Some(exe) || running.version.as_deref() != Some(version)
}

/// `path` with its symlinks resolved, or as it is when it cannot be (it is gone).
fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Runs quickhop.app: asks a running daemon of this same app to show its window, else becomes
/// the daemon (stopping one that runs from another executable first), with the launcher window
/// shown (or the onboarding window), until `quickhop daemon stop`. A failure is also shown in
/// an alert and logged, as nothing else of the app is on screen.
///
/// # Errors
/// When the running daemon fails the request or does not stop, or the configuration or the
/// daemon cannot be set up.
pub(crate) fn run(out: &mut impl Write) -> Result<(), CliError> {
    let result = serve(out);
    if let Err(error) = &result {
        report(error);
    }
    result
}

fn serve(out: &mut impl Write) -> Result<(), CliError> {
    match daemon::send(&Request::Status) {
        Ok(running) => {
            let exe = resolved(&std::env::current_exe()?);
            let version = env!("CARGO_PKG_VERSION");
            let running_exe = running.exe.as_deref().map(resolved);
            let running = Response {
                exe: running_exe,
                ..running
            };
            if !takes_over(&running, &exe, version) {
                daemon::message(daemon::send(&Request::AppOpened)?)?;
                return Ok(());
            }
            log(&format!(
                "stopping the quickhop daemon (pid {}, {}, version {}) to run it as {}, \
                 version {version}",
                running.pid,
                running
                    .exe
                    .as_deref()
                    .map_or_else(|| "executable unknown".into(), Path::to_string_lossy),
                running.version.as_deref().unwrap_or("unknown"),
                exe.display(),
            ));
            daemon::stop_and_wait(running.pid)?;
        }
        Err(DaemonError::NotRunning) => {}
        Err(error) => return Err(error.into()),
    }
    let options = daemon::Options {
        window: true,
        test_socket: None,
        app: true,
    };
    Ok(daemon::serve(load_config()?, &options, out)?)
}

/// What the alert says about `error`: the `quickhop:` line as the command line prints it,
/// then, for a configuration error, which file it is in, and where the log is.
fn failure_text(error: &CliError, config: Option<&Path>, log: Option<&Path>) -> String {
    let mut parts = vec![format!("quickhop: {error}")];
    if let (CliError::Config(_), Some(config)) = (error, config) {
        parts.push(format!("In the configuration file {}.", config.display()));
    }
    if let Some(log) = log {
        parts.push(format!("This is also in {}.", log.display()));
    }
    parts.join("\n\n")
}

/// `flavour`'s app log: `~/Library/Logs/quickhop.log` or `quickhop-dev.log`, where Console
/// finds it.
fn log_path(flavour: Flavour, home: &Path) -> PathBuf {
    home.join("Library/Logs")
        .join(format!("{}.log", flavour.stem()))
}

/// Appends `line`, timestamped, to the log file at `path`, creating it.
///
/// # Errors
/// When the log cannot be written.
fn append_log(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(
        file,
        "{} {}.app: {line}",
        jiff::Timestamp::now(),
        Flavour::CURRENT.app_name()
    )
}

/// The log under this user's `HOME`, when it is set.
fn user_log() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| log_path(Flavour::CURRENT, Path::new(&home)))
}

/// Appends `line` to the user's log; a log that cannot be written is reported on standard
/// error, the one other place left.
fn log(line: &str) {
    if let Some(path) = user_log()
        && let Err(error) = append_log(&path, line)
    {
        crate::warn(&format!("cannot write {}: {error}", path.display()));
    }
}

/// Logs `error` and shows it in an alert, waiting until it is dismissed (unless [`NO_ALERT`]
/// is set).
fn report(error: &CliError) {
    let log_file = user_log();
    log(&error.to_string());
    if std::env::var_os(NO_ALERT).is_some() {
        return;
    }
    let config = crate::default_path().ok();
    let text = failure_text(error, config.as_deref(), log_file.as_deref());
    // The text goes in as arguments of the script, never into its source.
    let shown = std::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "activate",
            "-e",
            "display alert (item 1 of argv) message (item 2 of argv) as critical",
            "-e",
            "end run",
            FAILURE_TITLE,
            &text,
        ])
        .output();
    match shown {
        Ok(output) if output.status.success() => {}
        Ok(output) => crate::warn(&format!(
            "cannot show the alert ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => crate::warn(&format!("cannot run osascript: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundle_executable_without_arguments_is_the_app() {
        let exe = Path::new("/Applications/quickhop.app/Contents/MacOS/quickhop");
        assert!(is_app_launch(exe, &[]));
        // Another name and place: any bundle's executable.
        let exe = Path::new("/Users/me/Apps/My Launcher.app/Contents/MacOS/launcher");
        assert!(is_app_launch(exe, &[]));
    }

    #[test]
    fn arguments_make_it_the_command_line() {
        let exe = Path::new("/Applications/quickhop.app/Contents/MacOS/quickhop");
        assert!(!is_app_launch(exe, &["daemon".into()]));
        assert!(!is_app_launch(exe, &["--help".into()]));
    }

    #[test]
    fn executables_outside_a_bundle_are_the_command_line() {
        for exe in [
            "/Users/me/.local/bin/quickhop",
            "/Applications/quickhop.app/quickhop",
            "/Applications/quickhop/Contents/MacOS/quickhop",
            "/Applications/quickhop.app/Contents/Resources/quickhop",
            "/opt/quickhop.app/MacOS/quickhop",
            "quickhop",
            "/",
        ] {
            assert!(!is_app_launch(Path::new(exe), &[]), "{exe}");
        }
    }

    const APP: &str = "/Applications/quickhop.app/Contents/MacOS/quickhop";

    fn running(exe: Option<&str>, version: Option<&str>) -> Response {
        Response {
            ok: true,
            pid: 42,
            message: "running".into(),
            version: version.map(str::to_owned),
            exe: exe.map(PathBuf::from),
            may_paste: None,
        }
    }

    #[test]
    fn the_app_hands_over_to_a_daemon_of_its_own_executable_and_version() {
        let own = running(Some(APP), Some("1.2.3"));
        assert!(!takes_over(&own, Path::new(APP), "1.2.3"));
    }

    #[test]
    fn the_app_takes_over_from_a_bare_binary_another_version_or_an_older_daemon() {
        let exe = Path::new(APP);
        for other in [
            running(Some("/Users/me/.cargo/bin/quickhop"), Some("1.2.3")),
            running(Some(APP), Some("1.2.2")),
            running(Some(APP), None),
            running(None, Some("1.2.3")),
            running(None, None),
        ] {
            assert!(takes_over(&other, exe, "1.2.3"), "{other:?}");
        }
    }

    #[test]
    fn a_configuration_failure_names_the_file_and_the_log() {
        let error = CliError::Config(quickhop_core::ConfigError::Invalid(
            "max_results must be between 1 and 200, got 0".into(),
        ));
        let config = Path::new("/Users/me/.config/quickhop/config.toml");
        let log = Path::new("/Users/me/Library/Logs/quickhop.log");
        let text = failure_text(&error, Some(config), Some(log));
        assert!(
            text.starts_with("quickhop: invalid config: max_results"),
            "{text}"
        );
        assert!(text.contains(&config.display().to_string()), "{text}");
        assert!(text.contains(&log.display().to_string()), "{text}");
    }

    #[test]
    fn a_parse_failure_keeps_the_line_toml_names() {
        let error =
            CliError::Config(quickhop_core::Config::from_toml("max_results = \"x\"").unwrap_err());
        let text = failure_text(&error, Some(Path::new("/c/config.toml")), None);
        assert!(text.contains("line 1"), "{text}");
        assert!(text.contains("/c/config.toml"), "{text}");
    }

    #[test]
    fn other_failures_name_no_configuration_file() {
        let error = CliError::Daemon(DaemonError::Failed("the request thread panicked".into()));
        let text = failure_text(&error, Some(Path::new("/c/config.toml")), None);
        assert_eq!(text, "quickhop: the request thread panicked");
    }

    #[test]
    fn the_log_is_in_the_users_library_logs() {
        assert_eq!(
            log_path(Flavour::Release, Path::new("/Users/me")),
            Path::new("/Users/me/Library/Logs/quickhop.log")
        );
        assert_eq!(
            log_path(Flavour::Dev, Path::new("/Users/me")),
            Path::new("/Users/me/Library/Logs/quickhop-dev.log")
        );
    }

    #[test]
    fn log_lines_are_appended_with_a_timestamp() {
        let home = tempfile::tempdir().unwrap();
        let path = log_path(Flavour::CURRENT, home.path());
        append_log(&path, "first").unwrap();
        append_log(&path, "second").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        let app = format!(" {}.app: ", Flavour::CURRENT.app_name());
        assert!(lines[0].ends_with(&format!("{app}first")), "{text}");
        assert!(lines[1].ends_with(&format!("{app}second")), "{text}");
        let stamp = lines[0].split(' ').next().unwrap();
        assert!(stamp.parse::<jiff::Timestamp>().is_ok(), "{stamp}");
    }
}
