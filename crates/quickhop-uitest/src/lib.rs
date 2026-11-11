//! The UI test harness (`docs/plan.md`, "How the UI is tested"): it runs the real
//! `quickhop daemon` with a private configuration, data and runtime directory, drives it
//! with real input as a person would, and checks what it shows through the daemon's
//! `--test-socket` snapshot and the window server.
//!
//! The macOS driver (**[ax]**) posts key events through the HID event tap, so the process
//! running the tests needs the Accessibility permission. Keys are only ever sent while the
//! daemon is the frontmost app, and a test refuses to start while someone is using the Mac.
//! The tests run with `just ui-test`, which builds the binary first. Only Unix has the
//! daemon, so the crate is empty elsewhere.

#![cfg(unix)]
#![forbid(unsafe_code)]

use std::error::Error;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// What a harness step returns.
pub type Fallible<T = ()> = Result<T, Box<dyn Error>>;

/// How long a check waits for the window to get there.
pub const PATIENCE: Duration = Duration::from_secs(5);

/// The private pasteboard the daemon under test copies to (`QUICKHOP_PASTEBOARD`), so the
/// tests never touch the user's clipboard.
pub const PASTEBOARD: &str = "dev.quickhop.uitest";

/// The `quickhop` binary under test: `QUICKHOP_BIN`, else the workspace's debug build.
pub fn binary() -> PathBuf {
    std::env::var_os("QUICKHOP_BIN").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/quickhop"),
        PathBuf::from,
    )
}

/// A running `quickhop daemon` over private directories; stopped when dropped.
pub struct Daemon {
    child: Child,
    dir: tempfile::TempDir,
    /// The daemon's process id.
    pub pid: i32,
    /// The pasteboard it copies to, `None` for the general one.
    pasteboard: Option<&'static str>,
}

/// The configuration the tests run with: no file search, whose results depend on the
/// machine's files and on Spotlight.
pub const TEST_CONFIG: &str = "[providers]\nfiles = false\n";

impl Daemon {
    /// Starts the daemon with the launcher window and [`TEST_CONFIG`], and waits until its
    /// test socket answers.
    ///
    /// # Errors
    /// When the binary is missing or the daemon does not come up.
    pub fn start() -> Fallible<Self> {
        Self::start_with(TEST_CONFIG)
    }

    /// Starts the daemon with the launcher window and `config` as its `config.toml`.
    ///
    /// # Errors
    /// As [`Self::start`].
    pub fn start_with(config: &str) -> Fallible<Self> {
        Self::start_with_files(config, &[])
    }

    /// As [`Self::start_with`], with `files` (a path relative to the daemon's directory and
    /// its content) written first: `snippets.toml` sits next to `config.toml` there.
    ///
    /// # Errors
    /// As [`Self::start`], or when a file cannot be written.
    pub fn start_with_files(config: &str, files: &[(&str, &str)]) -> Fallible<Self> {
        Self::launch(config, files, Some(PASTEBOARD))
    }

    /// As [`Self::start_with`], but on the general pasteboard, the one `⌘V` pastes from: for
    /// a paste test only, which borrows the user's clipboard with [`ax::Clipboard`].
    ///
    /// # Errors
    /// As [`Self::start`].
    pub fn start_on_the_general_pasteboard(config: &str) -> Fallible<Self> {
        Self::launch(config, &[], None)
    }

    fn launch(
        config: &str,
        files: &[(&str, &str)],
        pasteboard: Option<&'static str>,
    ) -> Fallible<Self> {
        let binary = binary();
        if !binary.is_file() {
            return Err(format!("{} is missing: run `just ui-test`", binary.display()).into());
        }
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("run"))?;
        std::fs::write(dir.path().join("config.toml"), config)?;
        for (path, content) in files {
            std::fs::write(dir.path().join(path), content)?;
        }
        let log = std::fs::File::create(dir.path().join("daemon.log"))?;
        let child = Self::command_in(dir.path(), pasteboard)
            .arg("daemon")
            .arg("--test-socket")
            .arg(dir.path().join("test.sock"))
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        let pid = i32::try_from(child.id())?;
        let daemon = Self {
            child,
            dir,
            pid,
            pasteboard,
        };
        daemon
            .wait_for("the test socket", |_| true)
            .map_err(|error| format!("{error}; daemon output: {}", daemon.log()))?;
        Ok(daemon)
    }

    /// What the daemon printed so far, for failure messages.
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("daemon.log")).unwrap_or_default()
    }

    /// Replaces the daemon's `config.toml` with `config` and runs `quickhop reload`.
    ///
    /// # Errors
    /// When the file cannot be written or the daemon refuses the reload.
    pub fn reload_with(&self, config: &str) -> Fallible {
        std::fs::write(self.dir.path().join("config.toml"), config)?;
        let output = self.quickhop(&["reload"])?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "reload failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into())
        }
    }

    /// `quickhop` with this daemon's private configuration, data, runtime directory and
    /// pasteboard (`None`: the general one).
    fn command_in(dir: &Path, pasteboard: Option<&str>) -> Command {
        let mut command = Command::new(binary());
        command
            .env("QUICKHOP_CONFIG", dir.join("config.toml"))
            .env("QUICKHOP_DATA", dir.join("data"))
            .env("QUICKHOP_RUNTIME", dir.join("run"))
            .stdin(Stdio::null());
        match pasteboard {
            Some(name) => command.env("QUICKHOP_PASTEBOARD", name),
            None => command.env_remove("QUICKHOP_PASTEBOARD"),
        };
        command
    }

    /// Runs `quickhop <args>` against this daemon.
    ///
    /// # Errors
    /// When the command cannot run.
    pub fn quickhop(&self, args: &[&str]) -> Fallible<Output> {
        Ok(Self::command_in(self.dir.path(), self.pasteboard)
            .args(args)
            .output()?)
    }

    /// What the window shows now, from the test socket.
    ///
    /// # Errors
    /// When the socket does not answer with JSON.
    pub fn snapshot(&self) -> Fallible<Value> {
        self.ask("snapshot")
    }

    /// Moves the daemon's clock for inline script refreshes `seconds` ahead.
    ///
    /// # Errors
    /// When the socket does not confirm it.
    pub fn advance(&self, seconds: u64) -> Fallible {
        let answer = self.ask(&format!("advance {seconds}"))?;
        if answer["advanced"] == seconds {
            Ok(())
        } else {
            Err(format!("advance {seconds}: {answer}").into())
        }
    }

    /// Sets what the onboarding window hears when it asks whether quickhop may paste:
    /// `granted`, `denied`, or `system` to ask macOS again.
    ///
    /// # Errors
    /// When the socket does not confirm it.
    pub fn set_permission(&self, answer: &str) -> Fallible {
        let reply = self.ask(&format!("permission {answer}"))?;
        if reply["permission"] == answer {
            Ok(())
        } else {
            Err(format!("permission {answer}: {reply}").into())
        }
    }

    /// Sends `line` on the test socket and reads the JSON line answering it.
    fn ask(&self, line: &str) -> Fallible<Value> {
        let mut stream = UnixStream::connect(self.dir.path().join("test.sock"))?;
        stream.set_read_timeout(Some(PATIENCE))?;
        writeln!(stream, "{line}")?;
        let mut answer = String::new();
        BufReader::new(stream).read_line(&mut answer)?;
        Ok(serde_json::from_str(&answer)?)
    }

    /// The snapshot once `check` holds for it, or an error naming `what` after
    /// [`PATIENCE`].
    ///
    /// # Errors
    /// When `check` never holds.
    pub fn wait_for(&self, what: &str, check: impl Fn(&Value) -> bool) -> Fallible<Value> {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let last = match self.snapshot() {
                Ok(snapshot) if check(&snapshot) => return Ok(snapshot),
                Ok(snapshot) => snapshot.to_string(),
                Err(error) => error.to_string(),
            };
            if Instant::now() > deadline {
                #[cfg(target_os = "macos")]
                let last = format!("{last}; {}", ax::focus());
                return Err(format!("waited for {what}; last: {last}").into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Asks for `quickhop daemon stop` and waits for the process to end.
    fn stop(&mut self) {
        let _ = self.quickhop(&["daemon", "stop"]);
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "macos")]
pub mod ax {
    //! The macOS driver: key events through the HID event tap, the frontmost app and the
    //! window list from the window server, `screencapture` for images.

    use std::io::Write as _;
    use std::process::Command;
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::time::{Duration, Instant};

    use quickhop_core::{ClipboardSource as _, Contents, Hotkey};
    use quickhop_platform_macos::input::{self, focused_pid};
    pub use quickhop_platform_macos::input::{
        Key, OnScreen, app_name, focused_window_title, frontmost_pid, is_running, quit,
        windows_on_screen,
    };
    use quickhop_platform_macos::pasteboard::{Pasteboard, Saved};

    use super::{Daemon, Fallible, PATIENCE};

    /// Seconds without keyboard or pointer input before the tests may take over the screen.
    pub const IDLE_BEFORE_TESTS: f64 = 60.0;

    /// Whether this test process checked that nobody is using the Mac. The tests' own key
    /// events count as input, so the check is made once, before the first test.
    static SCREEN: Mutex<bool> = Mutex::new(false);

    /// The screen, held by one test at a time: the tests share one display and one
    /// frontmost app.
    pub struct Screen {
        _held: MutexGuard<'static, bool>,
    }

    /// Takes the screen for one test, waiting for the test before it to end. The first one
    /// refuses to go on while a person is using the Mac: the tests post real keys. Every one
    /// refuses while the screen is locked, where those keys would reach the password field.
    ///
    /// # Errors
    /// Without a window server session, while the screen is locked, when there was input in
    /// the last [`IDLE_BEFORE_TESTS`] seconds, or when keys cannot be posted.
    pub fn take_over() -> Fallible<Screen> {
        let mut checked = SCREEN.lock().unwrap_or_else(PoisonError::into_inner);
        match input::screen_locked() {
            None => return Err("the window server reports no session: log in first".into()),
            Some(true) => return Err("the screen is locked: unlock it first".into()),
            Some(false) => {}
        }
        if !*checked {
            if !quickhop_platform_macos::keys::is_trusted() {
                return Err(
                    "this process may not post key events: grant the terminal Accessibility".into(),
                );
            }
            let idle = input::seconds_since_input();
            if idle < IDLE_BEFORE_TESTS {
                return Err(format!(
                    "someone used this Mac {idle:.0} s ago; the UI tests need it idle for \
                     {IDLE_BEFORE_TESTS} s"
                )
                .into());
            }
            *checked = true;
        }
        Ok(Screen { _held: checked })
    }

    impl Daemon {
        /// The daemon's window on screen, if any.
        pub fn window(&self) -> Option<OnScreen> {
            windows_on_screen()
                .into_iter()
                .find(|window| window.pid == self.pid)
        }

        /// The daemon's window once on screen with the keyboard focus. The window is a
        /// non-activating panel, so the app in front may stay in front meanwhile.
        ///
        /// # Errors
        /// When it does not show within [`PATIENCE`].
        pub fn wait_for_window(&self) -> Fallible<OnScreen> {
            let deadline = Instant::now() + PATIENCE;
            loop {
                if let Some(window) = self.window()
                    && focused_pid() == Some(self.pid)
                {
                    return Ok(window);
                }
                if Instant::now() > deadline {
                    return Err(format!(
                        "no focused window of the daemon (pid {}); on screen: {:?}, {}; \
                         daemon output: {}",
                        self.pid,
                        self.window(),
                        focus(),
                        self.log()
                    )
                    .into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        /// Waits until the daemon has no window on screen.
        ///
        /// # Errors
        /// When one is still there after [`PATIENCE`].
        pub fn wait_for_no_window(&self) -> Fallible {
            let deadline = Instant::now() + PATIENCE;
            while let Some(window) = self.window() {
                if Instant::now() > deadline {
                    return Err(format!("the window is still on screen: {window:?}").into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(())
        }

        /// Fails unless the daemon has the keyboard focus, so keys never reach another app.
        fn ensure_focused(&self) -> Fallible {
            match focused_pid() {
                Some(pid) if pid == self.pid => Ok(()),
                other => Err(format!(
                    "not sending keys: the focus is in pid {other:?}, not the daemon ({})",
                    self.pid
                )
                .into()),
            }
        }

        /// Presses `key` (with `⌘` when `command`) in the window.
        ///
        /// # Errors
        /// When the daemon does not have the focus or the event cannot be posted.
        pub fn press(&self, key: Key, command: bool) -> Fallible {
            self.ensure_focused()?;
            input::press(key, command)?;
            Ok(())
        }

        /// Types `text` in the window.
        ///
        /// # Errors
        /// As [`Self::press`].
        pub fn type_text(&self, text: &str) -> Fallible {
            self.ensure_focused()?;
            input::type_text(text)?;
            Ok(())
        }

        /// Presses the key with modifiers `text` names (`Alt+Backspace`, as a hotkey is
        /// written) in the window.
        ///
        /// # Errors
        /// When `text` is not a key with modifiers, or as [`Self::press`].
        pub fn press_shortcut(&self, text: &str) -> Fallible {
            let shortcut = Hotkey::parse(text)?;
            self.ensure_focused()?;
            input::press_hotkey(&shortcut)?;
            Ok(())
        }

        /// Presses the global hotkey `text`, wherever the focus is, once `quickhop daemon
        /// status` says the daemon holds it: otherwise the keys would reach another app.
        ///
        /// # Errors
        /// When the daemon does not hold the hotkey or the events cannot be posted.
        pub fn press_hotkey(&self, text: &str) -> Fallible {
            let hotkey = Hotkey::parse(text)?;
            let status = String::from_utf8(self.quickhop(&["daemon", "status"])?.stdout)?;
            if !status.contains(&format!("hotkey: {hotkey};"))
                && !status.trim_end().ends_with(&format!("hotkey: {hotkey}"))
            {
                return Err(format!("not pressing {hotkey}: the daemon says {status:?}").into());
            }
            input::press_hotkey(&hotkey)?;
            Ok(())
        }
    }

    /// Which apps have the keyboard focus and the front, by process id and bundle
    /// identifier, for failure messages.
    pub fn focus() -> String {
        let name = |pid: Option<i32>| match pid {
            Some(pid) => format!("{pid} ({})", input::app_name(pid).unwrap_or_default()),
            None => "none".to_owned(),
        };
        format!(
            "focus: {}, frontmost: {}",
            name(focused_pid()),
            name(frontmost_pid())
        )
    }

    /// The text on the daemon's private pasteboard ([`super::PASTEBOARD`]), if any.
    pub fn copied() -> Option<String> {
        match Pasteboard::named(super::PASTEBOARD).contents() {
            Contents::Text { text, .. } => Some(text),
            Contents::Secret | Contents::Other => None,
        }
    }

    /// Puts `text` on the daemon's private pasteboard, so a test sees what it changes.
    ///
    /// # Errors
    /// When the pasteboard refuses it.
    pub fn set_copied(text: &str) -> Fallible {
        if Pasteboard::named(super::PASTEBOARD).write_text(text, false) {
            Ok(())
        } else {
            Err("the private pasteboard refused the text".into())
        }
    }

    /// The user's clipboard, borrowed by a test that pastes (`⌘V` pastes from the general
    /// pasteboard) and put back, every item and type, when dropped. Declare it before the
    /// daemon, so the daemon is gone before the clipboard comes back.
    pub struct Clipboard {
        saved: Saved,
    }

    impl Clipboard {
        /// Saves what the general pasteboard holds now.
        pub fn borrow() -> Self {
            Self {
                saved: Pasteboard::general().save(),
            }
        }

        /// Copies `text` to the general pasteboard, as an app would.
        ///
        /// # Errors
        /// When the pasteboard refuses it.
        pub fn copy(&self, text: &str) -> Fallible {
            if Pasteboard::general().write_text(text, false) {
                Ok(())
            } else {
                Err("the general pasteboard refused the text".into())
            }
        }
    }

    impl Drop for Clipboard {
        fn drop(&mut self) {
            if !Pasteboard::general().restore(&self.saved) {
                let _ = writeln!(
                    std::io::stderr(),
                    "quickhop-uitest: the clipboard could not be put back"
                );
            }
        }
    }

    /// The bundle identifier of [`PasteTarget`].
    const PASTE_TARGET: &str = "dev.quickhop.pastetarget";

    const PASTE_TARGET_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>dev.quickhop.pastetarget</string>
    <key>CFBundleExecutable</key>
    <string>PasteTarget</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
</dict>
</plist>
"#;

    /// The app a paste test pastes into: one text window in front, built with `swiftc` from
    /// `tests/fixtures/paste_target.swift`, that writes what it holds to a file. Quit when
    /// dropped.
    pub struct PasteTarget {
        dir: tempfile::TempDir,
    }

    impl PasteTarget {
        /// Builds the app, opens it and waits until it is in front.
        ///
        /// # Errors
        /// When one already runs, `swiftc` or `open` fails, or it does not come to the front
        /// within [`PATIENCE`].
        pub fn open() -> Fallible<Self> {
            if is_running(PASTE_TARGET) {
                return Err("a paste target from an earlier test still runs".into());
            }
            let dir = tempfile::tempdir()?;
            let app = dir.path().join("PasteTarget.app");
            std::fs::create_dir_all(app.join("Contents/MacOS"))?;
            std::fs::write(app.join("Contents/Info.plist"), PASTE_TARGET_PLIST)?;
            let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/paste_target.swift");
            let built = Command::new("swiftc")
                .arg("-o")
                .arg(app.join("Contents/MacOS/PasteTarget"))
                .arg(&source)
                .status()?;
            if !built.success() {
                return Err(format!("swiftc {} failed: {built}", source.display()).into());
            }
            let target = Self { dir };
            let opened = Command::new("open")
                .arg("-n")
                .arg(&app)
                .arg("--args")
                .arg(target.file())
                .status()?;
            if !opened.success() {
                return Err(format!("open {} failed: {opened}", app.display()).into());
            }
            let deadline = Instant::now() + PATIENCE;
            while frontmost_pid().and_then(input::app_name).as_deref() != Some(PASTE_TARGET) {
                if Instant::now() > deadline {
                    return Err(format!("the paste target is not in front; {}", focus()).into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(target)
        }

        fn file(&self) -> std::path::PathBuf {
            self.dir.path().join("text.txt")
        }

        /// The text in its window, as last written to its file.
        pub fn text(&self) -> String {
            std::fs::read_to_string(self.file()).unwrap_or_default()
        }
    }

    impl Drop for PasteTarget {
        fn drop(&mut self) {
            quit(PASTE_TARGET);
            let deadline = Instant::now() + PATIENCE;
            while is_running(PASTE_TARGET) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    /// Captures `window` with `screencapture -l` into a PNG and returns its size in bytes.
    ///
    /// # Errors
    /// When `screencapture` fails (it needs the Screen Recording permission).
    pub fn capture(window: &OnScreen, path: &std::path::Path) -> Fallible<u64> {
        let status = std::process::Command::new("screencapture")
            .args(["-x", "-o", "-l"])
            .arg(window.id.to_string())
            .arg(path)
            .status()?;
        if !status.success() {
            return Err(format!("screencapture failed: {status}").into());
        }
        Ok(std::fs::metadata(path)?.len())
    }

    /// The pixels of the PNG capture at `path`: its RGB or RGBA samples, row after row, its
    /// width in pixels and its samples per pixel.
    ///
    /// # Errors
    /// When the file is no PNG.
    pub fn pixels(path: &std::path::Path) -> Fallible<(Vec<u8>, u32, usize)> {
        let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?));
        let mut reader = decoder.read_info()?;
        let mut pixels = vec![0; reader.output_buffer_size().ok_or("image too large")?];
        let frame = reader.next_frame(&mut pixels)?;
        pixels.truncate(frame.buffer_size());
        Ok((pixels, frame.width, frame.color_type.samples()))
    }

    /// The bundle identifier of System Settings.
    pub const SYSTEM_SETTINGS: &str = "com.apple.systempreferences";

    /// System Settings, which a test opens: quit when dropped. The test needs it closed
    /// before, so it never quits a window the user had open.
    pub struct SystemSettings;

    impl SystemSettings {
        /// Takes System Settings for one test.
        ///
        /// # Errors
        /// When it runs already.
        pub fn closed() -> Fallible<Self> {
            if is_running(SYSTEM_SETTINGS) {
                return Err("System Settings is open: quit it first (the test opens it)".into());
            }
            Ok(Self)
        }
    }

    impl Drop for SystemSettings {
        fn drop(&mut self) {
            quit(SYSTEM_SETTINGS);
            let deadline = Instant::now() + PATIENCE;
            while is_running(SYSTEM_SETTINGS) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    /// The app bundle built around [`super::binary`] by `scripts/bundle-macos.sh` ("quickhop
    /// Dev.app" for the debug build, "quickhop.app" for a release) and opened with `open`, as
    /// Finder opens it, over private directories as [`Daemon`] uses; stopped when dropped.
    pub struct App {
        dir: tempfile::TempDir,
        /// The bundle, as the script printed it.
        bundle: std::path::PathBuf,
        /// The app's process id, which is its daemon's.
        pub pid: i32,
    }

    impl App {
        /// Bundles the binary under test with `config`, opens the bundle and waits until its
        /// daemon answers.
        ///
        /// # Errors
        /// When bundling or `open` fail, or no daemon answers within [`PATIENCE`].
        pub fn open(config: &str) -> Fallible<Self> {
            let dir = tempfile::tempdir()?;
            std::fs::create_dir(dir.path().join("run"))?;
            std::fs::write(dir.path().join("config.toml"), config)?;
            let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scripts/bundle-macos.sh");
            let bundled = Command::new(&script)
                .arg("--bin")
                .arg(super::binary())
                .arg("--out")
                .arg(dir.path())
                .output()?;
            if !bundled.status.success() {
                return Err(format!(
                    "{} failed: {}",
                    script.display(),
                    String::from_utf8_lossy(&bundled.stderr)
                )
                .into());
            }
            let printed = String::from_utf8(bundled.stdout)?;
            let bundle = std::path::PathBuf::from(
                printed
                    .lines()
                    .last()
                    .ok_or_else(|| format!("{} printed no bundle", script.display()))?,
            );
            let path = |name: &str| dir.path().join(name).display().to_string();
            let opened = Command::new("open")
                .arg("-n")
                .args(["--env", &format!("QUICKHOP_CONFIG={}", path("config.toml"))])
                .args(["--env", &format!("QUICKHOP_DATA={}", path("data"))])
                .args(["--env", &format!("QUICKHOP_RUNTIME={}", path("run"))])
                .args([
                    "--env",
                    &format!("QUICKHOP_PASTEBOARD={}", super::PASTEBOARD),
                ])
                .args(["--stdout", &path("app.log")])
                .args(["--stderr", &path("app.err")])
                .arg(&bundle)
                .status()?;
            if !opened.success() {
                return Err(format!("open {} failed: {opened}", bundle.display()).into());
            }
            let deadline = Instant::now() + PATIENCE;
            loop {
                let status = Daemon::command_in(dir.path(), Some(super::PASTEBOARD))
                    .args(["daemon", "status"])
                    .output()?;
                let pid = String::from_utf8_lossy(&status.stdout)
                    .strip_prefix("running (pid ")
                    .and_then(|rest| rest.split(')').next()?.parse().ok());
                if let Some(pid) = pid {
                    return Ok(Self { dir, bundle, pid });
                }
                if Instant::now() > deadline {
                    let log =
                        std::fs::read_to_string(dir.path().join("app.err")).unwrap_or_default();
                    return Err(
                        format!("{}'s daemon never answered: {log}", bundle.display()).into(),
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        /// The bundle's identifier, from its Info.plist.
        ///
        /// # Errors
        /// When `plutil` cannot read it.
        pub fn bundle_id(&self) -> Fallible<String> {
            let plist = self.bundle.join("Contents/Info.plist");
            let read = Command::new("/usr/bin/plutil")
                .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
                .arg(&plist)
                .output()?;
            if !read.status.success() {
                return Err(format!("plutil cannot read {}", plist.display()).into());
            }
            Ok(String::from_utf8(read.stdout)?.trim().to_owned())
        }

        /// Runs `quickhop <args>` against the app's daemon.
        ///
        /// # Errors
        /// When the command cannot run.
        pub fn quickhop(&self, args: &[&str]) -> Fallible<std::process::Output> {
            Ok(Daemon::command_in(self.dir.path(), Some(super::PASTEBOARD))
                .args(args)
                .output()?)
        }

        /// The app's window on screen that `matches`, once there.
        ///
        /// # Errors
        /// When none is within [`PATIENCE`].
        pub fn wait_for_window(&self, matches: impl Fn(&OnScreen) -> bool) -> Fallible<OnScreen> {
            let deadline = Instant::now() + PATIENCE;
            loop {
                let windows: Vec<OnScreen> = windows_on_screen()
                    .into_iter()
                    .filter(|window| window.pid == self.pid)
                    .collect();
                if let Some(window) = windows.iter().find(|window| matches(window)) {
                    return Ok(*window);
                }
                if Instant::now() > deadline {
                    return Err(format!(
                        "no such window of {} (pid {}); its windows: {windows:?}; {}",
                        self.bundle.display(),
                        self.pid,
                        focus()
                    )
                    .into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    impl Drop for App {
        fn drop(&mut self) {
            let _ = self.quickhop(&["daemon", "stop"]);
            let deadline = Instant::now() + PATIENCE;
            while app_name(self.pid).is_some() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}
