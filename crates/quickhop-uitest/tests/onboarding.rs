//! The onboarding window on macOS, driven for real (plan slice C2, PLT-02, **[ax]**): it says
//! whether quickhop may paste, its button opens the Accessibility pane of System Settings
//! (Device Control and Data Access on macOS 27), it asks again when quickhop comes back to the
//! front and, after the button, every moment until allowed, coming back to the front then, and
//! `Return` or `Esc` close it once nothing is missing.
//!
//! The terminal running the tests holds the Accessibility permission, and the daemon it
//! starts shares it, so the tests set what the window hears through the test socket
//! (`permission denied|granted`). Ignored by default, like every UI test: run them with
//! `just ui-test` on an unlocked Mac nobody is using.

#![cfg(target_os = "macos")]

use std::time::Duration;

use quickhop_uitest::ax::{self, Key};
use quickhop_uitest::{Daemon, Fallible, PATIENCE, TEST_CONFIG};
use serde_json::Value;

/// A hotkey no other app uses, so no press of the test reaches another one.
const HOTKEY: &str = "Ctrl+Alt+Cmd+F13";

/// The daemon with its onboarding window open and focused, hearing `permission`.
fn onboarding(permission: &str, expected: &str) -> Fallible<Daemon> {
    let daemon = Daemon::start_with(&format!("hotkey = \"{HOTKEY}\"\n{TEST_CONFIG}"))?;
    daemon.set_permission(permission)?;
    let output = daemon.quickhop(&["onboarding"])?;
    let said = String::from_utf8(output.stdout)?;
    if !output.status.success() || said.trim() != expected {
        return Err(format!(
            "onboarding printed {said:?}, expected {expected:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let window = daemon.wait_for_window()?;
    assert_eq!(
        window.layer, 0,
        "the onboarding window is an ordinary window"
    );
    Ok(daemon)
}

fn granted(snapshot: &Value) -> Option<bool> {
    snapshot["onboarding"]["granted"].as_bool()
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn the_window_asks_again_when_quickhop_comes_back_to_the_front() -> Fallible {
    let _screen = ax::take_over()?;
    let daemon = onboarding("denied", "onboarding: Accessibility not allowed")?;
    let shown = daemon.wait_for("the onboarding window", |s| granted(s) == Some(false))?;
    assert_eq!(
        shown["visible"], false,
        "the launcher stays hidden: {shown}"
    );
    assert_eq!(shown["onboarding"]["asked"], false, "{shown}");

    // Granted meanwhile; nothing asks while quickhop stays in front.
    daemon.set_permission("granted")?;
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(granted(&daemon.snapshot()?), Some(false));

    // Another app comes to the front, then the hotkey brings quickhop back.
    let _target = ax::PasteTarget::open()?;
    daemon.press_hotkey(HOTKEY)?;
    daemon.wait_for("the launcher", |s| s["visible"] == true)?;
    daemon.wait_for("the onboarding window to ask again", |s| {
        granted(s) == Some(true)
    })?;
    Ok(())
}

/// The titles System Settings gives the pane with the Accessibility list: before macOS 27, and
/// since.
const PANE_TITLES: [&str; 2] = ["Accessibility", "Device Control and Data Access"];

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn the_button_opens_the_accessibility_pane_of_system_settings() -> Fallible {
    let _screen = ax::take_over()?;
    let _settings = ax::SystemSettings::closed()?;
    let daemon = onboarding("denied", "onboarding: Accessibility not allowed")?;
    daemon.press(Key::Return, false)?;
    let deadline = std::time::Instant::now() + PATIENCE * 2;
    loop {
        let front = ax::frontmost_pid();
        let app = front.and_then(ax::app_name);
        let title = front.and_then(ax::focused_window_title);
        if app.as_deref() == Some(ax::SYSTEM_SETTINGS)
            && title
                .as_deref()
                .is_some_and(|title| PANE_TITLES.contains(&title))
        {
            break;
        }
        if std::time::Instant::now() > deadline {
            return Err(format!("in front: {app:?}, its window: {title:?}").into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let asked = daemon.snapshot()?;
    assert_eq!(asked["onboarding"]["asked"], true, "{asked}");
    assert_eq!(granted(&asked), Some(false), "{asked}");

    // Turned on while System Settings stays in front: the window sees it and comes back.
    daemon.set_permission("granted")?;
    daemon.wait_for("the onboarding window to see the grant", |s| {
        granted(s) == Some(true)
    })?;
    let deadline = std::time::Instant::now() + PATIENCE;
    while ax::frontmost_pid() != Some(daemon.pid) {
        if std::time::Instant::now() > deadline {
            return Err(format!(
                "quickhop did not come to the front; {:?} is",
                ax::frontmost_pid().and_then(ax::app_name)
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn done_and_escape_close_the_window() -> Fallible {
    let _screen = ax::take_over()?;
    let daemon = onboarding("granted", "onboarding: Accessibility allowed")?;
    // Allowed: the button is Done.
    daemon.press(Key::Return, false)?;
    daemon.wait_for_no_window()?;
    daemon.wait_for("no onboarding window", |s| s["onboarding"].is_null())?;

    let reopened = daemon.quickhop(&["onboarding"])?;
    assert!(reopened.status.success(), "{reopened:?}");
    daemon.wait_for_window()?;
    daemon.press(Key::Escape, false)?;
    daemon.wait_for_no_window()?;
    daemon.wait_for("no onboarding window", |s| s["onboarding"].is_null())?;
    Ok(())
}
