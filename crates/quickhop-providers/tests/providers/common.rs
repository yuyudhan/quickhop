//! Fixtures shared by the provider integration tests.

use std::path::PathBuf;

use quickhop_core::{Cancel, Provider, Quicklink};
use quickhop_index::Index;
use quickhop_providers::Env;
use quickhop_providers::apps::icons::IconSearch;
use quickhop_providers::apps::{AppsProvider, Sources};
use quickhop_providers::calc::CalcProvider;
use quickhop_providers::emoji::EmojiProvider;
use quickhop_providers::files::FilesProvider;
use quickhop_providers::quicklinks::QuicklinksProvider;
use quickhop_providers::scripts::ScriptsProvider;
use quickhop_providers::snippets::SnippetsProvider;
use quickhop_providers::system::SystemProvider;

/// `tests/fixtures`.
pub(crate) fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// A GNOME session in English with an empty `PATH`, so nothing depends on the machine.
pub(crate) fn linux_env() -> Env {
    Env::new(
        "linux",
        [
            ("HOME", "/nonexistent-home"),
            ("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"),
            ("LANG", "en_US.UTF-8"),
            ("TERMINAL", "fixture-terminal"),
        ],
    )
}

/// Desktop entries from the two fixture directories with the fixture icon theme.
pub(crate) fn desktop_sources() -> Sources {
    let root = fixtures();
    Sources::Desktop {
        dirs: vec![root.join("applications"), root.join("applications-system")],
        icons: IconSearch {
            theme: "Fixture".into(),
            base_dirs: vec![root.join("icons")],
            pixmaps: vec![root.join("pixmaps")],
        },
    }
}

/// Desktop-entry apps (the warnings are checked in `apps.rs`).
pub(crate) fn desktop_apps() -> AppsProvider {
    AppsProvider::load(&desktop_sources(), &linux_env()).0
}

/// Fake `.app` bundles.
pub(crate) fn bundle_apps() -> AppsProvider {
    let sources = Sources::Bundles {
        dirs: vec![fixtures().join("Applications")],
    };
    AppsProvider::load(&sources, &linux_env()).0
}

/// Fake Start Menu shortcuts, without PowerShell.
pub(crate) fn start_menu_apps() -> AppsProvider {
    let sources = Sources::StartMenu {
        dirs: vec![fixtures().join("StartMenu")],
        packaged: false,
    };
    AppsProvider::load(&sources, &linux_env()).0
}

/// The script fixtures this OS can run: `/bin/sh` scripts on Unix, `.cmd`/`.ps1` on Windows.
/// Both sets have the same titles, modes, arguments and output.
pub(crate) fn exec_scripts_dir() -> PathBuf {
    fixtures().join(if cfg!(windows) {
        "scripts-windows"
    } else {
        "scripts"
    })
}

/// Fixture script commands; inline scripts are not run so results match on every OS.
pub(crate) fn scripts() -> ScriptsProvider {
    ScriptsProvider::load(&[fixtures().join("scripts")], None).0
}

/// Two quicklinks: a search template with an alias and a plain link.
pub(crate) fn quicklinks() -> QuicklinksProvider {
    QuicklinksProvider::new(vec![
        Quicklink {
            name: "Google".into(),
            url: "https://www.google.com/search?q={query}".into(),
            alias: Some("g".into()),
        },
        Quicklink {
            name: "GitHub".into(),
            url: "https://github.com".into(),
            alias: Some("gh".into()),
        },
    ])
}

/// The fixture snippet library.
pub(crate) fn snippets() -> SnippetsProvider {
    let (provider, warnings) =
        SnippetsProvider::load(&fixtures().join("snippets").join("snippets.toml"));
    assert_eq!(warnings, Vec::<String>::new());
    provider
}

/// The files provider over an index of the whole fixture folder.
pub(crate) fn files() -> FilesProvider {
    let options = quickhop_index::Options::new(vec![fixtures()]);
    let index = Index::in_memory().ok().and_then(|mut index| {
        let stats = index.rebuild(&options, &Cancel::new()).ok()?;
        assert!(stats.files > 50, "{stats:?}");
        Some(index)
    });
    assert!(index.is_some(), "the fixture index could not be built");
    FilesProvider::new(index, None)
}

/// Every provider over fixtures, in the order `quickhop_providers::builtin` uses.
pub(crate) fn all_providers() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(quicklinks()),
        Box::new(CalcProvider::new()),
        Box::new(desktop_apps()),
        Box::new(scripts()),
        Box::new(snippets()),
        Box::new(SystemProvider::for_os("linux")),
        Box::new(EmojiProvider::new()),
        // No index: snapshot paths would depend on where the repository is checked out.
        Box::new(FilesProvider::new(None, None)),
    ]
}
