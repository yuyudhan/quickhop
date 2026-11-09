//! The lists commands open in the launcher window on macOS, driven for real (plan slices C1,
//! C2, S1 and F1 in the window, **[ax]**): Clipboard History, Search Snippets and Search
//! Files, each searched as it is typed and acting on the chosen entry.
//!
//! Ignored by default, like `window.rs`: run them with `just ui-test`.

#![cfg(target_os = "macos")]

use std::time::Duration;

use quickhop_uitest::ax::{self, Key};
use quickhop_uitest::{Daemon, Fallible, PATIENCE, TEST_CONFIG};
use serde_json::Value;

fn strings<'a>(snapshot: &'a Value, key: &str) -> Vec<&'a str> {
    snapshot[key]
        .as_array()
        .map(|values| values.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Shows the window, finds the command `id` by typing `text` and opens its list.
fn open_list(daemon: &Daemon, text: &str, id: &str, title: &str) -> Fallible<Value> {
    let output = daemon.quickhop(&["toggle"])?;
    if String::from_utf8(output.stdout)?.trim() != "shown" {
        return Err(format!("toggle failed; daemon output: {}", daemon.log()).into());
    }
    daemon.wait_for_window()?;
    daemon.type_text(text)?;
    daemon.wait_for(&format!("{id} first"), |s| {
        s["query"] == text && strings(s, "results").first() == Some(&id)
    })?;
    daemon.press(Key::Return, false)?;
    daemon.wait_for(&format!("the {title} list"), |s| {
        strings(s, "stack") == ["", title] && s["query"] == ""
    })
}

/// Waits until the private pasteboard holds `text`.
fn copied(text: &str) -> Fallible {
    let deadline = std::time::Instant::now() + PATIENCE;
    while ax::copied().as_deref() != Some(text) {
        if std::time::Instant::now() > deadline {
            return Err(format!("copied: {:?}, expected {text:?}", ax::copied()).into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn clipboard_history_lists_copies_newest_first_and_pastes_the_chosen_one() -> Fallible {
    let _screen = ax::take_over()?;
    let daemon = Daemon::start()?;
    for text in ["alpha one", "beta two", "gamma three"] {
        ax::set_copied(text)?;
        // The daemon looks at the pasteboard every 250 ms.
        std::thread::sleep(Duration::from_millis(600));
    }
    let list = open_list(
        &daemon,
        "clipboard history",
        "views:clipboard",
        "Clipboard History",
    )?;
    assert_eq!(
        strings(&list, "titles"),
        ["gamma three", "beta two", "alpha one"],
        "{list}"
    );
    daemon.type_text("beta")?;
    let found = daemon.wait_for("only beta", |s| strings(s, "titles") == ["beta two"])?;
    assert_eq!(found["query"], "beta");
    // A private pasteboard is never pasted from: the entry is copied and the HUD says why.
    ax::set_copied("something else")?;
    daemon.press(Key::Return, false)?;
    daemon.wait_for_no_window()?;
    copied("beta two")?;
    let hud = daemon.wait_for("the HUD", |s| {
        s["hud"]
            .as_str()
            .is_some_and(|text| text.starts_with("Copied, not pasted"))
    })?;
    assert_eq!(hud["visible"], false);
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn search_snippets_lists_every_snippet_and_copies_the_chosen_one_expanded() -> Fallible {
    const SNIPPETS: &str = "[[snippets]]\nname = \"Signature\"\ntext = \"Best, Ada\"\n\n\
                            [[snippets]]\nname = \"Today\"\n\
                            text = \"Today is {date format=\\\"yyyy-MM-dd\\\"}\"\n";
    let _screen = ax::take_over()?;
    let daemon = Daemon::start_with_files(TEST_CONFIG, &[("snippets.toml", SNIPPETS)])?;
    let list = open_list(
        &daemon,
        "search snippets",
        "views:snippets",
        "Search Snippets",
    )?;
    assert_eq!(strings(&list, "titles"), ["Signature", "Today"], "{list}");
    daemon.type_text("today")?;
    daemon.wait_for("only Today", |s| strings(s, "titles") == ["Today"])?;
    let today = std::process::Command::new("date")
        .arg("+%Y-%m-%d")
        .output()?;
    let today = String::from_utf8(today.stdout)?;
    daemon.press(Key::Return, false)?;
    daemon.wait_for_no_window()?;
    copied(&format!("Today is {}", today.trim()))?;
    Ok(())
}

#[test]
#[ignore = "drives the real window: run with `just ui-test` in a GUI session"]
fn search_files_finds_an_indexed_file_and_copies_its_path() -> Fallible {
    let _screen = ax::take_over()?;
    let files = tempfile::tempdir()?;
    let report = files.path().join("qhfixture-report.txt");
    std::fs::write(&report, "a report")?;
    let root = files
        .path()
        .to_str()
        .ok_or("the temporary folder is not UTF-8")?;
    let daemon = Daemon::start_with(&format!("[files]\nroots = [\"{root}\"]\n"))?;
    let index = daemon.quickhop(&["index"])?;
    if !index.status.success() {
        return Err(format!("index failed: {}", String::from_utf8_lossy(&index.stderr)).into());
    }
    daemon.reload_with(&format!("[files]\nroots = [\"{root}\"]\n"))?;
    let list = open_list(&daemon, "search files", "views:files", "Search Files")?;
    assert_eq!(
        strings(&list, "results"),
        Vec::<&str>::new(),
        "nothing before typing"
    );
    daemon.type_text("qhfixture")?;
    let found = daemon.wait_for("the report", |s| {
        strings(s, "titles") == ["qhfixture-report.txt"]
    })?;
    assert!(
        strings(&found, "results")[0].ends_with("/qhfixture-report.txt"),
        "{found}"
    );
    daemon.press(Key::Char('k'), true)?;
    daemon.wait_for("the action panel", |s| !s["panel"].is_null())?;
    daemon.type_text("copy")?;
    daemon.wait_for("only the copy action", |s| {
        strings(s, "panel") == ["Copy to Clipboard"]
    })?;
    daemon.press(Key::Return, false)?;
    daemon.wait_for_no_window()?;
    let deadline = std::time::Instant::now() + PATIENCE;
    while !ax::copied().is_some_and(|path| path.ends_with("/qhfixture-report.txt")) {
        if std::time::Instant::now() > deadline {
            return Err(format!("copied: {:?}", ax::copied()).into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}
