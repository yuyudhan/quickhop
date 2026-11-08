//! Application scanners over the fixture directories: desktop entries with the icon
//! theme, `.app` bundles (XML and binary plists) and Start Menu shortcuts.

use crate::common;
use std::path::{Path, PathBuf};

use quickhop_core::{Action, Item};
use quickhop_providers::apps::icons::{IconResolver, IconSearch};
use quickhop_providers::apps::{AppsProvider, bundles};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn find<'a>(items: &'a [Item], title: &str) -> Result<&'a Item, String> {
    items
        .iter()
        .find(|i| i.title == title)
        .ok_or_else(|| format!("{title} not found"))
}

fn titles(items: &[Item]) -> Vec<&str> {
    items.iter().map(|i| i.title.as_str()).collect()
}

fn icon_of(item: &Item) -> PathBuf {
    PathBuf::from(item.icon.clone().unwrap_or_default())
}

#[test]
fn desktop_entries_are_filtered_and_sorted() {
    let apps = common::desktop_apps();
    assert_eq!(
        titles(apps.all()),
        vec![
            "Calculator",
            "Files",
            "Firefox Web Browser",
            "GNU Image Manipulation Program",
            "Htop",
            "Nested Tool",
            "Terminal",
            "Visual Studio Code",
        ]
    );
}

#[test]
fn earlier_directories_shadow_later_ones() -> TestResult {
    let apps = common::desktop_apps();
    let firefox = find(apps.all(), "Firefox Web Browser")?;
    assert_eq!(firefox.id.key, "firefox.desktop");
    assert!(apps.all().iter().all(|i| i.title != "Old Firefox"));
    Ok(())
}

#[test]
fn desktop_ids_join_subdirectories_with_dashes() -> TestResult {
    let apps = common::desktop_apps();
    assert_eq!(
        find(apps.all(), "Nested Tool")?.id.key,
        "vendor-sub-nested.desktop"
    );
    Ok(())
}

#[test]
fn desktop_items_carry_keywords_subtitles_and_commands() -> TestResult {
    let apps = common::desktop_apps();
    let firefox = find(apps.all(), "Firefox Web Browser")?;
    assert_eq!(firefox.subtitle.as_deref(), Some("Web Browser"));
    assert!(firefox.keywords.iter().any(|k| k == "Browser"));
    assert!(firefox.keywords.iter().any(|k| k == "firefox"));
    assert_eq!(
        firefox.default_action(),
        Some(&Action::Run {
            program: "firefox".into(),
            args: vec![],
            confirm: false
        })
    );
    let files = find(apps.all(), "Files")?;
    assert_eq!(files.subtitle.as_deref(), Some("Access and organize files"));
    let terminal = find(apps.all(), "Terminal")?;
    assert_eq!(
        terminal.default_action(),
        Some(&Action::Run {
            program: "gnome-terminal".into(),
            args: vec![
                "--name=Terminal".into(),
                "--icon".into(),
                "utilities-terminal".into()
            ],
            confirm: false
        })
    );
    let htop = find(apps.all(), "Htop")?;
    assert_eq!(
        htop.default_action(),
        Some(&Action::Run {
            program: "fixture-terminal".into(),
            args: vec!["-e".into(), "htop".into()],
            confirm: false
        })
    );
    Ok(())
}

#[test]
fn broken_files_become_warnings() {
    let (_, warnings) = AppsProvider::load(&common::desktop_sources(), &common::linux_env());
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("broken.desktop") && w.contains("Desktop Entry"))
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("bad-exec.desktop") && w.contains("quote"))
    );
}

#[test]
fn localised_names_follow_lang() -> TestResult {
    let env = quickhop_providers::Env::new(
        "linux",
        [("LANG", "de_DE.UTF-8"), ("XDG_CURRENT_DESKTOP", "GNOME")],
    );
    let (apps, _) = AppsProvider::load(&common::desktop_sources(), &env);
    let firefox = find(apps.all(), "Firefox-Webbrowser")?;
    assert_eq!(firefox.subtitle.as_deref(), Some("Webbrowser"));
    Ok(())
}

#[test]
fn icons_resolve_through_the_theme_chain() -> TestResult {
    let apps = common::desktop_apps();
    let icons = common::fixtures().join("icons");
    // Exact size in the preferred theme beats hicolor.
    assert_eq!(
        icon_of(find(apps.all(), "Firefox Web Browser")?),
        icons.join("Fixture").join("48x48/apps/firefox.png")
    );
    // Scalable dir in the inherited theme; the bogus `.png` suffix is ignored.
    assert_eq!(
        icon_of(find(apps.all(), "GNU Image Manipulation Program")?),
        icons.join("hicolor").join("scalable/apps/gimp.svg")
    );
    // No exact size anywhere: the closest size wins.
    assert_eq!(
        icon_of(find(apps.all(), "Visual Studio Code")?),
        icons.join("hicolor").join("16x16/apps/vscode.png")
    );
    // Unthemed fallback.
    assert_eq!(
        icon_of(find(apps.all(), "Htop")?),
        common::fixtures().join("pixmaps").join("htop.xpm")
    );
    // Not found: the symbolic name stays.
    assert_eq!(
        find(apps.all(), "Files")?.icon.as_deref(),
        Some("org.gnome.Nautilus")
    );
    Ok(())
}

#[test]
fn icon_chain_ends_with_hicolor_and_skips_missing_themes() {
    let base = common::fixtures().join("icons");
    let resolver = IconResolver::new(&IconSearch {
        theme: "Fixture".into(),
        base_dirs: vec![PathBuf::from("/nonexistent"), base.clone()],
        pixmaps: vec![],
    });
    assert_eq!(resolver.chain(), vec!["Fixture", "hicolor"]);
    let mut missing = IconResolver::new(&IconSearch {
        theme: "NoSuchTheme".into(),
        base_dirs: vec![base],
        pixmaps: vec![],
    });
    assert_eq!(missing.chain(), vec!["hicolor"]);
    assert_eq!(missing.lookup("no-such-icon"), None);
    assert_eq!(missing.lookup("/nonexistent/icon.png"), None);
}

#[test]
fn bundles_use_info_plist() -> TestResult {
    let apps = common::bundle_apps();
    assert_eq!(
        titles(apps.all()),
        vec!["Code", "No Plist", "Safari", "Terminal"]
    );
    let safari = find(apps.all(), "Safari")?;
    assert_eq!(safari.id.key, "com.apple.Safari");
    let root = common::fixtures().join("Applications");
    assert_eq!(
        safari.default_action(),
        Some(&Action::Open {
            target: root.join("Safari.app").to_string_lossy().into_owned()
        })
    );
    assert_eq!(
        icon_of(safari),
        root.join("Safari.app/Contents/Resources/compass.icns")
    );
    let code = find(apps.all(), "Code")?;
    assert!(code.keywords.iter().any(|k| k == "Visual Studio Code"));
    assert!(code.keywords.iter().any(|k| k == "Electron"));
    let terminal = find(apps.all(), "Terminal")?;
    assert_eq!(terminal.id.key, "com.apple.Terminal");
    Ok(())
}

#[test]
fn binary_plists_are_read() -> TestResult {
    let dir = tempfile::tempdir()?;
    let app = dir.path().join("Binary.app");
    std::fs::create_dir_all(app.join("Contents"))?;
    let mut dict = plist::Dictionary::new();
    dict.insert(
        "CFBundleName".into(),
        plist::Value::String("Binary Bundle".into()),
    );
    dict.insert(
        "CFBundleIdentifier".into(),
        plist::Value::String("com.example.binary".into()),
    );
    plist::Value::Dictionary(dict).to_file_binary(app.join("Contents/Info.plist"))?;
    let bundle = bundles::read_bundle(&app);
    assert_eq!(bundle.name, "Binary Bundle");
    assert_eq!(bundle.identifier.as_deref(), Some("com.example.binary"));
    Ok(())
}

#[test]
fn background_agents_are_skipped() {
    let agent = bundles::read_bundle(&common::fixtures().join("Applications/Menu Agent.app"));
    assert!(agent.background);
}

#[test]
fn start_menu_shortcuts_skip_uninstallers() -> TestResult {
    let apps = common::start_menu_apps();
    assert_eq!(titles(apps.all()), vec!["Firefox", "Notepad", "Tool"]);
    let notepad = find(apps.all(), "Notepad")?;
    assert_eq!(notepad.subtitle.as_deref(), Some("Accessories"));
    let Some(Action::Open { target }) = notepad.default_action() else {
        return Err("Notepad opens its shortcut".into());
    };
    assert!(Path::new(target).ends_with(Path::new("Accessories").join("Notepad.lnk")));
    Ok(())
}

#[test]
fn missing_configured_directories_do_not_fail() {
    let sources = quickhop_providers::apps::Sources::Bundles {
        dirs: vec![PathBuf::from("/nonexistent/quickhop-apps")],
    };
    let (apps, warnings) = AppsProvider::load(&sources, &common::linux_env());
    assert_eq!(apps.all().len(), 0);
    assert_eq!(warnings.len(), 0, "{warnings:?}");
}
