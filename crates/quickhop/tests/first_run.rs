//! End-to-end tests of the first run: `autostart` registering and starting the daemon, the
//! app-menu entry's `daemon --notify`, and what `doctor` says about them. Each test has its own
//! home, configuration and runtime directory, so none touches the user's login items or daemon.
#![cfg(unix)]

use std::error::Error;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

type TestResult = Result<(), Box<dyn Error>>;

/// A private home, XDG configuration folder and daemon runtime directory.
struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Result<Self, Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("config.toml"), "")?;
        Ok(Self { dir })
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn config_home(&self) -> PathBuf {
        self.path().join("config")
    }

    fn runtime(&self) -> PathBuf {
        self.path().join("run")
    }

    /// The login entry `autostart on` writes on this system.
    fn entry(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.path()
                .join("Library/LaunchAgents/io.github.yuyudhan.quickhop.plist")
        } else {
            self.config_home().join("autostart/quickhop.desktop")
        }
    }

    /// The binary in this home, in a GNOME session without a display, with `PATH` holding
    /// only the system folders (no notification tools of the user's).
    fn cmd(&self) -> Result<Command, Box<dyn Error>> {
        let mut cmd = Command::cargo_bin("quickhop")?;
        cmd.env("HOME", self.path())
            .env("XDG_CONFIG_HOME", self.config_home())
            .env("QUICKHOP_CONFIG", self.path().join("config.toml"))
            .env("QUICKHOP_DATA", self.path().join("data"))
            .env("QUICKHOP_RUNTIME", self.runtime())
            .env("QUICKHOP_PASTEBOARD", "dev.quickhop.test.first-run")
            .env("XDG_CURRENT_DESKTOP", "GNOME")
            .env("PATH", "/usr/bin:/bin")
            .env_remove("XDG_SESSION_DESKTOP")
            .env_remove("SWAYSOCK")
            .env_remove("I3SOCK")
            .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
            .env_remove("XDG_DATA_HOME")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY");
        Ok(cmd)
    }
}

impl Drop for Home {
    /// Stops a daemon a test started, however the test ends.
    fn drop(&mut self) {
        if let Ok(mut cmd) = self.cmd() {
            let _ = cmd.args(["daemon", "stop"]).output();
        }
    }
}

fn run(cmd: &mut Command) -> Result<(bool, String, String), Box<dyn Error>> {
    let output = cmd.output()?;
    Ok((
        output.status.success(),
        String::from_utf8(output.stdout)?,
        String::from_utf8(output.stderr)?,
    ))
}

fn stdout(cmd: &mut Command) -> Result<String, Box<dyn Error>> {
    match run(cmd)? {
        (true, out, _) => Ok(out),
        (false, out, err) => Err(format!("quickhop failed: {out}{err}").into()),
    }
}

#[test]
fn autostart_on_no_start_registers_and_status_reads_the_program_back() -> TestResult {
    let home = Home::new()?;
    let on = stdout(home.cmd()?.args(["autostart", "on", "--no-start"]))?;
    assert!(on.starts_with("quickhop daemon starts at login: "), "{on}");
    assert!(!on.contains("started the quickhop daemon"), "{on}");
    assert!(home.entry().is_file());
    let status = stdout(home.cmd()?.args(["autostart", "status"]))?;
    assert!(
        status.starts_with(&format!("on: {} runs ", home.entry().display())),
        "{status}"
    );
    assert!(!status.contains("warn:"), "{status}");
    let doctor = stdout(home.cmd()?.arg("doctor"))?;
    assert!(doctor.contains("\nautostart: on: "), "{doctor}");
    assert!(doctor.contains("\ndaemon: not running"), "{doctor}");
    assert!(doctor.contains("\nbinary: "), "{doctor}");
    Ok(())
}

#[test]
fn an_entry_for_an_uninstalled_binary_warns_and_strict_fails() -> TestResult {
    let home = Home::new()?;
    stdout(home.cmd()?.args(["autostart", "on", "--no-start"]))?;
    let entry = home.entry();
    let gone = home.path().join("uninstalled/quickhop");
    let text = std::fs::read_to_string(&entry)?;
    let binary = String::from_utf8(home.cmd()?.args(["autostart", "status"]).output()?.stdout)?;
    let program = binary
        .split(" runs ")
        .nth(1)
        .and_then(|rest| rest.strip_suffix(" daemon\n"))
        .ok_or_else(|| format!("no program in {binary:?}"))?
        .to_owned();
    std::fs::write(&entry, text.replace(&program, &gone.to_string_lossy()))?;

    let status = stdout(home.cmd()?.args(["autostart", "status"]))?;
    let warning = format!(
        "warn: autostart runs {}, which does not exist (moved or uninstalled): `quickhop \
         autostart on` points it at ",
        gone.display()
    );
    assert!(status.contains(&warning), "{status}");
    let doctor = stdout(home.cmd()?.arg("doctor"))?;
    assert!(doctor.contains(&warning), "{doctor}");
    let (ok, out, err) = run(home.cmd()?.args(["doctor", "--strict"]))?;
    assert!(!ok, "{out}");
    assert!(err.contains("doctor: "), "{err}");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
mod linux {
    use super::{Home, TestResult, run, stdout};

    #[test]
    fn autostart_on_starts_the_daemon_now() -> TestResult {
        let home = Home::new()?;
        let on = stdout(home.cmd()?.args(["autostart", "on"]))?;
        assert!(on.contains("\nstarted the quickhop daemon (pid "), "{on}");
        let status = stdout(home.cmd()?.args(["daemon", "status"]))?;
        assert!(status.starts_with("running (pid "), "{status}");
        // Again: the running daemon is this binary's, so it is kept.
        let again = stdout(home.cmd()?.args(["autostart", "on"]))?;
        assert!(
            again.contains("the quickhop daemon already runs (pid "),
            "{again}"
        );
        let doctor = stdout(home.cmd()?.arg("doctor"))?;
        assert!(doctor.contains("\ndaemon: running (pid "), "{doctor}");
        Ok(())
    }

    #[test]
    fn on_sway_autostart_prints_the_exec_line_instead_of_registering() -> TestResult {
        let home = Home::new()?;
        let (ok, _, err) = run(home.cmd()?.env("XDG_CURRENT_DESKTOP", "sway").args([
            "autostart",
            "on",
            "--no-start",
        ]))?;
        assert!(!ok, "{err}");
        let exe = std::fs::canonicalize(env!("CARGO_BIN_EXE_quickhop"))?;
        let line = format!(
            "Add this line to {}:\n    exec {} daemon\n",
            home.config_home().join("sway/config").display(),
            exe.display()
        );
        assert!(err.contains(&line), "{err}");
        assert!(!home.entry().exists());

        // Once the line is there, sway starts it: nothing to do.
        let sway = home.config_home().join("sway/config");
        std::fs::create_dir_all(sway.parent().ok_or("no parent")?)?;
        std::fs::write(&sway, format!("exec {} daemon\n", exe.display()))?;
        let on = stdout(home.cmd()?.env("XDG_CURRENT_DESKTOP", "sway").args([
            "autostart",
            "on",
            "--no-start",
        ]))?;
        assert!(
            on.contains("sway starts quickhop daemon at login from "),
            "{on}"
        );
        // `--xdg` writes the entry anyway, for dex and uwsm.
        stdout(home.cmd()?.env("XDG_CURRENT_DESKTOP", "sway").args([
            "autostart",
            "on",
            "--no-start",
            "--xdg",
        ]))?;
        assert!(home.entry().is_file());
        Ok(())
    }

    #[test]
    fn the_menu_entry_starts_the_daemon_and_says_so() -> TestResult {
        let home = Home::new()?;
        // No notification service in the test: the message goes to standard error as well.
        let (ok, out, err) = run(home
            .cmd()?
            .env("PATH", "/nonexistent")
            .args(["daemon", "--notify"]))?;
        assert!(ok, "{err}");
        assert!(
            out.starts_with("quickhop now runs in the background: pid "),
            "{out}"
        );
        assert!(err.contains("cannot show a desktop notification"), "{err}");
        let (ok, again, _) = run(home
            .cmd()?
            .env("PATH", "/nonexistent")
            .args(["daemon", "--notify"]))?;
        assert!(ok);
        assert!(
            again.starts_with("quickhop is already running: pid "),
            "{again}"
        );
        Ok(())
    }

    #[test]
    fn doctor_names_only_this_sessions_clipboard_tool() -> TestResult {
        let home = Home::new()?;
        let doctor = stdout(
            home.cmd()?
                .env("DISPLAY", ":99")
                .env("PATH", "/nonexistent")
                .arg("doctor"),
        )?;
        assert!(doctor.contains("\nsession: x11\n"), "{doctor}");
        assert!(
            doctor.contains("\nwarn: xclip missing (needed to copy on X11): sudo "),
            "{doctor}"
        );
        assert!(
            doctor.contains("\ntool wl-copy: missing (not used in this x11 session)\n"),
            "{doctor}"
        );
        Ok(())
    }
}

#[cfg(target_os = "macos")]
#[test]
fn doctor_reports_accessibility() -> TestResult {
    let home = Home::new()?;
    let doctor = stdout(home.cmd()?.arg("doctor"))?;
    assert!(doctor.contains("accessibility: "), "{doctor}");
    Ok(())
}
