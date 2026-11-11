//! `quickhop daemon`: the long-running process, one per user (an exclusive lock on
//! `daemon.lock`), reached over the Unix socket `daemon.sock`, both in
//! [`runtime_dir`](quickhop_core::config::runtime_dir). It watches the clipboard, rebuilds the
//! file index and keeps it current, and serves `status`, `open`, `toggle`, `show`, `hide`,
//! `onboarding`, `settings`, `app-opened`, `reload` and `stop` requests: one JSON line in, one
//! JSON line out per connection. On macOS it owns the launcher, onboarding and settings
//! windows and the menu-bar icon: the main thread runs the windows, the requests are served on
//! a thread of their own. On Linux it shows the tray icon. It reads the config again when
//! `config.toml` or `snippets.toml` is saved ([`crate::config_watch`]).

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use quickhop_core::config::runtime_dir;
use quickhop_core::{Cancel, Config, Flavour, ItemId, QuickhopCommand};
use quickhop_index::{Index, Watcher};
use quickhop_providers::Env;
use serde::{Deserialize, Serialize};

use crate::clipboard::{self, Settings, Watching};
use crate::{LaunchRequest, launch, load_config, now_ms};

/// How long a client waits for a started daemon to answer.
const START_TIMEOUT: Duration = Duration::from_secs(5);

/// How long one request may take to arrive.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// What a client asks the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub(crate) enum Request {
    /// Report the pid and what the daemon watches.
    Status,
    /// Run an item's action, as `quickhop launch` does.
    Open {
        id: String,
        query: String,
        args: Vec<String>,
        yes: bool,
        action: Option<String>,
        #[serde(default)]
        paste: bool,
    },
    /// Read the configuration again and restart the watchers with it.
    Reload,
    /// Show the launcher window, or hide it when shown.
    Toggle,
    /// Show the launcher window.
    Show,
    /// Hide the launcher window.
    Hide,
    /// Open the onboarding window: whether quickhop may paste, and how to allow it.
    Onboarding,
    /// Open the settings: the settings window (macOS), else `config.toml` in the editor.
    Settings,
    /// quickhop.app was opened again: show the launcher, or the onboarding window while
    /// quickhop may not paste.
    AppOpened,
    /// Exit.
    Stop,
}

/// How `quickhop daemon` runs.
#[derive(Debug, Default)]
pub(crate) struct Options {
    /// Whether to open the launcher window (macOS; other systems have none yet).
    pub window: bool,
    /// Where to answer `snapshot` with what the window shows (the UI tests).
    pub test_socket: Option<PathBuf>,
    /// Whether this is quickhop.app being opened: once the window runs, show it, or the
    /// onboarding window while quickhop may not paste.
    #[cfg(target_os = "macos")]
    pub app: bool,
}

/// The launcher window, once the app runs: set by the main thread, used by the request thread.
#[cfg(target_os = "macos")]
type Ui = Arc<std::sync::OnceLock<quickhop_ui::window::Handle>>;

/// The daemon's answer.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Response {
    /// Whether the request succeeded.
    pub ok: bool,
    /// The daemon's process id.
    pub pid: u32,
    /// What to print: the result, or the error.
    pub message: String,
    /// The daemon's quickhop version (`None`: a daemon from before replies carried it).
    #[serde(default)]
    pub version: Option<String>,
    /// The executable the daemon runs from, as the system reports it (`None`: a daemon from
    /// before replies carried it, or one that could not tell).
    #[serde(default)]
    pub exe: Option<PathBuf>,
    /// Whether the daemon may press keys in other apps (macOS Accessibility, as macOS answers
    /// the daemon's own process); `None` where no permission is needed or a daemon from before
    /// replies carried it.
    #[serde(default)]
    pub may_paste: Option<bool>,
}

impl Response {
    /// This process's answer: its pid, version and executable `exe`.
    fn new(ok: bool, message: String, exe: Option<&Path>) -> Self {
        Self {
            ok,
            pid: std::process::id(),
            message,
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            exe: exe.map(Path::to_path_buf),
            #[cfg(target_os = "macos")]
            may_paste: Some(quickhop_platform_macos::keys::is_trusted()),
            #[cfg(not(target_os = "macos"))]
            may_paste: None,
        }
    }
}

/// The first part of the `status` answer: `running (pid N); version V; exe PATH`, the exe
/// left out when unknown.
fn running_line(pid: u32, version: &str, exe: Option<&Path>) -> String {
    match exe {
        Some(exe) => format!(
            "running (pid {pid}); version {version}; exe {}",
            exe.display()
        ),
        None => format!("running (pid {pid}); version {version}"),
    }
}

/// Why talking to or running the daemon failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum DaemonError {
    #[error("quickhop daemon is already running (pid {0})")]
    AlreadyRunning(String),
    #[error("{path}: {source}")]
    File {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("the quickhop daemon is not running (start it with `quickhop daemon`)")]
    NotRunning,
    #[error("the quickhop daemon did not start within {} s", START_TIMEOUT.as_secs())]
    NoStart,
    #[error("talking to the daemon: {0}")]
    Io(#[from] io::Error),
    #[error("bad message from the daemon: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error("{0}")]
    Failed(String),
}

/// The lock file and socket.
struct Paths {
    dir: PathBuf,
    lock: PathBuf,
    socket: PathBuf,
}

fn paths() -> Paths {
    let dir = runtime_dir(Flavour::CURRENT, |name| std::env::var(name).ok());
    Paths {
        lock: dir.join("daemon.lock"),
        socket: dir.join("daemon.sock"),
        dir,
    }
}

fn file_error(path: &Path) -> impl FnOnce(io::Error) -> DaemonError + '_ {
    move |source| DaemonError::File {
        path: path.to_path_buf(),
        source,
    }
}

/// Sends `request` to the running daemon and returns its answer.
///
/// # Errors
/// [`DaemonError::NotRunning`] when no daemon listens; I/O and protocol errors otherwise.
pub(crate) fn send(request: &Request) -> Result<Response, DaemonError> {
    let mut stream = match UnixStream::connect(paths().socket) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err(DaemonError::NotRunning);
        }
        Err(error) => return Err(error.into()),
    };
    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    // A daemon that answers at once and ends (`stop`) may have closed the connection already;
    // its answer is still there to read.
    if let Err(error) = stream.shutdown(std::net::Shutdown::Write)
        && error.kind() != io::ErrorKind::NotConnected
    {
        return Err(error.into());
    }
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    Ok(serde_json::from_str(reply.trim())?)
}

/// Like [`send`], first starting a daemon in the background when none runs.
///
/// # Errors
/// As [`send`], or [`DaemonError::NoStart`] when the started daemon never answers.
pub(crate) fn send_starting(request: &Request) -> Result<Response, DaemonError> {
    match send(request) {
        Err(DaemonError::NotRunning) => {}
        other => return other,
    }
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // Its own process group, so closing this terminal does not stop it.
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn()?;
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match send(&Request::Status) {
            Ok(_) => break,
            Err(DaemonError::NotRunning) if Instant::now() < deadline => {
                // A daemon that exited (say, one that lost the race to another) is reaped.
                let _ = child.try_wait();
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(DaemonError::NotRunning) => return Err(DaemonError::NoStart),
            Err(error) => return Err(error),
        }
    }
    send(request)
}

/// Asks the running daemon, pid `pid`, to stop and waits until it has ended: until its lock
/// is free, so a new daemon can take it.
///
/// # Errors
/// As [`send`], or [`DaemonError::Failed`] when it still holds the lock after
/// [`START_TIMEOUT`].
pub(crate) fn stop_and_wait(pid: u32) -> Result<(), DaemonError> {
    match send(&Request::Stop) {
        Ok(_) | Err(DaemonError::NotRunning) => {}
        Err(error) => return Err(error),
    }
    let paths = paths();
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        let file = match OpenOptions::new().read(true).write(true).open(&paths.lock) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(file_error(&paths.lock)(error)),
        };
        match file.try_lock() {
            // Unlocked when `file` is dropped: free for the caller's daemon.
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(DaemonError::Failed(format!(
                    "the quickhop daemon (pid {pid}) did not stop within {} s",
                    START_TIMEOUT.as_secs()
                )));
            }
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(file_error(&paths.lock)(error));
            }
        }
    }
}

/// The running services and the configuration they were started with.
struct State {
    config: Config,
    /// The executable this daemon runs from, for every reply.
    exe: Option<PathBuf>,
    clipboard_settings: Settings,
    clipboard: Result<Watching, String>,
    index: Result<IndexService, String>,
    /// What the window searches, once it runs; reloaded with the configuration.
    #[cfg(target_os = "macos")]
    launcher: Option<Arc<crate::window::Launcher>>,
    #[cfg(target_os = "macos")]
    ui: Ui,
    /// The launcher hotkey as bound, or why it is not, for `status`.
    #[cfg(target_os = "macos")]
    hotkey: Arc<Mutex<String>>,
    /// The menu-bar icon (macOS) or tray icon (Linux): `on`, or `off (why)`, for `status`.
    menu_bar: Arc<Mutex<String>>,
    /// The tray icon, while the daemon shows one.
    #[cfg(not(target_os = "macos"))]
    tray: Option<crate::tray::TraySlot>,
    /// Reload on save, kept running while the daemon runs (`None` until [`serve`] starts it).
    config_watch: Option<Result<crate::config_watch::ConfigWatch, String>>,
}

/// The file index being rebuilt, then watched, on its own thread.
struct IndexService {
    cancel: Cancel,
    watcher: Arc<Mutex<Option<Watcher>>>,
    roots: usize,
}

impl IndexService {
    fn describe(&self) -> String {
        let watching = self
            .watcher
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some();
        let doing = if watching { "watching" } else { "rebuilding" };
        format!("{doing} {} roots", self.roots)
    }

    fn stop(self) {
        self.cancel.cancel();
        // Dropping the watcher stops it.
        drop(
            self.watcher
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }
}

/// Rebuilds the index of `config`'s roots on a thread, then keeps it current; why not when
/// file search is off, there is nothing to index, or (on macOS, where Spotlight answers file
/// search) no index has been built with `quickhop index`.
fn start_index(config: &Config) -> Result<IndexService, String> {
    if !config.providers.files {
        return Err("[providers] files = false".into());
    }
    let env = Env::current();
    let options = quickhop_providers::index_options(config, &env);
    if options.roots.is_empty() {
        return Err("no [files] roots and no HOME".into());
    }
    let path = quickhop_providers::index_path(&env).map_err(|e| e.to_string())?;
    if env.os == "macos" && !path.is_file() {
        return Err(
            "Spotlight answers file search; `quickhop index` builds an index instead".into(),
        );
    }
    let service = IndexService {
        cancel: Cancel::new(),
        watcher: Arc::new(Mutex::new(None)),
        roots: options.roots.len(),
    };
    let (cancel, slot) = (service.cancel.clone(), Arc::clone(&service.watcher));
    let spawned = std::thread::Builder::new()
        .name("quickhop-index".into())
        .spawn(move || {
            let run = || -> Result<(), Box<dyn std::error::Error>> {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let mut index = Index::open(&path)?;
                // Events missed while no daemon ran are caught up by a full rebuild.
                index.rebuild(&options, &cancel)?;
                if cancel.is_cancelled() {
                    return Ok(());
                }
                let watcher = Watcher::start(Arc::new(Mutex::new(index)), options)?;
                *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(watcher);
                Ok(())
            };
            if let Err(error) = run() {
                let _ = writeln!(io::stderr(), "quickhop daemon: file index: {error}");
            }
        });
    spawned.map(|_| service).map_err(|e| e.to_string())
}

/// Starts the clipboard watcher when `[clipboard] enabled` is set.
fn start_clipboard(settings: &Settings) -> Result<Watching, String> {
    let enabled = settings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .enabled;
    if !enabled {
        return Err("[clipboard] enabled = false".into());
    }
    clipboard::start(Arc::clone(settings), now_ms).map_err(|e| e.to_string())
}

impl State {
    fn start(config: Config) -> Self {
        let clipboard_settings = Arc::new(Mutex::new(config.clipboard.clone()));
        let clipboard = start_clipboard(&clipboard_settings);
        let index = start_index(&config);
        Self {
            config,
            exe: std::env::current_exe().ok(),
            clipboard_settings,
            clipboard,
            index,
            #[cfg(target_os = "macos")]
            launcher: None,
            #[cfg(target_os = "macos")]
            ui: Ui::default(),
            #[cfg(target_os = "macos")]
            hotkey: Arc::new(Mutex::new(NO_WINDOW_HOTKEY.into())),
            menu_bar: Arc::new(Mutex::new(NO_MENU_BAR.into())),
            #[cfg(not(target_os = "macos"))]
            tray: None,
            config_watch: None,
        }
    }

    fn status(&self) -> String {
        let clipboard = match &self.clipboard {
            Ok(watching) => format!("watching ({})", watching.session()),
            Err(error) => format!("off ({error})"),
        };
        let index = match &self.index {
            Ok(service) => service.describe(),
            Err(why) => format!("off ({why})"),
        };
        #[cfg(target_os = "macos")]
        let window = match (&self.launcher, self.ui.get()) {
            (None, _) => "off",
            (Some(_), None) => "starting",
            (Some(_), Some(ui)) => match ui.send(quickhop_ui::window::Command::Snapshot) {
                Ok(snapshot) if snapshot.visible => "shown",
                Ok(_) => "hidden",
                Err(_) => "gone",
            },
        };
        #[cfg(target_os = "macos")]
        let hotkey = self
            .hotkey
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let menu_bar = self
            .menu_bar
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let watch = match &self.config_watch {
            Some(Ok(_)) => "on".to_owned(),
            Some(Err(why)) => format!("off ({why})"),
            None => "off".to_owned(),
        };
        #[cfg(target_os = "macos")]
        return format!(
            "clipboard: {clipboard}; file index: {index}; reload on save: {watch}; window: \
             {window}; menu bar: {menu_bar}; hotkey: {hotkey}"
        );
        #[cfg(not(target_os = "macos"))]
        format!(
            "clipboard: {clipboard}; file index: {index}; reload on save: {watch}; window: not \
             built for this system yet; tray: {menu_bar}"
        )
    }

    fn reload(&mut self) -> Result<String, String> {
        let config = load_config().map_err(|e| e.to_string())?;
        *self
            .clipboard_settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = config.clipboard.clone();
        // Restart when the setting changed, too: a watcher that could not start (no graphical
        // session) must report "[clipboard] enabled = false" once the config turns it off.
        if config.clipboard.enabled != self.config.clipboard.enabled
            || config.clipboard.enabled != self.clipboard.is_ok()
        {
            if let Ok(watching) = &mut self.clipboard {
                watching.stop();
            }
            self.clipboard = start_clipboard(&self.clipboard_settings);
        }
        // The index restarts only when what it covers changed, or it is not running (an index
        // built since, or one that failed): a settings switch must not rebuild it.
        if config.files != self.config.files
            || config.providers.files != self.config.providers.files
            || self.index.is_err()
        {
            if let Ok(index) = std::mem::replace(&mut self.index, Err(String::new())) {
                index.stop();
            }
            self.index = start_index(&config);
        }
        #[cfg(target_os = "macos")]
        if let Some(launcher) = &self.launcher {
            launcher.reload(&config);
        }
        #[cfg(target_os = "macos")]
        if config.hotkey != self.config.hotkey
            && let Some(ui) = self.ui.get()
        {
            *self.hotkey.lock().unwrap_or_else(PoisonError::into_inner) =
                bind_hotkey(ui, &config.hotkey);
        }
        if config.menu_bar_icon != self.config.menu_bar_icon {
            self.show_menu_bar(config.menu_bar_icon);
        }
        self.config = config;
        Ok(format!("reloaded; {}", self.status()))
    }

    fn stop(mut self) {
        if let Ok(watching) = &mut self.clipboard {
            watching.stop();
        }
        if let Ok(index) = self.index {
            index.stop();
        }
    }

    fn handle(&mut self, request: Request) -> Response {
        if let Request::Open { id, .. } = &request
            && let Some(command) = own_command(id)
        {
            let (ok, message) = self.own(command);
            return Response::new(ok, message, self.exe.as_deref());
        }
        let (ok, message) = match request {
            Request::Status => {
                let running = running_line(
                    std::process::id(),
                    env!("CARGO_PKG_VERSION"),
                    self.exe.as_deref(),
                );
                (true, format!("{running}; {}", self.status()))
            }
            Request::Open {
                id,
                query,
                args,
                yes,
                action,
                paste,
            } => {
                let request = LaunchRequest {
                    id: &id,
                    query: &query,
                    args: &args,
                    yes,
                    dry_run: false,
                    action: action.as_deref(),
                    paste,
                };
                let mut out = Vec::new();
                match launch(&self.config, &request, &mut out) {
                    Ok(()) => (true, String::from_utf8_lossy(&out).trim_end().to_owned()),
                    Err(error) => (false, error.to_string()),
                }
            }
            Request::Reload => match self.reload() {
                Ok(message) => (true, message),
                Err(message) => (false, message),
            },
            #[cfg(target_os = "macos")]
            Request::Toggle => window(&self.ui, quickhop_ui::window::Command::Toggle),
            #[cfg(target_os = "macos")]
            Request::Show => window(&self.ui, quickhop_ui::window::Command::Show),
            #[cfg(target_os = "macos")]
            Request::Hide => window(&self.ui, quickhop_ui::window::Command::Hide),
            #[cfg(target_os = "macos")]
            Request::Onboarding => window(&self.ui, quickhop_ui::window::Command::Onboarding),
            Request::Settings => self.settings(),
            #[cfg(target_os = "macos")]
            Request::AppOpened => window(&self.ui, quickhop_ui::window::Command::AppOpened),
            #[cfg(not(target_os = "macos"))]
            Request::Toggle
            | Request::Show
            | Request::Hide
            | Request::Onboarding
            | Request::AppOpened => (
                false,
                "the launcher window is not built for this system yet".into(),
            ),
            Request::Stop => (true, "stopping".into()),
        };
        Response::new(ok, message, self.exe.as_deref())
    }

    /// Opens the settings: the settings window while the launcher window runs, else
    /// `config.toml` in the default editor, which this daemon reloads when it is saved.
    fn settings(&self) -> (bool, String) {
        #[cfg(target_os = "macos")]
        if self.ui.get().is_some() {
            return window(&self.ui, quickhop_ui::window::Command::Settings);
        }
        let opened = crate::settings::config_file()
            .and_then(|path| crate::settings::open_in_editor(&path).map(|()| path));
        let applies = if matches!(self.config_watch, Some(Ok(_))) {
            "quickhop reads it again when you save"
        } else {
            "run `quickhop reload` after saving"
        };
        match opened {
            Ok(path) => (
                true,
                format!("opened {} in the editor; {applies}", path.display()),
            ),
            Err(error) => (false, error),
        }
    }

    /// Runs one of quickhop's own commands asked for by id (`quickhop open quickhop:reload`):
    /// here, not through `launch`, which would ask this very daemon over its socket.
    fn own(&mut self, command: QuickhopCommand) -> (bool, String) {
        match command {
            QuickhopCommand::Settings => self.settings(),
            QuickhopCommand::Reload => match self.reload() {
                Ok(message) => (true, message),
                Err(message) => (false, message),
            },
            QuickhopCommand::About => (true, crate::settings::about().trim_end().to_owned()),
            QuickhopCommand::Quit => (true, "stopping".into()),
        }
    }

    /// Shows or removes the menu-bar icon (macOS, once the window runs) or the tray icon
    /// (Linux, in a daemon with a display), recording the status line.
    fn show_menu_bar(&mut self, on: bool) {
        #[cfg(target_os = "macos")]
        let status = match self.ui.get() {
            Some(ui) => show_status_item(ui, on),
            None => NO_MENU_BAR.to_owned(),
        };
        #[cfg(not(target_os = "macos"))]
        let status = match &self.tray {
            Some(tray) => tray.set(on),
            None => NO_MENU_BAR.to_owned(),
        };
        *self.menu_bar.lock().unwrap_or_else(PoisonError::into_inner) = status;
    }
}

/// The menu-bar status of a daemon without a window or display.
const NO_MENU_BAR: &str = "off (no window)";

/// quickhop's own command the item `id` names (`quickhop:settings`, …).
fn own_command(id: &str) -> Option<QuickhopCommand> {
    ItemId::parse(id).and_then(|id| quickhop_providers::quickhop::command(&id))
}

/// Whether the daemon stops after answering `request`: `stop`, or Quit quickhop by id.
fn stops(request: &Request) -> bool {
    match request {
        Request::Stop => true,
        Request::Open { id, .. } => own_command(id) == Some(QuickhopCommand::Quit),
        _ => false,
    }
}

/// Carries out a window `command`; says whether the launcher is shown afterwards, or, when
/// the onboarding window came instead, what it says.
#[cfg(target_os = "macos")]
fn window(ui: &Ui, command: quickhop_ui::window::Command) -> (bool, String) {
    use quickhop_ui::window::Command;

    let Some(ui) = ui.get() else {
        return (
            false,
            "this daemon runs without the launcher window (--no-window, or no display to open \
             it on)"
                .into(),
        );
    };
    let snapshot = match ui.send(command) {
        Ok(snapshot) => snapshot,
        Err(gone) => return (false, gone.to_string()),
    };
    if command == Command::Settings {
        return (true, "settings shown".into());
    }
    let onboarding =
        command == Command::Onboarding || (command == Command::AppOpened && !snapshot.visible);
    if !onboarding {
        let shown = if snapshot.visible { "shown" } else { "hidden" };
        return (true, shown.into());
    }
    match snapshot.onboarding {
        Some(status) if status.granted => (true, "onboarding: Accessibility allowed".into()),
        Some(_) => (true, "onboarding: Accessibility not allowed".into()),
        None => (
            false,
            "the onboarding window did not open (the daemon's output says why)".into(),
        ),
    }
}

impl State {
    /// The window's backend, loaded on first use.
    #[cfg(target_os = "macos")]
    fn launcher(&mut self) -> Arc<crate::window::Launcher> {
        let hotkey = Arc::clone(&self.hotkey);
        Arc::clone(
            self.launcher.get_or_insert_with(|| {
                Arc::new(crate::window::Launcher::new(&self.config, hotkey))
            }),
        )
    }
}

/// Takes the lock, or names the pid that holds it.
fn lock(paths: &Paths) -> Result<File, DaemonError> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&paths.dir).map_err(file_error(&paths.dir))?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&paths.lock)
        .map_err(file_error(&paths.lock))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            let mut pid = String::new();
            let _ = file.read_to_string(&mut pid);
            return Err(DaemonError::AlreadyRunning(pid.trim().to_owned()));
        }
        Err(std::fs::TryLockError::Error(error)) => return Err(file_error(&paths.lock)(error)),
    }
    file.set_len(0).map_err(file_error(&paths.lock))?;
    write!(file, "{}", std::process::id()).map_err(file_error(&paths.lock))?;
    Ok(file)
}

fn read_request(stream: &UnixStream) -> Result<Request, String> {
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(line.trim()).map_err(|e| format!("bad request: {e}"))
}

/// Runs the daemon until a `stop` request. On macOS, unless `options.window` is off, this
/// thread runs the launcher window and the requests are served on another.
///
/// # Errors
/// When another daemon runs, or the lock or socket cannot be set up.
pub(crate) fn serve(
    config: Config,
    options: &Options,
    out: &mut impl Write,
) -> Result<(), DaemonError> {
    let paths = paths();
    let lock = lock(&paths)?;
    // Holding the lock means any socket left is from a daemon that died.
    if paths.socket.exists() {
        std::fs::remove_file(&paths.socket).map_err(file_error(&paths.socket))?;
    }
    let listener = UnixListener::bind(&paths.socket).map_err(file_error(&paths.socket))?;
    let mut state = State::start(config);
    state.config_watch = Some(crate::settings::config_file().and_then(|path| {
        crate::config_watch::start(&path, || {
            // Through the socket, as `quickhop reload`: the state lives on the request thread.
            if let Err(error) = send(&Request::Reload).and_then(message) {
                let _ = writeln!(io::stderr(), "quickhop daemon: reload on save: {error}");
            }
        })
    }));
    #[cfg(target_os = "macos")]
    let launcher = options.window.then(|| {
        *state.hotkey.lock().unwrap_or_else(PoisonError::into_inner) = "starting".into();
        *state
            .menu_bar
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = "starting".into();
        state.launcher()
    });
    #[cfg(not(target_os = "macos"))]
    if options.window || options.test_socket.is_some() {
        crate::warn("the launcher window is not built for this system yet");
    }
    // The tray needs a desktop session, which a daemon without a window has none of.
    #[cfg(not(target_os = "macos"))]
    if options.window {
        state.tray = Some(crate::tray::TraySlot::default());
        state.show_menu_bar(state.config.menu_bar_icon);
    }
    writeln!(
        out,
        "quickhop daemon {} listening on {}; {}",
        std::process::id(),
        paths.socket.display(),
        state.status()
    )?;
    out.flush()?;
    #[cfg(target_os = "macos")]
    let ui = Arc::clone(&state.ui);
    #[cfg(target_os = "macos")]
    let (hotkey, hotkey_text) = (Arc::clone(&state.hotkey), state.config.hotkey.clone());
    #[cfg(target_os = "macos")]
    let (menu_bar, menu_bar_on) = (Arc::clone(&state.menu_bar), state.config.menu_bar_icon);
    // Whether this thread is in the window's run loop, which only the window can end.
    #[cfg(target_os = "macos")]
    let window_running = Arc::new(AtomicBool::new(launcher.is_some()));
    #[cfg(target_os = "macos")]
    let running = Arc::clone(&window_running);
    let requests = std::thread::Builder::new()
        .name("quickhop-daemon".into())
        .spawn(move || {
            let _lock = lock;
            let served = accept(&listener, &mut state);
            let _ = std::fs::remove_file(&paths.socket);
            #[cfg(target_os = "macos")]
            let ui = Arc::clone(&state.ui);
            state.stop();
            #[cfg(target_os = "macos")]
            match ui.get() {
                // Ends the app, and with it the process, once everything is stopped.
                Some(ui) => {
                    let _ = ui.send(quickhop_ui::window::Command::Quit);
                }
                // The window is still starting (seconds on a slow GPU), so nothing can ask it
                // to quit, and once up it would run on alone: everything is stopped, so the
                // process ends here.
                None if running.load(Ordering::SeqCst) && served.is_ok() => std::process::exit(0),
                None => {}
            }
            served
        })?;
    #[cfg(target_os = "macos")]
    if let Some(launcher) = launcher {
        let test_socket = options.test_socket.clone();
        let app = options.app;
        let backend: Arc<dyn quickhop_ui::window::Backend> = Arc::clone(&launcher) as _;
        // Returns early only when the window cannot be opened; the requests go on without it.
        quickhop_ui::window::run(backend, move |handle| {
            let _ = ui.set(handle.clone());
            *hotkey.lock().unwrap_or_else(PoisonError::into_inner) =
                bind_hotkey(&handle, &hotkey_text);
            *menu_bar.lock().unwrap_or_else(PoisonError::into_inner) =
                show_status_item(&handle, menu_bar_on);
            crate::window::refresh_inline(Arc::clone(&launcher), handle.clone());
            if app {
                let _ = handle.send(quickhop_ui::window::Command::AppOpened);
            }
            // Last: a test socket that answers means the window and the hotkey are ready.
            if let Some(path) = test_socket {
                serve_snapshots(&path, handle, launcher);
            }
        });
        window_running.store(false, Ordering::SeqCst);
    }
    requests
        .join()
        .unwrap_or_else(|_| Err(DaemonError::Failed("the request thread panicked".into())))
}

/// Answers requests until a `stop`.
fn accept(listener: &UnixListener, state: &mut State) -> Result<(), DaemonError> {
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                let _ = writeln!(io::stderr(), "quickhop daemon: {error}");
                continue;
            }
        };
        let (response, stop) = match read_request(&stream) {
            Ok(request) => {
                let stop = stops(&request);
                (state.handle(request), stop)
            }
            Err(message) => (Response::new(false, message, state.exe.as_deref()), false),
        };
        let mut line = serde_json::to_string(&response)?;
        line.push('\n');
        if let Err(error) = stream.write_all(line.as_bytes()) {
            let _ = writeln!(io::stderr(), "quickhop daemon: {error}");
        }
        if stop {
            break;
        }
    }
    Ok(())
}

/// The hotkey status of a daemon without the launcher window.
#[cfg(target_os = "macos")]
const NO_WINDOW_HOTKEY: &str = "off (no launcher window)";

#[cfg(target_os = "macos")]
thread_local! {
    /// The launcher hotkey's registration, kept by the main thread that owns it.
    static HOTKEY: std::cell::RefCell<Option<quickhop_platform_macos::hotkey::Registration>> =
        const { std::cell::RefCell::new(None) };
}

/// Binds `text` as the hotkey that toggles the window, replacing the one bound before, and
/// returns the status line: the hotkey, or `off (why)`. Waits for the main thread, so it is
/// called from another one.
#[cfg(target_os = "macos")]
fn bind_hotkey(ui: &quickhop_ui::window::Handle, text: &str) -> String {
    const GONE: &str = "off (the launcher window is gone)";
    let hotkey = match quickhop_core::Hotkey::parse(text) {
        Ok(hotkey) => hotkey,
        Err(error) => return format!("off ({error})"),
    };
    let (reply, answer) = std::sync::mpsc::channel();
    let toggler = ui.clone();
    let sent = ui.on_main_thread(move || {
        // Released first, so binding the same combination again succeeds.
        HOTKEY.with_borrow_mut(Option::take);
        // The press arrives on the main thread, which `send` waits for: toggle from another.
        let on_press = move || {
            let ui = toggler.clone();
            std::thread::spawn(move || ui.send(quickhop_ui::window::Command::Toggle));
        };
        let status = match quickhop_platform_macos::hotkey::register(hotkey, on_press) {
            Ok(registration) => {
                HOTKEY.set(Some(registration));
                hotkey.to_string()
            }
            Err(error) => format!("off ({error})"),
        };
        let _ = reply.send(status);
    });
    match sent {
        Ok(()) => answer.recv().unwrap_or_else(|_| GONE.into()),
        Err(_) => GONE.into(),
    }
}

#[cfg(target_os = "macos")]
thread_local! {
    /// The menu-bar icon, kept by the main thread that owns it.
    static STATUS_ITEM: std::cell::RefCell<Option<quickhop_platform_macos::status_item::StatusItem>> =
        const { std::cell::RefCell::new(None) };
}

/// Puts quickhop's icon in the menu bar when `on` (Open quickhop, Settings…, Quit quickhop),
/// else removes it; returns the status line: `on`, or `off (why)`. Waits for the main
/// thread, so it is called from another one.
#[cfg(target_os = "macos")]
fn show_status_item(ui: &quickhop_ui::window::Handle, on: bool) -> String {
    use quickhop_platform_macos::status_item::{self, Entry};
    use quickhop_ui::window::Command;

    const GONE: &str = "off (the launcher window is gone)";
    let (reply, answer) = std::sync::mpsc::channel();
    let window = ui.clone();
    let sent = ui.on_main_thread(move || {
        if !on {
            STATUS_ITEM.with_borrow_mut(Option::take);
            let _ = reply.send("off (menu_bar_icon = false)".to_owned());
            return;
        }
        if STATUS_ITEM.with_borrow(Option::is_some) {
            let _ = reply.send("on".to_owned());
            return;
        }
        // Menu choices arrive on the main thread, which `send` waits for: send from another.
        let command = |command: Command| -> Box<dyn Fn()> {
            let window = window.clone();
            Box::new(move || {
                let window = window.clone();
                std::thread::spawn(move || window.send(command));
            })
        };
        let entries = vec![
            Entry::Command {
                title: "Open quickhop".into(),
                key: String::new(),
                action: command(Command::Show),
            },
            Entry::Command {
                title: "Settings…".into(),
                key: ",".into(),
                action: command(Command::Settings),
            },
            Entry::Separator,
            Entry::Command {
                title: "Quit quickhop".into(),
                key: "q".into(),
                action: Box::new(|| {
                    std::thread::spawn(|| {
                        if let Err(error) = send(&Request::Stop) {
                            let _ = writeln!(io::stderr(), "quickhop daemon: quit: {error}");
                        }
                    });
                }),
            },
        ];
        let status = match status_item::install(Flavour::CURRENT.app_name(), entries) {
            Ok(item) => {
                STATUS_ITEM.set(Some(item));
                "on".to_owned()
            }
            Err(error) => format!("off ({error})"),
        };
        let _ = reply.send(status);
    });
    match sent {
        Ok(()) => answer.recv().unwrap_or_else(|_| GONE.into()),
        Err(_) => GONE.into(),
    }
}

/// Answers the UI tests on the Unix socket `path`, one JSON line per line asked: `snapshot`
/// with what the window shows ([`quickhop_ui::model::Snapshot`]), `advance <seconds>` by
/// moving the clock of inline script refreshes ahead, `permission granted|denied|system` by
/// setting what the onboarding window hears when it asks whether quickhop may paste.
#[cfg(target_os = "macos")]
fn serve_snapshots(
    path: &Path,
    ui: quickhop_ui::window::Handle,
    launcher: Arc<crate::window::Launcher>,
) {
    let _ = std::fs::remove_file(path);
    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(error) => {
            crate::warn(&format!("test socket {}: {error}", path.display()));
            return;
        }
    };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let ui = ui.clone();
            let launcher = Arc::clone(&launcher);
            std::thread::spawn(move || answer_snapshots(stream, &ui, &launcher));
        }
    });
}

#[cfg(target_os = "macos")]
fn answer_snapshots(
    stream: UnixStream,
    ui: &quickhop_ui::window::Handle,
    launcher: &crate::window::Launcher,
) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        let line = line.trim();
        let answer = if line == "snapshot" {
            match ui.send(quickhop_ui::window::Command::Snapshot) {
                Ok(snapshot) => serde_json::to_value(snapshot)
                    .unwrap_or_else(|error| serde_json::json!({ "error": error.to_string() })),
                Err(gone) => serde_json::json!({ "error": gone.to_string() }),
            }
        } else if let Some(seconds) = line
            .strip_prefix("advance ")
            .and_then(|s| s.trim().parse::<u64>().ok())
        {
            launcher.advance(Duration::from_secs(seconds));
            serde_json::json!({ "advanced": seconds })
        } else if let Some(answer) = line.strip_prefix("permission ") {
            let granted = match answer.trim() {
                "granted" => Ok(Some(true)),
                "denied" => Ok(Some(false)),
                "system" => Ok(None),
                other => Err(other),
            };
            match granted {
                Ok(granted) => {
                    launcher.set_permission(granted);
                    serde_json::json!({ "permission": answer.trim() })
                }
                Err(other) => serde_json::json!({
                    "error": format!("permission {other:?}: expected granted, denied or system")
                }),
            }
        } else {
            serde_json::json!({ "error": format!("unknown command {line:?}") })
        };
        if writeln!(writer, "{answer}").is_err() {
            return;
        }
    }
}

/// The message of a successful `response`, or its error.
///
/// # Errors
/// [`DaemonError::Failed`] when the daemon reported a failure.
pub(crate) fn message(response: Response) -> Result<String, DaemonError> {
    if response.ok {
        Ok(response.message)
    } else {
        Err(DaemonError::Failed(response.message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reply_names_the_daemons_version_and_executable() {
        let exe = Path::new("/Applications/quickhop.app/Contents/MacOS/quickhop");
        let reply = serde_json::to_value(Response::new(true, "shown".into(), Some(exe))).unwrap();
        assert_eq!(reply["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(reply["exe"], exe.to_str().unwrap());
        assert_eq!(reply["pid"], std::process::id());
        assert_eq!(reply["may_paste"].is_boolean(), cfg!(target_os = "macos"));
    }

    #[test]
    fn a_reply_from_an_older_daemon_has_no_version_or_executable() {
        let reply: Response =
            serde_json::from_str(r#"{"ok":true,"pid":7,"message":"running"}"#).unwrap();
        assert_eq!(reply.version, None);
        assert_eq!(reply.exe, None);
        assert_eq!(reply.may_paste, None);
        assert_eq!(message(reply).unwrap(), "running");
    }

    #[test]
    fn the_status_keeps_its_pid_prefix_then_names_version_and_executable() {
        let exe = Path::new("/Users/me/.cargo/bin/quickhop");
        assert_eq!(
            running_line(7, "1.2.3", Some(exe)),
            "running (pid 7); version 1.2.3; exe /Users/me/.cargo/bin/quickhop"
        );
        assert_eq!(
            running_line(7, "1.2.3", None),
            "running (pid 7); version 1.2.3"
        );
    }
}
