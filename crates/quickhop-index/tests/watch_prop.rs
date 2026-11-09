//! Property test of the watcher: every created file is found once the watcher settles, and
//! deleted ones disappear, whatever the names and folder nesting.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use quickhop_core::Cancel;
use quickhop_index::{Index, Options, Watcher};

const SETTLE: Duration = Duration::from_secs(10);

/// Polls `check` until it holds or [`SETTLE`] passes.
fn settles(index: &Mutex<Index>, check: impl Fn(&Index) -> bool) -> bool {
    let deadline = Instant::now() + SETTLE;
    while Instant::now() < deadline {
        if index.lock().is_ok_and(|index| check(&index)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn indexed(index: &Index, path: &Path) -> bool {
    index.contains(path).unwrap_or(false)
}

/// Relative file paths: one or two folder levels, unique.
fn files() -> impl Strategy<Value = BTreeSet<PathBuf>> {
    proptest::collection::btree_set(
        (
            "[a-z]{1,6}",
            proptest::option::of("[a-z]{1,6}"),
            "[a-z0-9]{1,8}\\.(txt|md|rs)",
        )
            .prop_map(|(dir, sub, name)| {
                let mut path = PathBuf::from(dir);
                if let Some(sub) = sub {
                    path.push(sub);
                }
                path.push(name);
                path
            }),
        1..12,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]

    #[test]
    fn created_files_appear_and_deleted_ones_vanish(
        files in files(),
        delete_every in 1usize..4,
    ) {
        let root = tempfile::tempdir().map_err(|e| TestCaseError::fail(e.to_string()))?;
        let options = Options::new(vec![root.path().to_path_buf()]);
        let mut index = Index::in_memory().map_err(|e| TestCaseError::fail(e.to_string()))?;
        index.rebuild(&options, &Cancel::new()).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let index = Arc::new(Mutex::new(index));
        let watcher = Watcher::start(Arc::clone(&index), options)
            .map_err(|e| TestCaseError::fail(e.to_string()))?;

        let paths: Vec<PathBuf> = files.iter().map(|f| root.path().join(f)).collect();
        for path in &paths {
            // A file whose name is already a folder (or the other way round) is skipped.
            if let Some(parent) = path.parent()
                && std::fs::create_dir_all(parent).is_ok()
            {
                let _ = std::fs::write(path, "x");
            }
        }
        let created: Vec<&PathBuf> = paths.iter().filter(|p| p.is_file()).collect();
        prop_assert!(
            settles(&index, |index| created.iter().all(|p| indexed(index, p))),
            "not all created files were indexed: {created:?}"
        );

        let gone: Vec<&PathBuf> = created.iter().step_by(delete_every).copied().collect();
        let kept: Vec<&PathBuf> = created
            .iter()
            .copied()
            .filter(|path| !gone.contains(path))
            .collect();
        for path in &gone {
            std::fs::remove_file(path).map_err(|e| TestCaseError::fail(e.to_string()))?;
        }
        prop_assert!(
            settles(&index, |index| gone.iter().all(|p| !indexed(index, p))),
            "deleted files are still indexed: {gone:?}"
        );
        let index = index.lock().map_err(|e| TestCaseError::fail(e.to_string()))?;
        for path in &kept {
            prop_assert!(indexed(&index, path), "{path:?} vanished");
        }
        prop_assert_eq!(watcher.take_errors(), Vec::<String>::new());
    }
}
