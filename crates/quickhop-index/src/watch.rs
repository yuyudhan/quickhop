//! Keeps an [`Index`] current from filesystem events (inotify on Linux, `FSEvents` on macOS).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::JoinHandle;

use notify::event::ModifyKind;
use notify::{Event, EventKind, RecursiveMode, Watcher as _};
use quickhop_core::Cancel;

use crate::{Index, IndexError, Options};

enum Message {
    Event(notify::Result<Event>),
    Stop,
}

/// A running watcher over the roots of its [`Options`]; dropping it stops watching.
pub struct Watcher {
    sender: mpsc::Sender<Message>,
    worker: Option<JoinHandle<()>>,
    applied: Arc<AtomicUsize>,
    errors: Arc<Mutex<Vec<String>>>,
}

impl Watcher {
    /// Watches `options.roots` recursively and applies every change to `index` on a worker
    /// thread: a path that exists is (re)added with everything under it, one that vanished
    /// is removed with everything under it. New folders are watched as they appear (inotify
    /// misses folders created before their parent's watch exists), and when the OS reports
    /// lost events the index is rebuilt.
    ///
    /// # Errors
    /// When the OS watcher cannot be created or a root cannot be watched.
    pub fn start(index: Arc<Mutex<Index>>, options: Options) -> Result<Self, IndexError> {
        let (sender, receiver) = mpsc::channel();
        let events = sender.clone();
        let mut inner = notify::recommended_watcher(move |event| {
            let _ = events.send(Message::Event(event));
        })?;
        for root in &options.roots {
            inner.watch(root, RecursiveMode::Recursive)?;
        }
        let applied = Arc::new(AtomicUsize::new(0));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let (count, problems) = (Arc::clone(&applied), Arc::clone(&errors));
        let worker = std::thread::Builder::new()
            .name("quickhop-index-watch".into())
            .spawn(move || {
                for message in receiver {
                    let outcome = match message {
                        Message::Stop => break,
                        Message::Event(Ok(event)) => apply(&index, &options, &event, &mut inner),
                        Message::Event(Err(error)) => Err(IndexError::Watch(error)),
                    };
                    if let Err(error) = outcome {
                        problems
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push(error.to_string());
                    }
                    count.fetch_add(1, Ordering::SeqCst);
                }
            })
            .map_err(|error| IndexError::Watch(notify::Error::io(error)))?;
        Ok(Self {
            sender,
            worker: Some(worker),
            applied,
            errors,
        })
    }

    /// Number of events handled so far.
    pub fn events(&self) -> usize {
        self.applied.load(Ordering::SeqCst)
    }

    /// Problems met while applying events, oldest first; taking them clears the list.
    pub fn take_errors(&self) -> Vec<String> {
        std::mem::take(&mut *self.errors.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.sender.send(Message::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn apply(
    index: &Mutex<Index>,
    options: &Options,
    event: &Event,
    watcher: &mut notify::RecommendedWatcher,
) -> Result<(), IndexError> {
    // Opening and reading folders (our own walks included) is not a change.
    if matches!(event.kind, EventKind::Access(_)) {
        return Ok(());
    }
    let mut index = index.lock().unwrap_or_else(PoisonError::into_inner);
    if event.need_rescan() {
        index.rebuild(options, &Cancel::new())?;
        return Ok(());
    }
    let appeared = matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
    );
    let mut paths: Vec<PathBuf> = event.paths.clone();
    paths.dedup();
    for path in paths {
        if appeared && path.is_dir() && !options.is_ignored(&path) {
            watcher.watch(&path, RecursiveMode::Recursive)?;
        }
        index.refresh(options, &path)?;
    }
    Ok(())
}
