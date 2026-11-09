//! Spotlight-backed file search against the real index (plan slice F1, m4).
//!
//! Ignored by default: it waits for Spotlight to index a new file, which takes seconds and
//! needs indexing enabled, as on a developer's Mac but not on hosted CI runners. Run it with
//! `cargo test -p quickhop-providers --test providers spotlight -- --include-ignored`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use quickhop_core::{Action, Cancel, Item, Provider, ProviderError, Query};
use quickhop_providers::files::{FilesProvider, SPOTLIGHT_TIMEOUT, Spotlight};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// How long Spotlight gets to index a new file; a busy machine needs well over ten seconds.
const INDEX_WAIT: Duration = Duration::from_secs(90);

/// A directory Spotlight indexes: `target/tmp` lies inside the checkout, under the home
/// directory, while the system temporary directory is never indexed. The name must not
/// start with a dot (tempfile's default `.tmp`): Spotlight skips hidden folders.
fn indexed_dir() -> std::io::Result<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix("quickhop-spotlight-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
}

fn provider(root: &Path) -> FilesProvider {
    FilesProvider::new(
        Box::new(Spotlight::new(root, SPOTLIGHT_TIMEOUT)),
        "macos",
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

/// Polls until an item for `path` is found, or [`INDEX_WAIT`] passes.
fn wait_for(provider: &FilesProvider, text: &str, path: &Path) -> Result<Item, String> {
    let start = Instant::now();
    loop {
        let items = provider
            .items(&Query::new(text), &Cancel::new())
            .map_err(|e| e.to_string())?;
        if let Some(item) = items
            .into_iter()
            .find(|item| item.id.key == path.to_string_lossy())
        {
            return Ok(item);
        }
        if start.elapsed() > INDEX_WAIT {
            return Err(format!(
                "{} not indexed after {INDEX_WAIT:?}",
                path.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn run(action: Option<&Action>) -> TestResult {
    let Some(Action::Run { program, args, .. }) = action else {
        return Err(format!("not a run action: {action:?}").into());
    };
    let status = Command::new(program).args(args).status()?;
    if !status.success() {
        return Err(format!("{program} {args:?} failed: {status}").into());
    }
    Ok(())
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "waits for Spotlight to index a new file; run on m4 with --ignored"
)]
#[cfg_attr(not(target_os = "macos"), ignore = "Spotlight exists only on macOS")]
fn a_freshly_created_file_is_found_and_moved_to_the_trash() -> TestResult {
    let dir = indexed_dir()?;
    let unique = format!(
        "qhfresh-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis()
    );
    let path = dir.path().join(format!("{unique}.txt"));
    std::fs::write(&path, "found by quickhop")?;
    let path = path.canonicalize()?;

    let provider = provider(dir.path());
    let item = wait_for(&provider, &unique, &path)?;
    assert_eq!(item.title, format!("{unique}.txt"));
    assert_eq!(
        item.default_action(),
        Some(&Action::Open {
            target: path.to_string_lossy().into_owned()
        })
    );
    assert!(item.actions.contains(&Action::Copy {
        text: path.to_string_lossy().into_owned()
    }));

    // The last action moves the file to the Trash; it leaves the folder on disk.
    run(item.actions.last())?;
    assert!(!path.exists(), "{} is still there", path.display());
    Ok(())
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "Spotlight exists only on macOS")]
fn a_cancelled_spotlight_query_is_reported_as_cancelled() -> TestResult {
    let dir = indexed_dir()?;
    let cancel = Cancel::new();
    cancel.cancel();
    let result = provider(dir.path()).items(&Query::new("anything"), &cancel);
    assert!(
        matches!(result, Err(ProviderError::Cancelled { .. })),
        "{result:?}"
    );
    Ok(())
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "Spotlight exists only on macOS")]
fn a_name_nothing_has_gives_no_items_and_no_error() -> TestResult {
    let dir = indexed_dir()?;
    let items = provider(dir.path()).items(&Query::new("qqqqzzzz-no-such-file"), &Cancel::new())?;
    assert_eq!(items, []);
    Ok(())
}
