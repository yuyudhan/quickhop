//! `quickhop daemon`: the long-running process, one per user (an exclusive lock on
//! `daemon.lock`), reached over the Unix socket `daemon.sock`, both in
//! [`runtime_dir`](quickhop_core::config::runtime_dir). It watches the clipboard, rebuilds the
//! file index and keeps it current, and serves `status`, `open`, `reload` and `stop`
//! requests: one JSON line in, one JSON line out per connection. The launcher window joins it
//! with the UI slices.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use quickhop_core::config::runtime_dir;
use quickhop_core::{Cancel, Config};
use quickhop_index::{Index, Watcher};
use quickhop_providers::Env;
use quickhop_store::clipboard::Retention;
use serde::{Deserialize, Serialize};

use crate::clipboard::{self, Watching};
use crate::{LaunchRequest, launch, load_config, now};

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
    },
    /// Read the configuration again and restart the watchers with it.
    Reload,
    /// Exit.
    Stop,
}

/// The daemon's answer.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Response {
    /// Whether the request succeeded.
    pub ok: bool,
    /// The daemon's process id.
    pub pid: u32,
    /// What to print: the result, or the error.
    pub message: String,
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
    let dir = runtime_dir(|name| std::env::var(name).ok());
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
    stream.shutdown(std::net::Shutdown::Write)?;
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

/// The running services and the configuration they were started with.
struct State {
    config: Config,
    retention: Arc<Mutex<Retention>>,
    clipboard: Result<Watching, String>,
    index: Result<IndexService, String>,
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

fn retention(config: &Config) -> Retention {
    Retention {
        max_entries: config.clipboard.max_entries,
        max_age_days: config.clipboard.max_age_days,
    }
}

/// Rebuilds the index of `config`'s roots on a thread, then keeps it current; why not when
/// file search is off or there is nothing to index.
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

impl State {
    fn start(config: Config) -> Self {
        let retention = Arc::new(Mutex::new(retention(&config)));
        let clipboard = clipboard::start(Arc::clone(&retention), now).map_err(|e| e.to_string());
        let index = start_index(&config);
        Self {
            config,
            retention,
            clipboard,
            index,
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
        format!("clipboard: {clipboard}; file index: {index}")
    }

    fn reload(&mut self) -> Result<String, String> {
        let config = load_config().map_err(|e| e.to_string())?;
        *self
            .retention
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = retention(&config);
        if let Ok(index) = std::mem::replace(&mut self.index, Err(String::new())) {
            index.stop();
        }
        self.index = start_index(&config);
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
        let pid = std::process::id();
        let (ok, message) = match request {
            Request::Status => (true, format!("running (pid {pid}); {}", self.status())),
            Request::Open {
                id,
                query,
                args,
                yes,
                action,
            } => {
                let request = LaunchRequest {
                    id: &id,
                    query: &query,
                    args: &args,
                    yes,
                    dry_run: false,
                    action: action.as_deref(),
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
            Request::Stop => (true, "stopping".into()),
        };
        Response { ok, pid, message }
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

/// Runs the daemon until a `stop` request.
///
/// # Errors
/// When another daemon runs, or the lock or socket cannot be set up.
pub(crate) fn serve(config: Config, out: &mut impl Write) -> Result<(), DaemonError> {
    let paths = paths();
    let _lock = lock(&paths)?;
    // Holding the lock means any socket left is from a daemon that died.
    if paths.socket.exists() {
        std::fs::remove_file(&paths.socket).map_err(file_error(&paths.socket))?;
    }
    let listener = UnixListener::bind(&paths.socket).map_err(file_error(&paths.socket))?;
    let mut state = State::start(config);
    writeln!(
        out,
        "quickhop daemon {} listening on {}; {}",
        std::process::id(),
        paths.socket.display(),
        state.status()
    )?;
    out.flush()?;
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
                let stop = matches!(request, Request::Stop);
                (state.handle(request), stop)
            }
            Err(message) => (
                Response {
                    ok: false,
                    pid: std::process::id(),
                    message,
                },
                false,
            ),
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
    let _ = std::fs::remove_file(&paths.socket);
    state.stop();
    Ok(())
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
