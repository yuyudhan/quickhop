//! quickhop.app on macOS (plan slices U4 and C2, **[ax]**): the bundle that
//! `scripts/bundle-macos.sh` builds, opened with `open` as Finder opens it, is its own app
//! (the bundle's identifier), runs the daemon in that process with the configured hotkey,
//! and opens the onboarding window while it may not paste; the hotkey shows its launcher.
//!
//! A bundle signed a moment ago was never allowed Accessibility, so the onboarding window
//! comes for real. Ignored by default, like every UI test: run them with `just ui-test` on an
//! unlocked Mac nobody is using.

#![cfg(target_os = "macos")]

use quickhop_core::{Flavour, Hotkey};
use quickhop_uitest::{Fallible, TEST_CONFIG, ax};

/// A hotkey no other app uses, so no press of the test reaches another one.
const HOTKEY: &str = "Ctrl+Alt+Cmd+F13";

fn status(app: &ax::App) -> Fallible<String> {
    Ok(String::from_utf8(
        app.quickhop(&["daemon", "status"])?.stdout,
    )?)
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn the_app_runs_the_daemon_with_its_hotkey_and_asks_for_accessibility() -> Fallible {
    let _screen = ax::take_over()?;
    let app = ax::App::open(&format!("hotkey = \"{HOTKEY}\"\n{TEST_CONFIG}"))?;
    let bundle_id = app.bundle_id()?;
    assert!(
        [Flavour::Release, Flavour::Dev]
            .iter()
            .any(|flavour| flavour.bundle_id() == bundle_id),
        "{bundle_id} is neither flavour's identifier"
    );
    assert_eq!(
        ax::app_name(app.pid).as_deref(),
        Some(bundle_id.as_str()),
        "macOS knows the process by the bundle's identifier"
    );

    // Never allowed: the onboarding window, an ordinary one, instead of the launcher.
    app.wait_for_window(|window| window.layer == 0)?;
    let hotkey = Hotkey::parse(HOTKEY)?;
    let running = status(&app)?;
    assert!(
        running.contains("window: hidden")
            && running.trim_end().ends_with(&format!("hotkey: {hotkey}")),
        "{running}"
    );
    let said = app.quickhop(&["onboarding"])?;
    assert_eq!(
        String::from_utf8(said.stdout)?.trim(),
        "onboarding: Accessibility not allowed"
    );

    // The hotkey shows the app's launcher, a panel above ordinary windows.
    quickhop_platform_macos::input::press_hotkey(&hotkey)?;
    app.wait_for_window(|window| window.layer > 0)?;
    let shown = status(&app)?;
    assert!(shown.contains("window: shown"), "{shown}");
    Ok(())
}
