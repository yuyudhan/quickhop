//! The launcher window on macOS, driven for real (plan slices U1 and U2, **[ax]**): the
//! daemon under test shows its window when toggled, hides it on `Esc` and toggle, and the
//! window answers typing, arrows, `⌘K` and `Return` as the view model says.
//!
//! Ignored by default: they post real key events, so they need a logged-in GUI session, a
//! terminal with the Accessibility permission and a Mac nobody is using. Run them with
//! `just ui-test`.

#![cfg(target_os = "macos")]

use quickhop_uitest::ax::{self, Key};
use quickhop_uitest::{Daemon, Fallible};
use serde_json::Value;

fn toggle(daemon: &Daemon, expected: &str) -> Fallible {
    let output = daemon.quickhop(&["toggle"])?;
    let stdout = String::from_utf8(output.stdout)?;
    if !output.status.success() || stdout.trim() != expected {
        return Err(format!(
            "toggle printed {stdout:?}, expected {expected:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

/// The daemon with its window shown and focused.
fn shown() -> Fallible<Daemon> {
    let daemon = Daemon::start()?;
    toggle(&daemon, "shown")?;
    daemon.wait_for_window()?;
    Ok(daemon)
}

fn results(snapshot: &Value) -> Vec<&str> {
    snapshot["results"]
        .as_array()
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// A hotkey no other app uses, so no press of the test reaches another one.
const HOTKEY: &str = "Ctrl+Alt+Cmd+F13";

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn the_global_hotkey_shows_the_window_and_hides_it_again() -> Fallible {
    let _screen = ax::take_over()?;
    let config = format!("hotkey = \"{HOTKEY}\"\n{}", quickhop_uitest::TEST_CONFIG);
    let daemon = Daemon::start_with(&config)?;
    assert_eq!(daemon.snapshot()?["visible"], false);
    daemon.press_hotkey(HOTKEY)?;
    daemon.wait_for_window()?;
    assert_eq!(daemon.snapshot()?["visible"], true);
    daemon.press_hotkey(HOTKEY)?;
    daemon.wait_for_no_window()?;
    assert_eq!(daemon.snapshot()?["visible"], false);
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn a_reloaded_hotkey_replaces_the_old_one() -> Fallible {
    const NEW: &str = "Ctrl+Alt+Cmd+F14";
    let _screen = ax::take_over()?;
    let config = |hotkey: &str| format!("hotkey = \"{hotkey}\"\n{}", quickhop_uitest::TEST_CONFIG);
    let daemon = Daemon::start_with(&config(HOTKEY))?;
    daemon.reload_with(&config(NEW))?;
    daemon.press_hotkey(NEW)?;
    daemon.wait_for_window()?;
    daemon.press_hotkey(NEW)?;
    daemon.wait_for_no_window()?;
    // The old combination was released: pressing it shows nothing.
    quickhop_platform_macos::input::press_hotkey(&quickhop_core::Hotkey::parse(HOTKEY)?)?;
    std::thread::sleep(std::time::Duration::from_secs(1));
    assert_eq!(
        daemon.window(),
        None,
        "the old hotkey still shows the window"
    );
    assert_eq!(daemon.snapshot()?["visible"], false);
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn toggle_shows_the_window_in_front_then_hides_it() -> Fallible {
    let _screen = ax::take_over()?;
    let daemon = Daemon::start()?;
    // Created hidden at start-up.
    assert_eq!(daemon.snapshot()?["visible"], false);
    assert_eq!(daemon.window(), None);

    toggle(&daemon, "shown")?;
    let window = daemon.wait_for_window()?;
    assert!(
        window.width >= 600.0 && window.height >= 300.0,
        "{window:?}"
    );
    let snapshot = daemon.snapshot()?;
    assert_eq!(snapshot["visible"], true);
    assert_eq!(snapshot["query"], "");
    assert!(
        !results(&snapshot).is_empty(),
        "the empty root lists items: {snapshot}"
    );

    toggle(&daemon, "hidden")?;
    daemon.wait_for_no_window()?;
    assert_eq!(daemon.snapshot()?["visible"], false);
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn escape_at_the_root_hides_the_window() -> Fallible {
    let _screen = ax::take_over()?;
    let daemon = shown()?;
    daemon.press(Key::Escape, false)?;
    daemon.wait_for_no_window()?;
    assert_eq!(daemon.snapshot()?["visible"], false);
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn typing_searches_and_arrows_move_the_selection() -> Fallible {
    let _screen = ax::take_over()?;
    let daemon = shown()?;
    daemon.type_text("calculatorx")?;
    daemon.wait_for("the typed query", |s| s["query"] == "calculatorx")?;
    daemon.press(Key::Delete, false)?;
    let found = daemon.wait_for("Calculator first", |s| {
        s["query"] == "calculator" && results(s).first() == Some(&"apps:com.apple.calculator")
    })?;
    assert_eq!(found["selected"], 0);

    // Selection wraps at both ends of the list.
    let count = results(&found).len();
    assert!(count >= 2, "more than one result for `calculator`: {found}");
    daemon.press(Key::Down, false)?;
    daemon.wait_for("the second result selected", |s| s["selected"] == 1)?;
    daemon.press(Key::Up, false)?;
    daemon.press(Key::Up, false)?;
    daemon.wait_for("the last result selected", |s| s["selected"] == count - 1)?;
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn option_and_command_backspace_delete_a_word_and_the_query() -> Fallible {
    let _screen = ax::take_over()?;
    let daemon = shown()?;
    daemon.type_text("hello world foo")?;
    daemon.wait_for("the typed query", |s| s["query"] == "hello world foo")?;
    daemon.press_shortcut("Alt+Backspace")?;
    daemon.wait_for("the last word deleted", |s| s["query"] == "hello world ")?;
    daemon.press_shortcut("Cmd+Backspace")?;
    daemon.wait_for("the query deleted", |s| s["query"] == "")?;
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn command_k_opens_the_action_panel_and_escape_closes_it() -> Fallible {
    let _screen = ax::take_over()?;
    let daemon = shown()?;
    daemon.type_text("calculator")?;
    daemon.wait_for("Calculator first", |s| {
        results(s).first() == Some(&"apps:com.apple.calculator")
    })?;
    daemon.press(Key::Char('k'), true)?;
    let open = daemon.wait_for("the action panel", |s| !s["panel"].is_null())?;
    let labels: Vec<&str> = open["panel"]
        .as_array()
        .map(|labels| labels.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert_eq!(
        labels,
        ["Open", "Show in File Manager", "Copy to Clipboard"],
        "{open}"
    );

    // `Esc` closes the panel, not the window; the query comes back.
    daemon.press(Key::Escape, false)?;
    let closed = daemon.wait_for("the panel closed", |s| s["panel"].is_null())?;
    assert_eq!(closed["visible"], true);
    assert_eq!(closed["query"], "calculator");
    assert!(daemon.window().is_some(), "the window stays on screen");
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn return_opens_the_first_app_and_hides_the_window() -> Fallible {
    const CALCULATOR: &str = "com.apple.calculator";
    let _screen = ax::take_over()?;
    if ax::is_running(CALCULATOR) {
        return Err("Calculator is open: the test opens and quits it, so quit it first".into());
    }
    let daemon = shown()?;
    daemon.type_text("calculator")?;
    daemon.wait_for("Calculator first", |s| {
        results(s).first() == Some(&"apps:com.apple.calculator")
    })?;
    daemon.press(Key::Return, false)?;
    daemon.wait_for_no_window()?;
    let opened = eventually(|| ax::is_running(CALCULATOR));
    // Quit it whatever happened, so no window of the test stays on screen.
    ax::quit(CALCULATOR);
    opened.map_err(|()| "Return did not open Calculator")?;
    Ok(())
}

/// Whether `check` holds within [`quickhop_uitest::PATIENCE`].
fn eventually(check: impl Fn() -> bool) -> Result<(), ()> {
    let deadline = std::time::Instant::now() + quickhop_uitest::PATIENCE;
    while !check() {
        if std::time::Instant::now() > deadline {
            return Err(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn return_copies_a_calculation_and_hides_the_window() -> Fallible {
    let _screen = ax::take_over()?;
    ax::set_copied("before")?;
    let daemon = shown()?;
    daemon.type_text("2+2")?;
    daemon.wait_for("the calculator result", |s| {
        results(s).first() == Some(&"calc:result")
    })?;
    daemon.press(Key::Return, false)?;
    daemon.wait_for_no_window()?;
    // The default action of a calculation copies it, to the private pasteboard here.
    eventually(|| ax::copied().as_deref() == Some("4"))
        .map_err(|()| format!("copied: {:?}", ax::copied()))?;
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn the_window_captures_at_its_size_and_is_not_blank() -> Fallible {
    let _screen = ax::take_over()?;
    let daemon = shown()?;
    let window = daemon.wait_for_window()?;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("window.png");
    ax::capture(&window, &path)?;

    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path)?));
    let mut reader = decoder.read_info()?;
    let mut pixels = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let frame = reader.next_frame(&mut pixels)?;
    // Points times the display's scale (2 on a Retina display).
    let scale = f64::from(frame.width) / window.width;
    assert!(
        scale >= 1.0 && scale.fract() == 0.0,
        "{frame:?} for {window:?}"
    );
    assert!(
        (f64::from(frame.height) - window.height * scale).abs() < 1.0,
        "{frame:?} for {window:?}"
    );
    // Without Screen Recording the window server hands back an empty or uniform image.
    let pixels = &pixels[..frame.buffer_size()];
    let step = frame.color_type.samples();
    let first = &pixels[..step];
    assert!(
        pixels.chunks_exact(step).any(|pixel| pixel != first),
        "the capture is one colour: is Screen Recording granted?"
    );
    Ok(())
}

/// Where the first result's icon is drawn, in points from the window's top-left corner: the
/// 56-point search bar and its 1-point border, the list's 8-point padding, then the 24-point
/// icon slot centred in the 44-point row, after the row's 8-point margin and 12-point padding
/// (`quickhop-ui/src/window.rs`, whose window is 750 points wide).
const FIRST_ICON: (usize, usize, usize) = (20, 75, 24);
const WINDOW_WIDTH_POINTS: usize = 750;

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn results_show_their_application_icon() -> Fallible {
    let _screen = ax::take_over()?;
    let daemon = shown()?;
    daemon.type_text("calculator")?;
    daemon.wait_for("Calculator first, with its icon", |s| {
        results(s).first() == Some(&"apps:com.apple.calculator") && s["icons"][0] == true
    })?;
    let window = daemon.wait_for_window()?;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("window.png");
    // The icon arrived; give the window a frame or two to draw it.
    std::thread::sleep(std::time::Duration::from_millis(200));
    ax::capture(&window, &path)?;
    let (pixels, width, step) = ax::pixels(&path)?;
    let width = usize::try_from(width)?;
    // Points times the display's scale (2 on a Retina display).
    let scale = width / WINDOW_WIDTH_POINTS;
    assert!(scale >= 1, "{width} px for {window:?}");
    // The middle half of the icon slot: an application icon has many colours there, an empty
    // slot or a placeholder square few.
    let (left, top, side) = FIRST_ICON;
    let (x0, x1) = ((left + side / 4) * scale, (left + side * 3 / 4) * scale);
    let (y0, y1) = ((top + side / 4) * scale, (top + side * 3 / 4) * scale);
    let row = width * step;
    let mut colours = std::collections::HashSet::new();
    for y in y0..y1 {
        for x in x0..x1 {
            colours.insert(&pixels[y * row + x * step..y * row + (x + 1) * step]);
        }
    }
    assert!(
        colours.len() >= 16,
        "{} colours in the icon's middle ({x0}..{x1}, {y0}..{y1} of {width} px wide)",
        colours.len()
    );
    Ok(())
}
