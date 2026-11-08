//! Fixtures shared by the provider integration tests.

use std::path::PathBuf;

use quickhop_core::{Provider, Quicklink};
use quickhop_providers::Env;
use quickhop_providers::apps::icons::IconSearch;
use quickhop_providers::apps::{AppsProvider, Sources};
use quickhop_providers::calc::CalcProvider;
use quickhop_providers::emoji::EmojiProvider;
use quickhop_providers::quicklinks::QuicklinksProvider;
use quickhop_providers::scripts::ScriptsProvider;
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

/// Every provider over fixtures, in the order `quickhop_providers::builtin` uses.
pub(crate) fn all_providers() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(quicklinks()),
        Box::new(CalcProvider::new()),
        Box::new(desktop_apps()),
        Box::new(scripts()),
        Box::new(SystemProvider::for_os("linux")),
        Box::new(EmojiProvider::new()),
    ]
}
