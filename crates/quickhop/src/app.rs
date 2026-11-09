//! quickhop.app (plan slice U4). Opened from Finder, Spotlight, Launchpad or `open`, the
//! bundle's executable runs without arguments and is then the app: the daemon with the
//! launcher window and its global hotkey, in this very process, so macOS knows it as
//! quickhop.app (its icon, and its own Accessibility permission for pasting). Opening it shows
//! the launcher, or the onboarding window while quickhop may not paste; when a daemon runs
//! already, that daemon is asked to do so and this process ends.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use crate::daemon::{self, DaemonError, Request};
use crate::{CliError, load_config};

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
    let Some(macos) = exe.parent() else {
        return false;
    };
    let Some(contents) = macos.parent() else {
        return false;
    };
    args.is_empty()
        && macos.file_name().is_some_and(|name| name == "MacOS")
        && contents.file_name().is_some_and(|name| name == "Contents")
        && contents
            .parent()
            .and_then(Path::extension)
            .is_some_and(|extension| extension == "app")
}

/// Runs quickhop.app: asks a running daemon to show its window, else becomes the daemon, with
/// the launcher window shown (or the onboarding window), until `quickhop daemon stop`.
///
/// # Errors
/// When the running daemon fails the request, or the configuration or the daemon cannot be
/// set up.
pub(crate) fn run(out: &mut impl Write) -> Result<(), CliError> {
    match daemon::send(&Request::AppOpened) {
        Ok(response) => {
            daemon::message(response)?;
            return Ok(());
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
}
