//! `quickhop doctor`: what this installation is and what stands in the way of a first run.
//! Every line is a fact; a `warn:` line also says how to fix it. Exit status stays success
//! unless `--strict` is given and something warned.

use std::io::Write;
use std::path::{Path, PathBuf};

use quickhop_core::config::data_dir;
use quickhop_core::{Cancel, Config, Query};
use quickhop_providers::Env;

use crate::tools::{self, Display};
use crate::{CliError, autostart, default_path, var, warning_text};

/// One finding of the report.
#[derive(Debug, PartialEq, Eq)]
enum Line {
    /// A fact.
    Info(String),
    /// A problem and its fix.
    Warn(String),
}

impl std::fmt::Display for Line {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info(text) => f.write_str(text),
            Self::Warn(text) => write!(f, "warn: {text}"),
        }
    }
}

/// The report being written, counting its warnings.
struct Report<'a, W> {
    out: &'a mut W,
    warnings: usize,
}

impl<W: Write> Report<'_, W> {
    fn line(&mut self, line: &Line) -> std::io::Result<()> {
        if matches!(line, Line::Warn(_)) {
            self.warnings += 1;
        }
        writeln!(self.out, "{line}")
    }

    fn info(&mut self, text: String) -> std::io::Result<()> {
        self.line(&Line::Info(text))
    }

    fn lines(&mut self, lines: &[Line]) -> std::io::Result<()> {
        lines.iter().try_for_each(|line| self.line(line))
    }
}

/// What the running daemon said about itself.
#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy)]
struct Running<'a> {
    pid: u32,
    /// `None`: a daemon too old to say.
    version: Option<&'a str>,
    /// `None`: a daemon too old to say, or one that could not tell; symlinks resolved.
    exe: Option<&'a Path>,
    /// Whether the daemon may paste into other apps: the macOS Accessibility permission of the
    /// daemon's own process (`None`: a daemon too old to say).
    #[cfg(any(target_os = "macos", test))]
    may_paste: Option<bool>,
}

/// Whether a daemon answered (Unix: Windows has no daemon yet).
#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy)]
enum Daemon<'a> {
    Running(Running<'a>),
    NotRunning,
    /// Talking to it failed.
    Failed(&'a str),
}

/// Who macOS asks the Accessibility permission for when `exe` runs: the app, for a bundle's
/// executable, else the binary itself.
#[cfg(any(target_os = "macos", test))]
fn grantee(exe: &Path) -> String {
    match autostart::app_bundle(exe) {
        Some(app) => format!("quickhop.app ({})", app.display()),
        None => format!("the binary {}", exe.display()),
    }
}

/// The daemon's lines: whether it runs, its version and executable, against this quickhop's
/// `version` and the program `autostart on` would run now, `expected`.
#[cfg(any(unix, test))]
fn daemon_lines(daemon: Daemon<'_>, version: &str, expected: &Path) -> Vec<Line> {
    let running = match daemon {
        Daemon::Running(running) => running,
        Daemon::NotRunning => {
            return vec![Line::Info(
                "daemon: not running (`quickhop autostart on` starts it now and at every login)"
                    .into(),
            )];
        }
        Daemon::Failed(error) => return vec![Line::Warn(format!("daemon: {error}"))],
    };
    let exe = running.exe.map_or_else(
        || "executable unknown".into(),
        |exe| exe.display().to_string(),
    );
    let pid = running.pid;
    let mut lines = vec![Line::Info(format!(
        "daemon: running (pid {pid}, version {}, {exe})",
        running.version.unwrap_or("unknown")
    ))];
    match running.version {
        Some(running_version) if running_version != version => lines.push(Line::Warn(format!(
            "the daemon (pid {pid}) runs quickhop {running_version}, this is {version}: \
             `quickhop autostart on` restarts it"
        ))),
        Some(_) => {}
        None => lines.push(Line::Warn(format!(
            "the daemon (pid {pid}) is from a quickhop too old to report its version and \
             executable: `quickhop autostart on` restarts it"
        ))),
    }
    if let Some(exe) = running.exe
        && exe != expected
    {
        let why = if autostart::app_bundle(expected).is_some() {
            ": quickhop.app is installed, and its Accessibility permission does not cover \
             another program"
        } else {
            ""
        };
        lines.push(Line::Warn(format!(
            "the daemon runs {}, not {}{why}; `quickhop autostart on` restarts it from {}",
            exe.display(),
            expected.display(),
            expected.display()
        )));
    }
    lines
}

/// The macOS Accessibility line: the daemon's own state when it reports one, else this
/// process's (`this_trusted`, the terminal's grant) and who the daemon needs it for.
#[cfg(any(target_os = "macos", test))]
fn accessibility_line(daemon: Daemon<'_>, this_trusted: bool, expected: &Path) -> Line {
    const PANE: &str = "System Settings › Privacy & Security › Accessibility (Device Control \
                        and Data Access on macOS 27)";
    let running = match daemon {
        Daemon::Running(running) => Some(running),
        _ => None,
    };
    let grantee = grantee(running.and_then(|r| r.exe).unwrap_or(expected));
    match running.and_then(|r| r.may_paste) {
        Some(true) => Line::Info(format!("accessibility: allowed for the daemon, {grantee}")),
        Some(false) => Line::Warn(format!(
            "accessibility: the daemon may not paste: switch on {grantee} in {PANE}. When it \
             is on already, it is an older build: switch it off and on, or remove it with − \
             and add it again. `quickhop onboarding` opens the pane"
        )),
        None if this_trusted => Line::Info(format!(
            "accessibility: allowed for this terminal; the daemon needs it for {grantee} \
             (`quickhop onboarding` shows its state)"
        )),
        None => Line::Warn(format!(
            "accessibility: not allowed for this terminal, so pasting from it only copies; the \
             daemon needs it for {grantee}: switch that on in {PANE} (`quickhop onboarding` \
             opens the pane)"
        )),
    }
}

/// The line for the Linux or BSD helper `tool` in a `display` session: found, unused here, or
/// missing with the command that installs it on the distribution `os_release` describes.
fn linux_tool_line(tool: &str, display: Display, found: bool, os_release: Option<&str>) -> Line {
    let state = if found { "found" } else { "missing" };
    match tools::purpose(tool, display) {
        None if display == Display::None => Line::Info(format!(
            "tool {tool}: {state} (not used without a graphical session)"
        )),
        None => Line::Info(format!(
            "tool {tool}: {state} (not used in this {} session)",
            display.describe()
        )),
        Some(_) if found => Line::Info(format!("tool {tool}: found")),
        Some(purpose) => Line::Warn(format!(
            "{tool} missing ({purpose}): {}",
            tools::install_hint(tool, os_release)
        )),
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

/// Whether `program` is an executable file in one of `env`'s `PATH` directories.
fn on_path(env: &Env, program: &str) -> bool {
    env.search_path()
        .iter()
        .any(|dir| dir.join(program).is_file())
}

/// The `quickhop` programs on `env`'s `PATH`, in `PATH` order, then the installed apps' (macOS),
/// symlinks resolved and each once.
fn copies(env: &Env) -> Vec<PathBuf> {
    let name = format!("quickhop{}", std::env::consts::EXE_SUFFIX);
    let home = env.var("HOME").map(PathBuf::from);
    let apps = if env.os == "macos" {
        autostart::app_candidates(home.as_deref())
    } else {
        Vec::new()
    };
    let mut found: Vec<PathBuf> = Vec::new();
    for path in env
        .search_path()
        .iter()
        .map(|dir| dir.join(&name))
        .chain(apps)
    {
        if path.is_file() {
            let path = autostart::resolved(&path);
            if !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

/// The lines about this binary (`this`, resolved) and the other copies: `copies` as
/// [`copies`] lists them, the first being the one `quickhop` runs when it is on the `PATH`
/// (`first_on_path`).
fn binary_lines(this: &Path, copies: &[PathBuf], first_on_path: bool) -> Vec<Line> {
    let mut lines = vec![Line::Info(format!("binary: {}", this.display()))];
    lines.extend(
        copies
            .iter()
            .filter(|copy| copy.as_path() != this)
            .map(|copy| Line::Info(format!("copy: {}", copy.display()))),
    );
    if !first_on_path {
        lines.push(Line::Info(
            "path: no `quickhop` on the PATH (add this binary's folder to it)".into(),
        ));
    } else if let Some(first) = copies.first()
        && first != this
    {
        lines.push(Line::Warn(format!(
            "`quickhop` on the PATH runs {}, not this binary: the one you type and the one \
             `autostart on` registers can differ; remove the copies you do not use",
            first.display()
        )));
    }
    lines
}

/// How the global hotkey is bound on this system.
fn hotkey_path() -> &'static str {
    if cfg!(target_os = "macos") {
        "registered by `quickhop daemon` through RegisterEventHotKey"
    } else {
        "not registered yet: the Linux window is still being built"
    }
}

/// The running daemon's lines, and on macOS the Accessibility line.
#[cfg(unix)]
fn daemon_section<W: Write>(
    report: &mut Report<'_, W>,
    version: &str,
    expected: &Path,
) -> std::io::Result<()> {
    use crate::daemon::{DaemonError, Request, send};

    let reply = send(&Request::Status);
    let exe = reply
        .as_ref()
        .ok()
        .and_then(|running| running.exe.as_deref())
        .map(autostart::resolved);
    let error = reply.as_ref().err().map(ToString::to_string);
    let daemon = match &reply {
        Ok(running) => Daemon::Running(Running {
            pid: running.pid,
            version: running.version.as_deref(),
            exe: exe.as_deref(),
            #[cfg(any(target_os = "macos", test))]
            may_paste: running.may_paste,
        }),
        Err(DaemonError::NotRunning) => Daemon::NotRunning,
        Err(_) => Daemon::Failed(error.as_deref().unwrap_or_default()),
    };
    report.lines(&daemon_lines(daemon, version, expected))?;
    #[cfg(target_os = "macos")]
    report.line(&accessibility_line(
        daemon,
        quickhop_platform_macos::keys::is_trusted(),
        expected,
    ))?;
    Ok(())
}

/// No daemon here yet.
#[cfg(not(unix))]
fn daemon_section<W: Write>(
    report: &mut Report<'_, W>,
    _version: &str,
    _expected: &Path,
) -> std::io::Result<()> {
    report.info("daemon: not available on this system yet".into())
}

/// The configuration, data and index lines; returns the configuration in effect.
fn paths_section<W: Write>(report: &mut Report<'_, W>, env: &Env) -> std::io::Result<Config> {
    let config = match default_path() {
        Ok(path) if path.exists() => match Config::load(&path) {
            Ok(config) => {
                report.info(format!("config: {} (ok)", path.display()))?;
                config
            }
            Err(error) => {
                report.line(&Line::Warn(format!(
                    "config: {error} (using defaults until it is fixed)"
                )))?;
                Config::default()
            }
        },
        Ok(path) => {
            report.info(format!(
                "config: {} (absent, using defaults)",
                path.display()
            ))?;
            Config::default()
        }
        Err(error) => {
            report.info(format!("config: {error} (using defaults)"))?;
            Config::default()
        }
    };
    match data_dir(std::env::consts::OS, var) {
        Ok(dir) => report.info(format!("data: {}", dir.display()))?,
        Err(error) => report.info(format!("data: {error} (frecency disabled)"))?,
    }
    if let Ok(path) = quickhop_providers::index_path(env) {
        let state = if path.is_file() {
            quickhop_index::Index::open(&path)
                .and_then(|index| index.len())
                .map_or_else(|e| e.to_string(), |n| format!("{n} entries"))
        } else {
            "absent: run `quickhop index`".to_owned()
        };
        report.info(format!("index: {} ({state})", path.display()))?;
    }
    Ok(config)
}

/// The autostart lines, for the program `autostart on` would register now, `expected`.
fn autostart_section<W: Write>(
    report: &mut Report<'_, W>,
    env: &Env,
    expected: &Path,
) -> std::io::Result<()> {
    match autostart::report(&env.os, var, expected) {
        Ok(state) => {
            report.info(format!("autostart: {}", state.summary))?;
            for problem in state.problems {
                report.line(&Line::Warn(problem))?;
            }
            Ok(())
        }
        Err(error) => report.info(format!("autostart: {error}")),
    }
}

/// The session line (Linux and the BSDs) and one line per helper tool.
fn tools_section<W: Write>(report: &mut Report<'_, W>, env: &Env) -> std::io::Result<()> {
    let linux = env.os != "macos" && env.os != "windows";
    let display = Display::from_vars(|name| env.var(name).map(str::to_owned));
    if linux {
        report.info(format!("session: {}", display.describe()))?;
    }
    let os_release = tools::os_release();
    for tool in helper_tools(&env.os) {
        let found = on_path(env, tool);
        let line = if linux {
            linux_tool_line(tool, display, found, os_release.as_deref())
        } else if found {
            Line::Info(format!("tool {tool}: found"))
        } else {
            Line::Warn(format!(
                "{tool} missing: it comes with {}; check that PATH holds its system folders",
                env.os
            ))
        };
        report.line(&line)?;
    }
    Ok(())
}

/// The script folders, hotkey, providers and their loading warnings.
fn providers_section<W: Write>(
    report: &mut Report<'_, W>,
    config: &Config,
    env: &Env,
) -> std::io::Result<()> {
    let dirs = quickhop_providers::script_dirs(config, env);
    if dirs.is_empty() {
        match quickhop_core::config::scripts_dir(&env.os, var) {
            Ok(dir) => report.info(format!("scripts: none (create {})", dir.display()))?,
            Err(error) => report.info(format!("scripts: {error}"))?,
        }
    }
    for dir in &dirs {
        report.info(format!("scripts: {}", dir.display()))?;
    }
    report.info(format!("hotkey: {} ({})", config.hotkey, hotkey_path()))?;
    let (providers, warnings) = quickhop_providers::builtin(config, env);
    let cancel = Cancel::new();
    for name in quickhop_providers::BUILTIN {
        match providers.iter().find(|p| p.name() == name) {
            Some(provider) => {
                // A one-letter query: providers that hide everything for an empty query
                // (emoji) still report their size.
                let count = provider
                    .items(&Query::new("a"), &cancel)
                    .map_or(0, |items| items.len());
                report.info(format!("provider {name}: enabled ({count} items)"))?;
            }
            None => report.info(format!("provider {name}: disabled"))?,
        }
    }
    for warning in &warnings {
        report.info(format!("warning: {}", warning_text(warning)))?;
    }
    Ok(())
}

/// Writes the report to `out`.
///
/// # Errors
/// When `out` fails, or, with `strict`, when anything warned.
pub(crate) fn run(strict: bool, out: &mut impl Write) -> Result<(), CliError> {
    let mut report = Report { out, warnings: 0 };
    let version = env!("CARGO_PKG_VERSION");
    let env = Env::current();
    report.info(format!("quickhop {version}"))?;
    let this = autostart::this_exe()?;
    let first_on_path = on_path(&env, &format!("quickhop{}", std::env::consts::EXE_SUFFIX));
    report.lines(&binary_lines(&this, &copies(&env), first_on_path))?;
    report.info(format!(
        "platform: {}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ))?;
    let config = paths_section(&mut report, &env)?;
    let home = env.var("HOME").map(PathBuf::from);
    let expected = autostart::resolved(&autostart::program(
        &env.os,
        &this,
        home.as_deref(),
        Path::is_file,
    ));
    daemon_section(&mut report, version, &expected)?;
    autostart_section(&mut report, &env, &expected)?;
    tools_section(&mut report, &env)?;
    providers_section(&mut report, &config, &env)?;
    let ui = if cfg!(target_os = "macos") {
        "launcher window in `quickhop daemon`: the hotkey or `quickhop toggle` shows it"
    } else {
        "launcher window not built yet (gpui, see docs/adr/0001-ui-toolkit.md)"
    };
    report.info(format!("ui: {ui}"))?;
    let warned = report.warnings;
    report.info(match warned {
        0 => "doctor: no problems found".to_owned(),
        1 => "doctor: 1 warning (the `warn:` line above says how to fix it)".to_owned(),
        n => format!("doctor: {n} warnings (each `warn:` line above says how to fix it)"),
    })?;
    if strict && warned > 0 {
        return Err(CliError::DoctorWarned(warned));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = "/Applications/quickhop.app/Contents/MacOS/quickhop";
    const CARGO: &str = "/Users/a/.cargo/bin/quickhop";

    fn text(lines: &[Line]) -> String {
        lines.iter().fold(String::new(), |mut text, line| {
            text.push_str(&line.to_string());
            text.push('\n');
            text
        })
    }

    fn running<'a>(
        version: Option<&'a str>,
        exe: Option<&'a str>,
        may_paste: Option<bool>,
    ) -> Daemon<'a> {
        Daemon::Running(Running {
            pid: 42,
            version,
            exe: exe.map(Path::new),
            may_paste,
        })
    }

    #[test]
    fn a_current_daemon_from_the_expected_program_is_one_line() {
        let lines = daemon_lines(
            running(Some("0.2.0"), Some(APP), None),
            "0.2.0",
            Path::new(APP),
        );
        assert_eq!(
            text(&lines),
            format!("daemon: running (pid 42, version 0.2.0, {APP})\n")
        );
    }

    #[test]
    fn a_stale_daemon_version_warns_with_the_fix() {
        let lines = daemon_lines(
            running(Some("0.1.0"), Some(APP), None),
            "0.2.0",
            Path::new(APP),
        );
        assert_eq!(
            text(&lines),
            format!(
                "daemon: running (pid 42, version 0.1.0, {APP})\n\
                 warn: the daemon (pid 42) runs quickhop 0.1.0, this is 0.2.0: `quickhop \
                 autostart on` restarts it\n"
            )
        );
        let old = daemon_lines(running(None, None, None), "0.2.0", Path::new(APP));
        assert_eq!(
            text(&old),
            "daemon: running (pid 42, version unknown, executable unknown)\n\
             warn: the daemon (pid 42) is from a quickhop too old to report its version and \
             executable: `quickhop autostart on` restarts it\n"
        );
    }

    #[test]
    fn a_daemon_outside_the_installed_app_warns() {
        let lines = daemon_lines(
            running(Some("0.2.0"), Some(CARGO), None),
            "0.2.0",
            Path::new(APP),
        );
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(
            lines[1],
            Line::Warn(format!(
                "the daemon runs {CARGO}, not {APP}: quickhop.app is installed, and its \
                 Accessibility permission does not cover another program; `quickhop autostart \
                 on` restarts it from {APP}"
            ))
        );
    }

    #[test]
    fn a_daemon_that_cannot_be_asked_warns() {
        let error = "talking to the daemon: Permission denied (os error 13)";
        assert_eq!(
            daemon_lines(Daemon::Failed(error), "0.2.0", Path::new(CARGO)),
            [Line::Warn(format!("daemon: {error}"))]
        );
    }

    #[test]
    fn no_daemon_is_a_fact_not_a_warning() {
        assert_eq!(
            daemon_lines(Daemon::NotRunning, "0.2.0", Path::new(CARGO)),
            [Line::Info(
                "daemon: not running (`quickhop autostart on` starts it now and at every login)"
                    .into()
            )]
        );
    }

    #[test]
    fn macos_untrusted_names_who_to_switch_on() {
        let pane = "System Settings › Privacy & Security › Accessibility (Device Control and \
                    Data Access on macOS 27)";
        assert_eq!(
            accessibility_line(
                running(Some("0.2.0"), Some(APP), Some(false)),
                true,
                Path::new(APP)
            )
            .to_string(),
            format!(
                "warn: accessibility: the daemon may not paste: switch on quickhop.app \
                 (/Applications/quickhop.app) in {pane}. When it is on already, it is an older \
                 build: switch it off and on, or remove it with − and add it again. `quickhop \
                 onboarding` opens the pane"
            )
        );
        assert_eq!(
            accessibility_line(Daemon::NotRunning, false, Path::new(CARGO)).to_string(),
            format!(
                "warn: accessibility: not allowed for this terminal, so pasting from it only \
                 copies; the daemon needs it for the binary {CARGO}: switch that on in {pane} \
                 (`quickhop onboarding` opens the pane)"
            )
        );
        assert_eq!(
            accessibility_line(
                running(Some("0.2.0"), Some(APP), Some(true)),
                false,
                Path::new(APP)
            ),
            Line::Info(
                "accessibility: allowed for the daemon, quickhop.app (/Applications/quickhop.app)"
                    .into()
            )
        );
    }

    #[test]
    fn x11_without_xclip_warns_with_the_install_command() {
        let unknown = linux_tool_line("xclip", Display::X11, false, None);
        assert_eq!(
            unknown.to_string(),
            "warn: xclip missing (needed to copy on X11): sudo apt install xclip / sudo dnf \
             install xclip / sudo pacman -S xclip"
        );
        assert_eq!(
            linux_tool_line("xclip", Display::X11, false, Some("ID=fedora\n")).to_string(),
            "warn: xclip missing (needed to copy on X11): sudo dnf install xclip"
        );
        // Only this session's tool matters.
        assert_eq!(
            linux_tool_line("wl-copy", Display::X11, false, None),
            Line::Info("tool wl-copy: missing (not used in this x11 session)".into())
        );
        assert_eq!(
            linux_tool_line("xclip", Display::None, false, None),
            Line::Info("tool xclip: missing (not used without a graphical session)".into())
        );
        assert_eq!(
            linux_tool_line("xclip", Display::X11, true, None),
            Line::Info("tool xclip: found".into())
        );
    }

    #[test]
    fn another_quickhop_first_on_the_path_warns() {
        let this = Path::new(CARGO);
        let local = PathBuf::from("/Users/a/.local/bin/quickhop");
        let lines = binary_lines(this, &[local.clone(), this.to_path_buf()], true);
        assert_eq!(
            text(&lines),
            format!(
                "binary: {CARGO}\ncopy: /Users/a/.local/bin/quickhop\nwarn: `quickhop` on the \
                 PATH runs /Users/a/.local/bin/quickhop, not this binary: the one you type and \
                 the one `autostart on` registers can differ; remove the copies you do not use\n"
            )
        );
        assert_eq!(
            binary_lines(this, &[this.to_path_buf(), PathBuf::from(APP)], true).len(),
            2
        );
        // Only the app, not on the PATH: nothing runs when `quickhop` is typed.
        let app_only = binary_lines(this, &[PathBuf::from(APP)], false);
        assert_eq!(app_only.len(), 3, "{app_only:?}");
        assert!(
            app_only[2]
                .to_string()
                .starts_with("path: no `quickhop` on the PATH")
        );
        let none = binary_lines(this, &[], false);
        assert!(
            none[1]
                .to_string()
                .starts_with("path: no `quickhop` on the PATH")
        );
    }
}
