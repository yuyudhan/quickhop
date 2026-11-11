//! quickhop's settings and presence on macOS, driven for real (CFG-05, **[ax]**): typing
//! `settings` in the launcher and pressing `Return` opens the settings window, and the daemon
//! puts its icon in the menu bar, whose menu opens.
//!
//! Ignored by default, like every UI test: run them with `just ui-test` on an unlocked Mac
//! nobody is using. With `QUICKHOP_UITEST_SHOTS=<dir>` they also save what they saw there
//! (`settings.png`, `menu-bar-icon.png`, `menu-bar-menu.png`).

#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

use quickhop_uitest::ax::{self, Key, OnScreen};
use quickhop_uitest::{Daemon, Fallible, PATIENCE, TEST_CONFIG};

/// A hotkey no other app uses, so no press of the test reaches another one.
const HOTKEY: &str = "Ctrl+Alt+Cmd+F13";

/// The settings window's width in points (`quickhop_ui::settings_window`).
const SETTINGS_WIDTH: f64 = 560.0;

/// The window server layer of menu-bar status items (`kCGStatusWindowLevel`).
const STATUS_LAYER: i32 = 25;

/// The folder `QUICKHOP_UITEST_SHOTS` names, created, if it is set.
fn shots() -> Fallible<Option<PathBuf>> {
    let Some(dir) = std::env::var_os("QUICKHOP_UITEST_SHOTS").map(PathBuf::from) else {
        return Ok(None);
    };
    std::fs::create_dir_all(&dir)?;
    Ok(Some(dir))
}

/// Saves `window` as `name` in the [`shots`] folder, if one is set.
fn save(window: &OnScreen, name: &str) -> Fallible {
    if let Some(dir) = shots()? {
        ax::capture(window, &dir.join(name))?;
    }
    Ok(())
}

/// The first of `daemon`'s windows that `matches`, waiting up to [`PATIENCE`].
fn wait_for(
    daemon: &Daemon,
    what: &str,
    matches: impl Fn(&OnScreen) -> bool,
) -> Fallible<OnScreen> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let windows: Vec<OnScreen> = ax::windows_on_screen()
            .into_iter()
            .filter(|window| window.pid == daemon.pid)
            .collect();
        if let Some(window) = windows.iter().find(|window| matches(window)) {
            return Ok(*window);
        }
        if Instant::now() > deadline {
            return Err(format!("no {what} among the daemon's windows {windows:?}").into());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn typing_settings_and_return_opens_the_settings_window() -> Fallible {
    let _screen = ax::take_over()?;
    let daemon = Daemon::start_with(&format!("hotkey = \"{HOTKEY}\"\n{TEST_CONFIG}"))?;
    daemon.press_hotkey(HOTKEY)?;
    daemon.wait_for_window()?;
    daemon.type_text("settings")?;
    daemon.wait_for("quickhop Settings first", |snapshot| {
        snapshot["query"] == "settings" && snapshot["results"][0] == "quickhop:settings"
    })?;
    daemon.press(Key::Return, false)?;
    let settings = wait_for(&daemon, "settings window", |window| {
        window.layer == 0 && (window.width - SETTINGS_WIDTH).abs() < 1.0
    })?;
    daemon.wait_for("the launcher off screen", |snapshot| {
        snapshot["visible"] == false
    })?;
    assert_eq!(
        ax::focused_window_title(daemon.pid).as_deref(),
        Some("quickhop Settings"),
        "the settings window has the focus"
    );
    save(&settings, "settings.png")?;
    // `⌘W` closes it, as any window.
    daemon.press(Key::Char('w'), true)?;
    let deadline = Instant::now() + PATIENCE;
    while ax::windows_on_screen()
        .iter()
        .any(|window| window.id == settings.id)
    {
        assert!(
            Instant::now() < deadline,
            "⌘W left the settings window open"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn the_menu_bar_icon_shows_and_opens_its_menu() -> Fallible {
    let _screen = ax::take_over()?;
    let daemon = Daemon::start_with(&format!("hotkey = \"{HOTKEY}\"\n{TEST_CONFIG}"))?;
    let icon = wait_for(&daemon, "menu-bar icon", |window| {
        window.layer == STATUS_LAYER
    })?;
    assert!(icon.height <= 40.0, "{icon:?}");
    let status = String::from_utf8(daemon.quickhop(&["daemon", "status"])?.stdout)?;
    assert!(status.contains("menu bar: on"), "{status}");
    save(&icon, "menu-bar-icon.png")?;

    // Clicks this daemon's icon only: System Events picks the process by its pid. The click
    // returns once the menu closes, which the daemon's end does below.
    let script = format!(
        "tell application \"System Events\" to tell (first process whose unix id is {}) \
         to click menu bar item 1 of menu bar 2",
        daemon.pid
    );
    let mut clicked = Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let menu = wait_for(&daemon, "open menu", |window| window.layer > STATUS_LAYER);
    let saved = menu
        .as_ref()
        .map_or(Ok(()), |menu| save(menu, "menu-bar-menu.png"));
    drop(daemon);
    clicked.wait()?;
    let menu = menu?;
    saved?;
    assert!(menu.height > icon.height, "{menu:?}");
    Ok(())
}
