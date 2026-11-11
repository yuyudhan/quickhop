//! `quickhop autostart on|off|status`: start the daemon at login, through an XDG autostart
//! entry on Linux and the BSDs (honoured by GNOME, KDE, Xfce, Cinnamon, MATE, and turned into
//! a systemd user unit by `systemd-xdg-autostart-generator`) or a `LaunchAgent` on macOS, and
//! start it right away.
//!
//! On macOS the entry runs quickhop.app's executable when an app is installed, so the daemon
//! holds the app's Accessibility permission, not that of a bare `quickhop` on the `PATH`.
//! sway, i3, Hyprland and river read no XDG autostart entries: there `autostart on` prints the
//! line for the compositor's own configuration instead of claiming success.
//!
//! Each [`Flavour`] has its own entry (`quickhop.desktop` or `quickhop-dev.desktop`, the
//! `LaunchAgent` `io.github.yuyudhan.quickhop` or `io.github.yuyudhan.quickhop.dev`) and app
//! ("quickhop.app" or "quickhop Dev.app"), so switching a development build's autostart on
//! or off never touches the release's.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use quickhop_core::Flavour;

/// This build's `LaunchAgent` label, also the reverse-DNS id of its app.
#[cfg(unix)]
pub(crate) const LABEL: &str = Flavour::CURRENT.bundle_id();

/// What `quickhop autostart` does.
#[derive(Debug, Clone, Copy, clap::Subcommand)]
pub(crate) enum AutostartAction {
    /// Start the daemon at every login, and now: from quickhop.app when one is installed in
    /// /Applications or ~/Applications (macOS), else from this quickhop binary.
    On {
        /// Only register it for the next login; do not start the daemon now.
        #[arg(long)]
        no_start: bool,
        /// Write the XDG autostart entry even on sway, i3, Hyprland or river, for sessions
        /// where dex or uwsm runs such entries.
        #[arg(long)]
        xdg: bool,
    },
    /// Stop starting it at login.
    Off,
    /// Print whether it starts at login, which program it runs, and what is wrong with that.
    Status,
}

/// Why autostart could not be changed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AutostartError {
    #[error("launch at login is not supported on {0} yet")]
    Unsupported(String),
    #[error("cannot find the home folder: HOME is unset")]
    NoHome,
    #[error("{path}: {source}")]
    File {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot find this program's path: {0}")]
    Exe(io::Error),
    #[error(
        "{name} does not start XDG autostart entries, so quickhop cannot register itself \
         there. Add this line to {}:\n    {line}\n(When dex or uwsm runs XDG autostart entries \
         in this session, `quickhop autostart on --xdg` writes one instead.)",
        config.display()
    )]
    NotHonoured {
        name: &'static str,
        config: PathBuf,
        line: String,
    },
    #[cfg(unix)]
    #[error(transparent)]
    Daemon(#[from] crate::daemon::DaemonError),
    #[cfg(unix)]
    #[error("launchctl {action} failed: {message}")]
    Launchctl {
        action: &'static str,
        message: String,
    },
    #[cfg(unix)]
    #[error(
        "the quickhop daemon did not answer within {} s of starting {}: `launchctl print \
         gui/$(id -u)/{LABEL}` shows why",
        START_TIMEOUT.as_secs(),
        .0.display()
    )]
    NoStart(PathBuf),
}

/// How long a daemon started by `autostart on` may take to answer (the app's first launch
/// loads its window toolkit).
#[cfg(unix)]
const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The XDG configuration folder: `XDG_CONFIG_HOME` when absolute, else `~/.config`.
fn config_home(var: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let get = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    get("XDG_CONFIG_HOME")
        .filter(|p| p.is_absolute())
        .or_else(|| get("HOME").map(|home| home.join(".config")))
}

/// Where `flavour`'s login entry lives on `os`.
///
/// # Errors
/// On Windows (not supported yet) or without `HOME` / `XDG_CONFIG_HOME`.
pub(crate) fn entry_path(
    flavour: Flavour,
    os: &str,
    var: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, AutostartError> {
    match os {
        "windows" => Err(AutostartError::Unsupported(os.to_owned())),
        "macos" => var("HOME")
            .filter(|v| !v.is_empty())
            .map(|home| {
                PathBuf::from(home)
                    .join("Library/LaunchAgents")
                    .join(format!("{}.plist", flavour.bundle_id()))
            })
            .ok_or(AutostartError::NoHome),
        _ => config_home(&var)
            .map(|config| {
                config
                    .join("autostart")
                    .join(format!("{}.desktop", flavour.stem()))
            })
            .ok_or(AutostartError::NoHome),
    }
}

/// A Linux window manager or compositor that reads no XDG autostart entries, and starts
/// programs at login from its own configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Compositor {
    /// sway: `exec` in `~/.config/sway/config`.
    Sway,
    /// i3: `exec` in `~/.config/i3/config`.
    I3,
    /// Hyprland: `exec-once` in `~/.config/hypr/hyprland.conf`.
    Hyprland,
    /// river: its `~/.config/river/init` script.
    River,
}

impl Compositor {
    /// Its name, as its users write it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Sway => "sway",
            Self::I3 => "i3",
            Self::Hyprland => "Hyprland",
            Self::River => "river",
        }
    }

    /// Its configuration file under the XDG configuration folder `config_home`.
    pub(crate) fn config(self, config_home: &Path) -> PathBuf {
        config_home.join(match self {
            Self::Sway => "sway/config",
            Self::I3 => "i3/config",
            Self::Hyprland => "hypr/hyprland.conf",
            Self::River => "river/init",
        })
    }

    /// The configuration line that starts `exe daemon` at login.
    pub(crate) fn line(self, exe: &Path) -> String {
        let command = format!("{} daemon", sh_quote(&exe.to_string_lossy()));
        match self {
            Self::Sway => format!("exec {command}"),
            Self::I3 => format!("exec --no-startup-id {command}"),
            Self::Hyprland => format!("exec-once = {command}"),
            Self::River => format!("{command} &"),
        }
    }
}

/// The compositor of this session when it is one that reads no XDG autostart entries: from
/// `XDG_CURRENT_DESKTOP` and `XDG_SESSION_DESKTOP`, else, when neither names a desktop, from the
/// compositors' own sockets.
pub(crate) fn compositor(var: &impl Fn(&str) -> Option<String>) -> Option<Compositor> {
    let desktops: Vec<String> = ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP"]
        .iter()
        .filter_map(|name| var(name))
        .flat_map(|value| {
            value
                .split(':')
                .map(|d| d.trim().to_ascii_lowercase())
                .collect::<Vec<_>>()
        })
        .filter(|d| !d.is_empty())
        .collect();
    if !desktops.is_empty() {
        return desktops.iter().find_map(|desktop| match desktop.as_str() {
            "sway" => Some(Compositor::Sway),
            "i3" => Some(Compositor::I3),
            "hyprland" => Some(Compositor::Hyprland),
            "river" => Some(Compositor::River),
            _ => None,
        });
    }
    let set = |name: &str| var(name).is_some_and(|v| !v.is_empty());
    if set("HYPRLAND_INSTANCE_SIGNATURE") {
        Some(Compositor::Hyprland)
    } else if set("SWAYSOCK") {
        Some(Compositor::Sway)
    } else if set("I3SOCK") {
        Some(Compositor::I3)
    } else {
        None
    }
}

/// `arg` quoted for `sh -c`, which sway, i3 and Hyprland run `exec` lines with.
fn sh_quote(arg: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "/._-+,:@=%".contains(c);
    if !arg.is_empty() && arg.chars().all(safe) {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

/// `arg` quoted for a desktop entry's `Exec` key: in double quotes when it holds anything
/// but safe characters, with `"`, `` ` ``, `$` and `\` escaped, and `%` doubled.
fn exec_quote(arg: &str) -> String {
    // `%` is not reserved; doubling it is its own escape.
    let safe = |c: char| c.is_ascii_alphanumeric() || "/._-+,:@=%".contains(c);
    let doubled = arg.replace('%', "%%");
    if !doubled.is_empty() && doubled.chars().all(safe) {
        return doubled;
    }
    let mut quoted = String::from("\"");
    for c in doubled.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    // The desktop-entry string type escapes backslashes once more.
    quoted.replace('\\', "\\\\")
}

/// The first argument of a desktop entry's `Exec` value, undoing [`exec_quote`].
fn exec_unquote(value: &str) -> Option<String> {
    // The desktop-entry string type first: `\\` is one backslash.
    let value = value.trim_start().replace("\\\\", "\\");
    let arg = if let Some(rest) = value.strip_prefix('"') {
        let mut arg = String::new();
        let mut chars = rest.chars();
        loop {
            match chars.next()? {
                '"' => break arg,
                '\\' => arg.push(chars.next()?),
                c => arg.push(c),
            }
        }
    } else {
        value.split_whitespace().next()?.to_owned()
    };
    Some(arg.replace("%%", "%"))
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// `flavour`'s login entry that runs `exe daemon` on `os`.
pub(crate) fn entry_text(flavour: Flavour, os: &str, exe: &Path) -> String {
    let exe = exe.to_string_lossy();
    if os == "macos" {
        return format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">
<plist version=\"1.0\">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
",
            flavour.bundle_id(),
            xml_escape(&exe)
        );
    }
    format!(
        "[Desktop Entry]
Type=Application
Name={}
Comment=Keyboard launcher: clipboard history and file index in the background
Exec={} daemon
Icon=quickhop
Terminal=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
",
        flavour.app_name(),
        exec_quote(&exe)
    )
}

/// The program a login entry `text` written for `os` runs: the `LaunchAgent`'s first
/// `ProgramArguments` string, or the first argument of the desktop entry's `Exec`.
pub(crate) fn entry_program(os: &str, text: &str) -> Option<PathBuf> {
    let program = if os == "macos" {
        let (_, rest) = text.split_once("<key>ProgramArguments</key>")?;
        let (_, rest) = rest.split_once("<string>")?;
        let (value, _) = rest.split_once("</string>")?;
        xml_unescape(value.trim())
    } else {
        let value = text
            .lines()
            .find_map(|line| line.trim_start().strip_prefix("Exec="))?;
        exec_unquote(value)?
    };
    (!program.is_empty()).then(|| PathBuf::from(program))
}

/// The bundle `exe` is the executable of: `<name>.app` for `<name>.app/Contents/MacOS/<file>`.
pub(crate) fn app_bundle(exe: &Path) -> Option<&Path> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app")
        .then_some(bundle)
}

/// The executables of `flavour`'s app ("quickhop.app" or "quickhop Dev.app") `autostart on`
/// looks for on macOS, in order.
pub(crate) fn app_candidates(flavour: Flavour, home: Option<&Path>) -> Vec<PathBuf> {
    let exe = format!("{}.app/Contents/MacOS/quickhop", flavour.app_name());
    let mut candidates = vec![Path::new("/Applications").join(&exe)];
    if let Some(home) = home {
        candidates.push(home.join("Applications").join(&exe));
    }
    candidates
}

/// The program `flavour`'s login entry runs on `os`, for the running executable `exe`: on
/// macOS, the executable of `flavour`'s app when `exe` is not one and that app is installed
/// (`exists`), so the daemon holds the app's Accessibility permission; else `exe` itself.
pub(crate) fn program(
    flavour: Flavour,
    os: &str,
    exe: &Path,
    home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    if os == "macos"
        && app_bundle(exe).is_none()
        && let Some(app) = app_candidates(flavour, home)
            .into_iter()
            .find(|path| exists(path))
    {
        return app;
    }
    exe.to_path_buf()
}

/// `path` with its symlinks resolved, or as it is when it cannot be (it is gone).
pub(crate) fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The running executable, symlinks resolved (a `quickhop` on the `PATH` that links into
/// quickhop.app is the app's executable).
///
/// # Errors
/// When the system cannot tell.
pub(crate) fn this_exe() -> Result<PathBuf, AutostartError> {
    std::env::current_exe()
        .map(|exe| resolved(&exe))
        .map_err(AutostartError::Exe)
}

/// What is wrong with a login entry at `entry` that runs `program` (`None`: it names none),
/// when `autostart on` would now register `expected`.
pub(crate) fn entry_problem(
    entry: &Path,
    program: Option<&Path>,
    expected: &Path,
) -> Option<String> {
    let Some(program) = program else {
        return Some(format!(
            "cannot tell which program {} runs: `quickhop autostart on` rewrites it",
            entry.display()
        ));
    };
    if !program.exists() {
        return Some(format!(
            "autostart runs {}, which does not exist (moved or uninstalled): `quickhop \
             autostart on` points it at {}",
            program.display(),
            expected.display()
        ));
    }
    (resolved(program) != resolved(expected)).then(|| {
        format!(
            "autostart runs {}, not {}: `quickhop autostart on` switches it",
            program.display(),
            expected.display()
        )
    })
}

/// The enabled link of `flavour`'s systemd user unit (`quickhop.service`, which the packages
/// ship; a development build's `quickhop-dev.service` exists only if someone made one), when
/// there is one: in the user's `config_home` or `/etc`.
fn unit_link(flavour: Flavour, config_home: Option<&Path>) -> Option<PathBuf> {
    let unit = format!("{}.service", flavour.stem());
    let wants = ["graphical-session.target.wants", "default.target.wants"];
    let roots = config_home
        .map(|config| config.join("systemd/user"))
        .into_iter()
        .chain([PathBuf::from("/etc/systemd/user")]);
    roots
        .flat_map(|root| wants.map(|w| root.join(w).join(&unit)))
        .find(|link| link.exists())
}

/// Whether the compositor configuration at `config` already starts `flavour`'s daemon: a
/// release's is any `quickhop … daemon` line; a development build's must name its program
/// `expected`, as the release's line names `quickhop` too.
fn compositor_starts(flavour: Flavour, config: &Path, expected: &Path) -> bool {
    let program = expected.to_string_lossy();
    std::fs::read_to_string(config).is_ok_and(|text| {
        text.lines().any(|line| {
            let line = line.trim_start();
            let names = match flavour {
                Flavour::Release => line.contains("quickhop"),
                Flavour::Dev => line.contains(program.as_ref()),
            };
            !line.starts_with('#') && names && line.contains("daemon")
        })
    })
}

/// How quickhop starts at login, for `autostart status` and `doctor`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Report {
    /// Whether anything starts it.
    pub(crate) on: bool,
    /// One line: what starts it, or how to make something do so.
    pub(crate) summary: String,
    /// What is wrong, each with its fix.
    pub(crate) problems: Vec<String>,
}

/// How `flavour`'s quickhop starts at login on `os` with the variables `var`, when
/// `autostart on` would now register `expected`.
///
/// # Errors
/// As [`entry_path`].
pub(crate) fn report(
    flavour: Flavour,
    os: &str,
    var: impl Fn(&str) -> Option<String>,
    expected: &Path,
) -> Result<Report, AutostartError> {
    let path = entry_path(flavour, os, &var)?;
    let mut report = Report::default();
    if let Ok(text) = std::fs::read_to_string(&path) {
        let program = entry_program(os, &text);
        report.on = true;
        report.summary = match &program {
            Some(program) => format!("on: {} runs {} daemon", path.display(), program.display()),
            None => format!("on: {}", path.display()),
        };
        report
            .problems
            .extend(entry_problem(&path, program.as_deref(), expected));
    } else {
        report.summary = format!(
            "off: {} does not exist (`quickhop autostart on` writes it)",
            path.display()
        );
    }
    if os == "macos" {
        return Ok(report);
    }
    let config_home = config_home(&var);
    let unit = unit_link(flavour, config_home.as_deref());
    if let Some(compositor) = compositor(&var) {
        let config = config_home.as_deref().map_or_else(
            || PathBuf::from("~/.config").join(compositor.config(Path::new(""))),
            |home| compositor.config(home),
        );
        let name = compositor.name();
        if compositor_starts(flavour, &config, expected) {
            report.on = true;
            report.summary = format!("on: {} starts quickhop daemon ({name})", config.display());
        } else if report.on {
            report.problems.push(format!(
                "{name} does not start XDG autostart entries (unless dex or uwsm runs them): \
                 add `{}` to {}",
                compositor.line(expected),
                config.display()
            ));
        } else {
            report.summary = format!(
                "off: {name} does not start XDG autostart entries; add `{}` to {}",
                compositor.line(expected),
                config.display()
            );
        }
        if let Some(unit) = &unit {
            report.problems.push(format!(
                "the systemd user unit ({}) waits for graphical-session.target, which {name} \
                 does not reach: `systemctl --user disable quickhop` and start quickhop from {} \
                 instead",
                unit.display(),
                config.display()
            ));
        }
    } else if let Some(unit) = unit {
        if report.on {
            report.problems.push(format!(
                "both the systemd user unit ({}) and {} start quickhop, so every login logs \
                 `already running`: keep one (`quickhop autostart off`, or `systemctl --user \
                 disable quickhop`)",
                unit.display(),
                path.display()
            ));
        } else {
            report.on = true;
            report.summary = format!(
                "on: the systemd user unit {}.service ({})",
                flavour.stem(),
                unit.display()
            );
        }
    }
    Ok(report)
}

/// Runs `action` for the running binary.
///
/// # Errors
/// When the entry cannot be written or removed, the daemon cannot be started, the session's
/// compositor reads no autostart entries, or the OS is not supported yet.
pub(crate) fn run(action: AutostartAction, out: &mut impl Write) -> Result<(), AutostartError> {
    let flavour = Flavour::CURRENT;
    let os = std::env::consts::OS;
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let path = entry_path(flavour, os, var)?;
    let file_error = |source| AutostartError::File {
        path: path.clone(),
        source,
    };
    let home = var("HOME").map(PathBuf::from);
    let expected = || -> Result<PathBuf, AutostartError> {
        Ok(program(
            flavour,
            os,
            &this_exe()?,
            home.as_deref(),
            Path::is_file,
        ))
    };
    match action {
        AutostartAction::On { no_start, xdg } => {
            let program = resolved(&expected()?);
            if os != "macos"
                && !xdg
                && let Some(compositor) = compositor(&var)
            {
                let config = compositor.config(&config_home(&var).ok_or(AutostartError::NoHome)?);
                if !no_start {
                    start_now(os, &program, None, out)?;
                }
                if compositor_starts(flavour, &config, &program) {
                    let _ = writeln!(
                        out,
                        "{} starts quickhop daemon at login from {}",
                        compositor.name(),
                        config.display()
                    );
                    return Ok(());
                }
                return Err(AutostartError::NotHonoured {
                    name: compositor.name(),
                    config,
                    line: compositor.line(&program),
                });
            }
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(file_error)?;
            }
            std::fs::write(&path, entry_text(flavour, os, &program)).map_err(file_error)?;
            let _ = writeln!(
                out,
                "quickhop daemon starts at login: {} runs {} daemon",
                path.display(),
                program.display()
            );
            if os == "macos" {
                let app = flavour.app_name();
                let _ = match app_bundle(&program) {
                    Some(bundle) => writeln!(
                        out,
                        "it runs {}, so the Accessibility permission you give {app}.app \
                         covers its pasting",
                        bundle.display()
                    ),
                    None => writeln!(
                        out,
                        "note: no {app}.app in /Applications or ~/Applications, so macOS asks \
                         Accessibility for this binary itself ({})",
                        program.display()
                    ),
                };
            }
            if !no_start {
                start_now(os, &program, Some(&path), out)?;
            }
        }
        AutostartAction::Off => match std::fs::remove_file(&path) {
            Ok(()) => {
                let _ = writeln!(out, "removed {}", path.display());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let _ = writeln!(out, "already off ({} does not exist)", path.display());
            }
            Err(error) => return Err(file_error(error)),
        },
        AutostartAction::Status => {
            let report = report(flavour, os, var, &expected()?)?;
            let _ = writeln!(out, "{}", report.summary);
            for problem in &report.problems {
                let _ = writeln!(out, "warn: {problem}");
            }
        }
    }
    Ok(())
}

/// Starts the daemon now, as the login entry at `entry` (`None`: there is none) would, from
/// `program`: through launchd on macOS, so it runs as the `LaunchAgent`; else in the
/// background. A daemon that already runs from `program` is kept; one from another program,
/// or an older version of this one, is stopped first.
#[cfg(unix)]
fn start_now(
    os: &str,
    program: &Path,
    entry: Option<&Path>,
    out: &mut impl Write,
) -> Result<(), AutostartError> {
    use crate::daemon::{self, DaemonError, Request};

    let this = this_exe()?;
    let version = env!("CARGO_PKG_VERSION");
    match daemon::send(&Request::Status) {
        Ok(running) => {
            let exe = running.exe.as_deref().map(resolved);
            // Another program's version is unknown here; this binary's must match.
            let current = program != this || running.version.as_deref() == Some(version);
            if exe.as_deref() == Some(program) && current {
                let _ = writeln!(
                    out,
                    "the quickhop daemon already runs (pid {}) from {}",
                    running.pid,
                    program.display()
                );
                return Ok(());
            }
            let _ = writeln!(
                out,
                "stopping the quickhop daemon (pid {}, {}, version {}) to start {} instead",
                running.pid,
                exe.as_deref()
                    .map_or_else(|| "executable unknown".into(), Path::to_string_lossy),
                running.version.as_deref().unwrap_or("unknown"),
                program.display()
            );
            daemon::stop_and_wait(running.pid)?;
        }
        Err(DaemonError::NotRunning) => {}
        Err(error) => return Err(error.into()),
    }
    let running = match entry {
        Some(plist) if os == "macos" => {
            launchd_start(plist)?;
            wait_for_daemon(program)?
        }
        // Elsewhere the entry runs this very binary (see `program`).
        _ => daemon::send_starting(&Request::Status)?,
    };
    let _ = writeln!(
        out,
        "started the quickhop daemon (pid {}) from {}",
        running.pid,
        running.exe.as_deref().unwrap_or(program).display()
    );
    Ok(())
}

/// Never reached: [`entry_path`] refuses these systems first.
#[cfg(not(unix))]
fn start_now(
    os: &str,
    _program: &Path,
    _entry: Option<&Path>,
    _out: &mut impl Write,
) -> Result<(), AutostartError> {
    Err(AutostartError::Unsupported(os.to_owned()))
}

/// Loads the `LaunchAgent` at `plist` into this user's GUI session, replacing a definition an
/// earlier `autostart on` loaded; `RunAtLoad` starts its daemon.
#[cfg(unix)]
fn launchd_start(plist: &Path) -> Result<(), AutostartError> {
    use std::os::unix::fs::MetadataExt as _;
    use std::process::Command;

    // The plist was just written by this user, so its owner is this user's id.
    let uid = std::fs::metadata(plist)
        .map_err(|source| AutostartError::File {
            path: plist.to_path_buf(),
            source,
        })?
        .uid();
    let domain = format!("gui/{uid}");
    // Absent when nothing was loaded: then there is nothing to replace.
    let _ = Command::new("/bin/launchctl")
        .args(["bootout", &format!("{domain}/{LABEL}")])
        .output();
    let launchctl_error = |message: String| AutostartError::Launchctl {
        action: "bootstrap",
        message,
    };
    // A job being booted out can make bootstrap fail for a moment.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let output = Command::new("/bin/launchctl")
            .args(["bootstrap", &domain])
            .arg(plist)
            .output()
            .map_err(|e| launchctl_error(e.to_string()))?;
        if output.status.success() {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            return Err(launchctl_error(format!(
                "{} ({})",
                String::from_utf8_lossy(&output.stderr).trim(),
                output.status
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// The status of the daemon launchd started from `program`, once it answers.
#[cfg(unix)]
fn wait_for_daemon(program: &Path) -> Result<crate::daemon::Response, AutostartError> {
    use crate::daemon::{self, DaemonError, Request};

    let deadline = std::time::Instant::now() + START_TIMEOUT;
    loop {
        match daemon::send(&Request::Status) {
            Ok(running) => return Ok(running),
            Err(DaemonError::NotRunning) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(DaemonError::NotRunning) => {
                return Err(AutostartError::NoStart(program.to_path_buf()));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickhop_core::Flavour::{Dev, Release};

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    /// `name` under an absolute root of the OS the tests run on: on Windows only a drive
    /// makes a path absolute.
    fn abs(name: &str) -> String {
        let root = if cfg!(windows) { r"C:\" } else { "/" };
        format!("{root}{name}")
    }

    #[test]
    fn entry_paths_per_os() {
        let (c, h) = (abs("c"), abs("h"));
        assert_eq!(
            entry_path(
                Release,
                "linux",
                vars(&[("XDG_CONFIG_HOME", &c), ("HOME", &h)])
            )
            .unwrap(),
            PathBuf::from(&c).join("autostart").join("quickhop.desktop")
        );
        assert_eq!(
            entry_path(
                Release,
                "freebsd",
                vars(&[("XDG_CONFIG_HOME", "rel"), ("HOME", &h)])
            )
            .unwrap(),
            PathBuf::from(&h)
                .join(".config")
                .join("autostart")
                .join("quickhop.desktop")
        );
        let user = abs("Users/a");
        assert_eq!(
            entry_path(Release, "macos", vars(&[("HOME", &user)])).unwrap(),
            PathBuf::from(&user).join("Library/LaunchAgents/io.github.yuyudhan.quickhop.plist")
        );
        assert!(matches!(
            entry_path(Release, "linux", vars(&[])),
            Err(AutostartError::NoHome)
        ));
        assert!(matches!(
            entry_path(Release, "windows", vars(&[("HOME", &h)])),
            Err(AutostartError::Unsupported(_))
        ));
    }

    #[test]
    fn a_development_build_has_its_own_entries() {
        let (c, user) = (abs("c"), abs("Users/a"));
        assert_eq!(
            entry_path(Dev, "linux", vars(&[("XDG_CONFIG_HOME", &c)])).unwrap(),
            PathBuf::from(&c)
                .join("autostart")
                .join("quickhop-dev.desktop")
        );
        assert_eq!(
            entry_path(Dev, "macos", vars(&[("HOME", &user)])).unwrap(),
            PathBuf::from(&user).join("Library/LaunchAgents/io.github.yuyudhan.quickhop.dev.plist")
        );
        let exe = Path::new("/w/quickhop/target/debug/quickhop");
        let plist = entry_text(Dev, "macos", exe);
        assert!(plist.contains("<string>io.github.yuyudhan.quickhop.dev</string>"));
        assert!(!plist.contains("<string>io.github.yuyudhan.quickhop</string>"));
        let desktop = entry_text(Dev, "linux", exe);
        assert!(desktop.contains("\nName=quickhop Dev\n"), "{desktop}");
        assert_eq!(entry_program("macos", &plist), Some(exe.to_path_buf()));
        assert_eq!(
            app_candidates(Dev, Some(Path::new("/Users/a"))),
            [
                PathBuf::from("/Applications/quickhop Dev.app/Contents/MacOS/quickhop"),
                PathBuf::from("/Users/a/Applications/quickhop Dev.app/Contents/MacOS/quickhop"),
            ]
        );
        // The installed release's app is not the development build's.
        let release_app = Path::new("/Applications/quickhop.app/Contents/MacOS/quickhop");
        let dev_app = Path::new("/Applications/quickhop Dev.app/Contents/MacOS/quickhop");
        assert_eq!(program(Dev, "macos", exe, None, |p| p == release_app), exe);
        assert_eq!(program(Dev, "macos", exe, None, |p| p == dev_app), dev_app);
        assert_eq!(program(Release, "macos", exe, None, |p| p == dev_app), exe);
    }

    #[test]
    fn exec_arguments_are_quoted_per_the_desktop_entry_spec() {
        assert_eq!(exec_quote("/usr/bin/quickhop"), "/usr/bin/quickhop");
        assert_eq!(
            exec_quote("/opt/my apps/quickhop"),
            "\"/opt/my apps/quickhop\""
        );
        assert_eq!(exec_quote("/x/100%/q"), "/x/100%%/q");
        assert_eq!(
            exec_quote("/a\"b$c`d\\e"),
            "\"/a\\\\\"b\\\\$c\\\\`d\\\\\\\\e\""
        );
    }

    #[test]
    fn entries_run_the_daemon() {
        let desktop = entry_text(Release, "linux", Path::new("/home/a b/.cargo/bin/quickhop"));
        assert!(desktop.starts_with("[Desktop Entry]\nType=Application\n"));
        assert!(desktop.contains("\nExec=\"/home/a b/.cargo/bin/quickhop\" daemon\n"));
        assert!(desktop.contains("\nName=quickhop\n"), "{desktop}");
        let plist = entry_text(Release, "macos", Path::new("/Applications/Q&A/quickhop"));
        assert!(plist.contains(
            "<string>/Applications/Q&amp;A/quickhop</string>\n        <string>daemon</string>"
        ));
        assert!(plist.contains("<string>io.github.yuyudhan.quickhop</string>"));
    }

    #[test]
    fn the_program_an_entry_runs_is_read_back() {
        for exe in [
            "/usr/bin/quickhop",
            "/home/a b/.cargo/bin/quickhop",
            "/x/100%/q",
            "/a\"b$c`d\\e/quickhop",
            "/Applications/Q&A <1>/quickhop.app/Contents/MacOS/quickhop",
        ] {
            for os in ["linux", "macos"] {
                assert_eq!(
                    entry_program(os, &entry_text(Release, os, Path::new(exe))),
                    Some(PathBuf::from(exe)),
                    "{os}: {exe}"
                );
            }
        }
        assert_eq!(entry_program("linux", "[Desktop Entry]\nName=x\n"), None);
        assert_eq!(entry_program("macos", "<plist><dict></dict></plist>"), None);
    }

    #[test]
    fn macos_registers_the_installed_app_not_a_bare_binary() {
        let home = Path::new("/Users/a");
        let system_app = Path::new("/Applications/quickhop.app/Contents/MacOS/quickhop");
        let user_app = Path::new("/Users/a/Applications/quickhop.app/Contents/MacOS/quickhop");
        let cargo = Path::new("/Users/a/.cargo/bin/quickhop");
        let installed =
            |apps: &'static [&'static str]| move |p: &Path| apps.iter().any(|a| p == Path::new(a));
        let both: &[&str] = &[
            "/Applications/quickhop.app/Contents/MacOS/quickhop",
            "/Users/a/Applications/quickhop.app/Contents/MacOS/quickhop",
        ];
        assert_eq!(
            program(Release, "macos", cargo, Some(home), installed(both)),
            system_app
        );
        let user: &[&str] = &["/Users/a/Applications/quickhop.app/Contents/MacOS/quickhop"];
        assert_eq!(
            program(Release, "macos", cargo, Some(home), installed(user)),
            user_app
        );
        assert_eq!(
            program(Release, "macos", cargo, Some(home), installed(&[])),
            cargo
        );
        // An app registers itself, wherever it is.
        let elsewhere = Path::new("/opt/quickhop.app/Contents/MacOS/quickhop");
        assert_eq!(
            program(Release, "macos", elsewhere, Some(home), installed(both)),
            elsewhere
        );
        // Linux has no app.
        assert_eq!(
            program(Release, "linux", cargo, Some(home), installed(both)),
            cargo
        );
        assert_eq!(
            app_bundle(system_app),
            Some(Path::new("/Applications/quickhop.app"))
        );
        assert_eq!(app_bundle(cargo), None);
    }

    #[test]
    fn tiling_compositors_are_detected_from_the_session() {
        type Session = &'static [(&'static str, &'static str)];
        let cases: [(Session, Option<Compositor>); 10] = [
            (&[("XDG_CURRENT_DESKTOP", "sway")], Some(Compositor::Sway)),
            (&[("XDG_CURRENT_DESKTOP", "i3")], Some(Compositor::I3)),
            (
                &[("XDG_CURRENT_DESKTOP", "Hyprland")],
                Some(Compositor::Hyprland),
            ),
            (&[("XDG_SESSION_DESKTOP", "river")], Some(Compositor::River)),
            (&[("XDG_CURRENT_DESKTOP", "ubuntu:GNOME")], None),
            (
                &[("XDG_CURRENT_DESKTOP", "KDE"), ("SWAYSOCK", "/run/s")],
                None,
            ),
            (
                &[("SWAYSOCK", "/run/sway.sock"), ("I3SOCK", "/run/sway.sock")],
                Some(Compositor::Sway),
            ),
            (&[("I3SOCK", "/run/i3/ipc")], Some(Compositor::I3)),
            (
                &[("HYPRLAND_INSTANCE_SIGNATURE", "abc")],
                Some(Compositor::Hyprland),
            ),
            (&[], None),
        ];
        for (session, expected) in cases {
            assert_eq!(compositor(&vars(session)), expected, "{session:?}");
        }
    }

    #[test]
    fn each_compositor_gets_its_exact_config_line() {
        let exe = Path::new("/home/a/.cargo/bin/quickhop");
        assert_eq!(
            Compositor::Sway.line(exe),
            "exec /home/a/.cargo/bin/quickhop daemon"
        );
        assert_eq!(
            Compositor::I3.line(exe),
            "exec --no-startup-id /home/a/.cargo/bin/quickhop daemon"
        );
        assert_eq!(
            Compositor::Hyprland.line(exe),
            "exec-once = /home/a/.cargo/bin/quickhop daemon"
        );
        assert_eq!(
            Compositor::River.line(exe),
            "/home/a/.cargo/bin/quickhop daemon &"
        );
        assert_eq!(
            Compositor::Sway.line(Path::new("/opt/it's here/quickhop")),
            r"exec '/opt/it'\''s here/quickhop' daemon"
        );
        let home = Path::new("/home/a/.config");
        assert_eq!(
            Compositor::Hyprland.config(home),
            home.join("hypr/hyprland.conf")
        );
        assert_eq!(Compositor::Sway.config(home), home.join("sway/config"));
    }

    #[test]
    fn an_entry_for_a_missing_or_other_binary_is_a_problem()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let entry = dir.path().join("quickhop.desktop");
        let here = dir.path().join("quickhop");
        std::fs::write(&here, "")?;
        let gone = dir.path().join("uninstalled/quickhop");
        let problem = entry_problem(&entry, Some(&gone), &here).ok_or("no problem")?;
        assert!(problem.contains("which does not exist"), "{problem}");
        assert!(problem.contains("`quickhop autostart on`"), "{problem}");
        let other = dir.path().join("other");
        std::fs::write(&other, "")?;
        let problem = entry_problem(&entry, Some(&other), &here).ok_or("no problem")?;
        assert!(
            problem.contains(&format!("not {}", here.display())),
            "{problem}"
        );
        assert_eq!(entry_problem(&entry, Some(&here), &here), None);
        assert!(entry_problem(&entry, None, &here).is_some());
        Ok(())
    }

    #[test]
    fn the_report_reads_the_entry_back() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let config = dir.path().join("config");
        let config_str = config.to_string_lossy().into_owned();
        let here = dir.path().join("quickhop");
        std::fs::write(&here, "")?;
        let pairs = [
            ("XDG_CONFIG_HOME", config_str.as_str()),
            ("XDG_CURRENT_DESKTOP", "GNOME"),
        ];
        let session = vars(&pairs);
        let off = report(Release, "linux", &session, &here)?;
        assert!(!off.on && off.summary.starts_with("off: "), "{off:?}");

        let entry = config.join("autostart").join("quickhop.desktop");
        std::fs::create_dir_all(entry.parent().ok_or("no parent")?)?;
        let gone = dir.path().join("gone/quickhop");
        std::fs::write(&entry, entry_text(Release, "linux", &gone))?;
        let stale = report(Release, "linux", &session, &here)?;
        assert!(stale.on, "{stale:?}");
        assert_eq!(
            stale.summary,
            format!("on: {} runs {} daemon", entry.display(), gone.display())
        );
        assert_eq!(stale.problems.len(), 1, "{stale:?}");
        assert!(
            stale.problems[0].contains("which does not exist"),
            "{stale:?}"
        );

        // The packages' unit as well: two starts at every login.
        let wants = config.join("systemd/user/graphical-session.target.wants");
        std::fs::create_dir_all(&wants)?;
        std::fs::write(wants.join("quickhop.service"), "")?;
        std::fs::write(&entry, entry_text(Release, "linux", &here))?;
        let both = report(Release, "linux", &session, &here)?;
        assert_eq!(both.problems.len(), 1, "{both:?}");
        assert!(both.problems[0].contains("already running"), "{both:?}");

        // A development build reads its own entry, and the release's unit is not its.
        let dev = report(Dev, "linux", &session, &here)?;
        assert!(!dev.on && dev.problems.is_empty(), "{dev:?}");
        assert!(dev.summary.contains("quickhop-dev.desktop"), "{dev:?}");
        Ok(())
    }

    #[test]
    fn on_sway_the_report_names_the_config_line() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let config = dir.path().join("config");
        let config_str = config.to_string_lossy().into_owned();
        let here = Path::new("/usr/bin/quickhop");
        let pairs = [
            ("XDG_CONFIG_HOME", config_str.as_str()),
            ("XDG_CURRENT_DESKTOP", "sway"),
        ];
        let sway = vars(&pairs);
        let off = report(Release, "linux", &sway, here)?;
        assert!(!off.on, "{off:?}");
        assert!(
            off.summary
                .contains("add `exec /usr/bin/quickhop daemon` to "),
            "{off:?}"
        );
        let sway_config = config.join("sway/config");
        std::fs::create_dir_all(sway_config.parent().ok_or("no parent")?)?;
        std::fs::write(
            &sway_config,
            "# exec quickhop daemon\nexec /usr/bin/quickhop daemon\n",
        )?;
        let on = report(Release, "linux", &sway, here)?;
        assert!(on.on && on.problems.is_empty(), "{on:?}");
        // The release's line does not start a development build's daemon.
        let dev_exe = Path::new("/w/target/debug/quickhop");
        let dev = report(Dev, "linux", &sway, dev_exe)?;
        assert!(!dev.on, "{dev:?}");
        std::fs::write(
            &sway_config,
            "exec /usr/bin/quickhop daemon\nexec /w/target/debug/quickhop daemon\n",
        )?;
        assert!(report(Dev, "linux", &sway, dev_exe)?.on);
        Ok(())
    }
}
