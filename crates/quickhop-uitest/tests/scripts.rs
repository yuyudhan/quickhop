//! Script commands in the launcher window on macOS, driven for real (plan slice U6, **[ax]**):
//! arguments typed into fields after the alias, a compact script's toast, a full-output
//! script's page with its ANSI colours drawn, an inline script's subtitle refreshed on its
//! `refreshTime`, and a silent script's HUD over the app in front.
//!
//! Ignored by default, like `window.rs`: run them with `just ui-test`.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::time::{Duration, Instant};

use quickhop_uitest::ax::{self, Key};
use quickhop_uitest::{Daemon, Fallible, PATIENCE, TEST_CONFIG};
use serde_json::Value;

/// The fixture scripts (`tests/fixtures/scripts`) copied into `dir`, so the inline one can read
/// a `status.txt` the test changes.
fn copy_scripts(dir: &Path) -> Fallible {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scripts");
    for entry in std::fs::read_dir(fixtures)? {
        let path = entry?.path();
        if let Some(name) = path.file_name() {
            // Copies the permissions too: the scripts stay executable.
            std::fs::copy(&path, dir.join(name))?;
        }
    }
    std::fs::write(dir.join("status.txt"), "first\n")?;
    Ok(())
}

/// A daemon over the fixture scripts (`search.sh` has the alias `sd`), its window shown and
/// focused, and the folder of the scripts.
fn shown_with_scripts() -> Fallible<(Daemon, tempfile::TempDir)> {
    let scripts = tempfile::tempdir()?;
    copy_scripts(scripts.path())?;
    let dir = scripts
        .path()
        .to_str()
        .ok_or("the temporary folder is not UTF-8")?;
    let config = format!(
        "{TEST_CONFIG}\n[scripts]\ndirs = [\"{dir}\"]\n\n\
         [[commands]]\nid = \"scripts:search.sh\"\nalias = \"sd\"\n"
    );
    let daemon = Daemon::start_with(&config)?;
    let output = daemon.quickhop(&["toggle"])?;
    if String::from_utf8(output.stdout)?.trim() != "shown" {
        return Err(format!("toggle failed; daemon output: {}", daemon.log()).into());
    }
    daemon.wait_for_window()?;
    Ok((daemon, scripts))
}

fn first_result(snapshot: &Value) -> Option<&str> {
    snapshot["results"][0].as_str()
}

/// Types `text` and waits until `id` is the first result.
fn find(daemon: &Daemon, text: &str, id: &str) -> Fallible<Value> {
    daemon.type_text(text)?;
    daemon.wait_for(&format!("{id} first for {text:?}"), |s| {
        s["query"] == text && first_result(s) == Some(id)
    })
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn the_alias_and_a_space_ask_for_arguments_and_the_compact_toast_shows_the_output() -> Fallible {
    let _screen = ax::take_over()?;
    let (daemon, _scripts) = shown_with_scripts()?;
    find(&daemon, "sd", "scripts:search.sh")?;
    daemon.type_text(" rust")?;
    daemon.wait_for("the query field filled", |s| {
        s["query"] == "sd" && s["arguments"][0] == "rust" && s["argument"] == 0
    })?;
    daemon.press(Key::Tab, false)?;
    daemon.type_text("python")?;
    daemon.wait_for("the site field filled", |s| {
        s["arguments"][1] == "python" && s["argument"] == 1
    })?;
    daemon.press(Key::Return, false)?;
    let done = daemon.wait_for("the compact toast", |s| {
        s["toast"] == "searching rust on python"
    })?;
    assert_eq!(done["visible"], true, "a compact script keeps the window");
    assert!(done["arguments"].is_null(), "running closes the fields");
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn return_with_a_required_argument_empty_focuses_it() -> Fallible {
    let _screen = ax::take_over()?;
    let (daemon, _scripts) = shown_with_scripts()?;
    find(&daemon, "search docs", "scripts:search.sh")?;
    daemon.press(Key::Return, false)?;
    daemon.wait_for("the fields open", |s| s["argument"] == 0)?;
    daemon.press(Key::Tab, false)?;
    daemon.wait_for("the second field focused", |s| s["argument"] == 1)?;
    daemon.press(Key::Return, false)?;
    let asked = daemon.wait_for("the empty query field focused again", |s| {
        s["argument"] == 0
    })?;
    assert!(asked["toast"].is_null(), "nothing ran: {asked}");
    daemon.press(Key::Escape, false)?;
    daemon.wait_for("the fields closed", |s| s["arguments"].is_null())?;
    assert_eq!(
        daemon.snapshot()?["visible"],
        true,
        "Esc closes the fields first"
    );
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn a_full_output_script_opens_its_output_and_escape_goes_back() -> Fallible {
    let _screen = ax::take_over()?;
    let (daemon, _scripts) = shown_with_scripts()?;
    find(&daemon, "hello world", "scripts:hello.sh")?;
    daemon.press(Key::Return, false)?;
    let page = daemon.wait_for("the output page", |s| !s["detail"].is_null())?;
    assert_eq!(page["stack"], serde_json::json!(["", "Hello World"]));
    assert_eq!(page["detail"], "Hello\nWorld\n");
    assert_eq!(page["visible"], true);
    daemon.press(Key::Escape, false)?;
    let back = daemon.wait_for("back at the root", |s| s["detail"].is_null())?;
    assert_eq!(back["stack"], serde_json::json!([""]));
    assert_eq!(back["query"], "hello world");
    Ok(())
}

/// Where the output's first lines are drawn, in points from the window's top-left corner:
/// below the 56-point search bar and its border and the page's 12-point padding, the first
/// 64 points of height and 400 of width.
const OUTPUT_LINES: (usize, usize, usize, usize) = (16, 70, 400, 64);
const WINDOW_WIDTH_POINTS: usize = 750;

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn ansi_red_output_is_drawn_red() -> Fallible {
    let _screen = ax::take_over()?;
    let (daemon, _scripts) = shown_with_scripts()?;
    find(&daemon, "colour output", "scripts:colours.sh")?;
    daemon.press(Key::Return, false)?;
    let page = daemon.wait_for("the output page", |s| !s["detail"].is_null())?;
    assert!(
        page["detail"]
            .as_str()
            .is_some_and(|text| text.starts_with("RED TEXT") && !text.contains('\u{1b}')),
        "{page}"
    );
    let window = daemon.wait_for_window()?;
    // A frame or two to draw the page.
    std::thread::sleep(Duration::from_millis(200));
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("window.png");
    ax::capture(&window, &path)?;
    let (pixels, width, step) = ax::pixels(&path)?;
    let width = usize::try_from(width)?;
    let scale = width / WINDOW_WIDTH_POINTS;
    assert!(scale >= 1, "{width} px for {window:?}");
    let (left, top, w, h) = OUTPUT_LINES;
    let row = width * step;
    let mut red = 0;
    for y in top * scale..(top + h) * scale {
        for x in left * scale..(left + w) * scale {
            let pixel = &pixels[y * row + x * step..y * row + x * step + 3];
            // The window's own colours (and anything blurred behind it) stay far below 200 in
            // red; drawn ANSI red is 0xff5555.
            if pixel[0] > 200 && pixel[1] < 130 && pixel[2] < 130 {
                red += 1;
            }
        }
    }
    assert!(red >= 200, "{red} red pixels where the red lines are drawn");
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn an_inline_script_refreshes_its_subtitle_on_its_refresh_time() -> Fallible {
    let _screen = ax::take_over()?;
    let (daemon, scripts) = shown_with_scripts()?;
    let found = find(&daemon, "inline status", "scripts:status.sh")?;
    assert_eq!(found["subtitles"][0], "first", "run once when loaded");
    std::fs::write(scripts.path().join("status.txt"), "second\n")?;
    // Its refreshTime is 10 s: nothing changes before the clock gets there.
    std::thread::sleep(Duration::from_millis(1_500));
    assert_eq!(daemon.snapshot()?["subtitles"][0], "first");
    daemon.advance(10)?;
    let refreshed = daemon.wait_for("the new subtitle", |s| s["subtitles"][0] == "second")?;
    assert_eq!(first_result(&refreshed), Some("scripts:status.sh"));
    assert_eq!(refreshed["query"], "inline status");
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn a_silent_script_hides_the_window_and_shows_a_hud_without_taking_the_focus() -> Fallible {
    let _screen = ax::take_over()?;
    let (daemon, _scripts) = shown_with_scripts()?;
    find(&daemon, "silent toggle", "scripts:silent.sh")?;
    daemon.press(Key::Return, false)?;
    let hud = daemon.wait_for("the HUD", |s| s["hud"] == "toggled")?;
    assert_eq!(hud["visible"], false);
    let deadline = Instant::now() + PATIENCE;
    let shown = loop {
        let windows: Vec<_> = ax::windows_on_screen()
            .into_iter()
            .filter(|window| window.pid == daemon.pid)
            .collect();
        if let [only] = windows.as_slice() {
            break *only;
        }
        if Instant::now() > deadline {
            return Err(format!("daemon windows on screen: {windows:?}").into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        shown.height < 100.0 && shown.width < 500.0,
        "the HUD, not the launcher: {shown:?}"
    );
    assert_ne!(
        ax::frontmost_pid(),
        Some(daemon.pid),
        "the HUD leaves the app in front where it is"
    );
    daemon.wait_for("the HUD gone", |s| s["hud"].is_null())?;
    let deadline = Instant::now() + PATIENCE;
    while ax::windows_on_screen().iter().any(|w| w.pid == daemon.pid) {
        if Instant::now() > deadline {
            return Err("the HUD stays on screen".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}
