//! Scale acceptance for slice F1: a 200,000-file tree is indexed in under 30 s and a query
//! answers in under 20 ms. Ignored by default (it writes 200k files); run it in release on
//! the Linux machine:
//!
//!     cargo test -p quickhop-index --release --test scale -- --ignored --nocapture

use std::path::Path;
use std::time::{Duration, Instant};

use quickhop_core::Cancel;
use quickhop_index::{Index, Options};

const FILES: usize = 200_000;
const PER_DIR: usize = 100;

/// `FILES` files, `PER_DIR` per folder, three folder levels, realistic names.
fn build_tree(root: &Path) -> std::io::Result<()> {
    let kinds = [
        "report", "notes", "invoice", "photo", "main", "config", "readme", "data",
    ];
    let exts = ["md", "txt", "pdf", "jpg", "rs", "toml", "json", "csv"];
    for i in 0..FILES / PER_DIR {
        let dir = root
            .join(format!("project-{:02}", i % 50))
            .join(format!("area-{:02}", (i / 50) % 40))
            .join(format!("batch-{i:04}"));
        std::fs::create_dir_all(&dir)?;
        for j in 0..PER_DIR {
            let n = i * PER_DIR + j;
            let name = format!(
                "{}-{n:06}.{}",
                kinds[n % kinds.len()],
                exts[(n / 7) % exts.len()]
            );
            std::fs::File::create(dir.join(name))?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "writes 200k files; run in release on the Linux machine"]
fn indexes_200k_files_in_30s_and_answers_in_20ms() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let started = Instant::now();
    build_tree(root.path())?;
    println!("tree: {FILES} files written in {:?}", started.elapsed());

    let db = tempfile::tempdir()?;
    let mut index = Index::open(&db.path().join("index.sqlite"))?;
    let stats = index.rebuild(
        &Options::new(vec![root.path().to_path_buf()]),
        &Cancel::new(),
    )?;
    println!("index: {stats:?}");
    assert_eq!(stats.files, FILES);
    assert!(stats.millis < 30_000, "indexing took {} ms", stats.millis);

    // `invoice-123458` exists: file 123458 is the third kind.
    for query in [
        "invoice-123458",
        "report 0420",
        "readme toml",
        "batch-1999 photo",
        "zzzz-none",
    ] {
        let mut best = Duration::MAX;
        let mut found = 0;
        for _ in 0..5 {
            let started = Instant::now();
            found = index.search(query, 20)?.len();
            best = best.min(started.elapsed());
        }
        println!("query {query:?}: {found} hits, best of 5 {best:?}");
        assert!(best < Duration::from_millis(20), "{query:?} took {best:?}");
    }
    let exact = index.search("invoice-123458", 5)?;
    assert_eq!(
        exact.first().map(|hit| hit.entry.name.as_str()),
        Some("invoice-123458.rs")
    );
    Ok(())
}
