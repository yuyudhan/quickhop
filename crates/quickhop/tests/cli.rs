//! End-to-end tests of the quickhop binary.

use std::error::Error;

use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;

type TestResult = Result<(), Box<dyn Error>>;

/// Where the clipboard commands read and write on macOS during the tests: a private
/// pasteboard, never the user's (`QUICKHOP_PASTEBOARD`; other systems ignore it).
const PASTEBOARD: &str = "dev.quickhop.test.cli";

/// The binary with no user configuration in reach: the XDG and quickhop variables unset,
/// `HOME` pointing nowhere and a private pasteboard, so no test reads or migrates the real
/// user's files or touches their clipboard.
fn quickhop() -> Result<Command, Box<dyn Error>> {
    let mut cmd = Command::cargo_bin("quickhop")?;
    cmd.env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("APPDATA")
        .env_remove("QUICKHOP_CONFIG")
        .env_remove("QUICKHOP_DATA")
        .env("QUICKHOP_PASTEBOARD", PASTEBOARD)
        .env("HOME", "/nonexistent-quickhop-home");
    Ok(cmd)
}

/// Script fixtures this OS runs: `/bin/sh` on Unix, cmd.exe batch files on Windows.
const SCRIPTS_DIR: &str = if cfg!(windows) {
    "scripts-windows"
} else {
    "scripts"
};
const SCRIPT_EXT: &str = if cfg!(windows) { "cmd" } else { "sh" };

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// A config over the fixture apps and scripts and a private data directory, so nothing
/// depends on (or writes to) the machine running the tests.
struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Result<Self, Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let toml = |p: PathBuf| p.to_string_lossy().replace('\\', "/");
        let config = format!(
            "[apps]\ndirs = [\"{}\"]\n[scripts]\ndirs = [\"{}\"]\n\n\
             [[quicklinks]]\nname = \"Google\"\nurl = \"https://www.google.com/search?q={{query}}\"\nalias = \"g\"\n",
            toml(fixtures().join("apps")),
            toml(fixtures().join(SCRIPTS_DIR)),
        );
        std::fs::write(dir.path().join("config.toml"), config)?;
        Ok(Self { dir })
    }

    fn cmd(&self) -> Result<Command, Box<dyn Error>> {
        let mut cmd = quickhop()?;
        cmd.env("QUICKHOP_CONFIG", self.dir.path().join("config.toml"))
            .env("QUICKHOP_DATA", self.dir.path().join("data"))
            .env_remove("XDG_CURRENT_DESKTOP");
        Ok(cmd)
    }

    /// Appends `toml` to the sandbox config.
    fn add_config(&self, toml: &str) -> Result<(), Box<dyn Error>> {
        let path = self.dir.path().join("config.toml");
        let config = std::fs::read_to_string(&path)?;
        std::fs::write(&path, format!("{config}\n{toml}"))?;
        Ok(())
    }

    fn query(&self, text: &str) -> Result<Value, Box<dyn Error>> {
        let out = stdout(self.cmd()?.args(["query", "--json", text]))?;
        Ok(serde_json::from_str(&out)?)
    }
}

fn first(results: &Value) -> &Value {
    &results["results"][0]
}

fn stdout(cmd: &mut Command) -> Result<String, Box<dyn Error>> {
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(format!("quickhop failed: {output:?}").into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[test]
fn version() -> TestResult {
    assert!(stdout(quickhop()?.arg("--version"))?.starts_with("quickhop 0.1.0"));
    Ok(())
}

#[test]
fn config_default_passes_check() -> TestResult {
    let dir = std::env::temp_dir().join(format!("quickhop-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("config.toml");
    std::fs::write(&file, stdout(quickhop()?.args(["config", "default"]))?)?;
    assert!(stdout(quickhop()?.args(["config", "check"]).arg(&file))?.ends_with(": ok\n"));
    std::fs::write(&file, "max_results = 0\n")?;
    let output = quickhop()?.args(["config", "check"]).arg(&file).output()?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("max_results"));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn config_path_honours_override() -> TestResult {
    let path = stdout(
        quickhop()?
            .env("QUICKHOP_CONFIG", "/tmp/q.toml")
            .args(["config", "path"]),
    )?;
    assert_eq!(path.trim(), "/tmp/q.toml");
    Ok(())
}

#[test]
fn doctor_reports_providers() -> TestResult {
    let text = stdout(
        quickhop()?
            .env("QUICKHOP_CONFIG", "/nonexistent/q.toml")
            .env("QUICKHOP_DATA", "/nonexistent/data")
            .arg("doctor"),
    )?;
    assert!(text.contains("absent, using defaults"), "{text}");
    assert!(text.contains("provider calc: enabled"), "{text}");
    assert!(
        text.contains("provider system: enabled (5 items)"),
        "{text}"
    );
    Ok(())
}

#[test]
fn query_finds_the_installed_app_on_every_os() -> TestResult {
    let sandbox = Sandbox::new()?;
    let json = sandbox.query("firefox")?;
    assert_eq!(json["query"], "firefox");
    assert_eq!(first(&json)["title"], "Firefox", "{json:#}");
    assert_eq!(first(&json)["provider"], "apps");
    assert_eq!(first(&json)["tier"], "matched");
    assert_eq!(json["errors"], Value::Array(vec![]), "{json:#}");
    Ok(())
}

#[test]
fn query_computes_and_converts() -> TestResult {
    let sandbox = Sandbox::new()?;
    let math = sandbox.query("2+2*3")?;
    assert_eq!(first(&math)["title"], "8");
    assert_eq!(first(&math)["tier"], "computed");
    assert_eq!(
        first(&math)["actions"][0],
        serde_json::json!({"kind": "copy", "text": "8"})
    );
    let units = sandbox.query("5 km to miles")?;
    assert_eq!(first(&units)["title"], "approx. 3.1068559612 miles");
    Ok(())
}

#[test]
fn query_finds_emoji_and_pins_quicklinks() -> TestResult {
    let sandbox = Sandbox::new()?;
    let smile = sandbox.query("smile")?;
    assert_eq!(first(&smile)["title"], "😄 grinning face with smiling eyes");
    let link = sandbox.query("g rust")?;
    assert_eq!(first(&link)["tier"], "pinned");
    assert_eq!(
        first(&link)["actions"][0]["target"],
        "https://www.google.com/search?q=rust"
    );
    Ok(())
}

#[test]
fn query_text_output_and_limit() -> TestResult {
    let sandbox = Sandbox::new()?;
    let text = stdout(sandbox.cmd()?.args(["query", "--limit", "1", "lock"]))?;
    assert!(text.starts_with("Lock Screen  ("), "{text}");
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.trim_end().ends_with("[system:lock]"), "{text}");
    Ok(())
}

#[test]
fn invalid_config_fails_the_query() -> TestResult {
    let sandbox = Sandbox::new()?;
    std::fs::write(sandbox.dir.path().join("config.toml"), "max_results = 0\n")?;
    let output = sandbox.cmd()?.args(["query", "x"]).output()?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("max_results"));
    Ok(())
}

#[test]
fn power_commands_need_confirmation() -> TestResult {
    let sandbox = Sandbox::new()?;
    let dry = stdout(
        sandbox
            .cmd()?
            .args(["launch", "system:reboot", "--dry-run"]),
    )?;
    let action: Value = serde_json::from_str(&dry)?;
    assert_eq!(action["kind"], "run");
    assert_eq!(action["confirm"], true);
    let refused = sandbox.cmd()?.args(["launch", "system:reboot"]).output()?;
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--yes"));
    Ok(())
}

#[test]
fn launch_rejects_unknown_ids() -> TestResult {
    let sandbox = Sandbox::new()?;
    for id in ["nonsense", "apps:no-such-app"] {
        let output = sandbox.cmd()?.args(["launch", id]).output()?;
        assert!(!output.status.success(), "{id}");
    }
    Ok(())
}

/// The commands that open a list in the launcher window are found like any command, follow
/// the switch of what they list, and say so on the command line instead of running.
#[test]
fn list_commands_are_found_and_open_only_in_the_window() -> TestResult {
    let sandbox = Sandbox::new()?;
    let found = |text: &str| -> Result<Vec<String>, Box<dyn Error>> {
        let json: Value =
            serde_json::from_str(&stdout(sandbox.cmd()?.args(["query", "--json", text]))?)?;
        Ok(ids(&json).into_iter().map(str::to_owned).collect())
    };
    assert_eq!(
        found("clipboard history")?.first().map(String::as_str),
        Some("views:clipboard")
    );
    assert_eq!(
        found("search snippets")?.first().map(String::as_str),
        Some("views:snippets")
    );
    let output = sandbox
        .cmd()?
        .args(["launch", "views:clipboard"])
        .output()?;
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("opens a list in the launcher window"),
        "{stderr}"
    );
    sandbox.add_config("[clipboard]\nenabled = false\n")?;
    assert!(!found("clipboard history")?.contains(&"views:clipboard".to_owned()));
    Ok(())
}

fn ids(results: &Value) -> Vec<&str> {
    results["results"]
        .as_array()
        .map(|r| r.iter().filter_map(|v| v["id"].as_str()).collect())
        .unwrap_or_default()
}

#[test]
fn commands_alias_disable_and_order_items() -> TestResult {
    let sandbox = Sandbox::new()?;
    sandbox.add_config(
        "[[commands]]\nid = \"system:reboot\"\nalias = \"rb\"\n\n\
         [[commands]]\nid = \"emoji:1f62c\"\nalias = \"grim\"\n\n\
         [[commands]]\nid = \"system:lock\"\nenabled = false\n\n\
         [[commands]]\nid = \"system:shutdown\"\norder = 1\n",
    )?;
    for (text, id) in [("rb", "system:reboot"), ("grim", "emoji:1f62c")] {
        let alias = sandbox.query(text)?;
        assert_eq!(first(&alias)["id"], id, "{alias:#}");
        assert_eq!(first(&alias)["tier"], "pinned");
    }
    let lock = sandbox.query("lock")?;
    assert!(!ids(&lock).contains(&"system:lock"), "{lock:#}");
    let launch = sandbox.cmd()?.args(["launch", "system:lock"]).output()?;
    assert!(!launch.status.success());
    let root = sandbox.query("")?;
    assert_eq!(first(&root)["id"], "system:shutdown", "{root:#}");
    Ok(())
}

#[test]
fn unknown_command_ids_are_config_errors() -> TestResult {
    let sandbox = Sandbox::new()?;
    sandbox.add_config("[[commands]]\nid = \"system:no-such-command\"\nalias = \"x\"\n")?;
    let query = sandbox.cmd()?.args(["query", "lock"]).output()?;
    assert!(!query.status.success());
    let stderr = String::from_utf8_lossy(&query.stderr);
    assert!(
        stderr.contains("unknown command id system:no-such-command"),
        "{stderr}"
    );
    let check = sandbox.cmd()?.args(["config", "check"]).output()?;
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stderr).contains("system:no-such-command"));
    Ok(())
}

const SNIPPETS: &str = r#"
[[snippets]]
name = "Today"
text = "{date format=\"yyyy-MM-dd\"}"

[[snippets]]
name = "Signature"
keyword = ";sig"
text = "Best,\n{argument name=\"who\" default=\"Ada\"}"

[[snippets]]
name = "Reply"
text = "Hi {argument name=\"name\"} ({argument options=\"formal, casual\"}),\n{cursor}\n{snippet name=\"signature\"}"
"#;

#[test]
fn snippets_are_found_and_expanded_on_launch() -> TestResult {
    let sandbox = Sandbox::new()?;
    std::fs::write(sandbox.dir.path().join("snippets.toml"), SNIPPETS)?;
    let launch = |args: &[&str]| -> Result<std::process::Output, Box<dyn Error>> {
        Ok(sandbox
            .cmd()?
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .arg("launch")
            .args(args)
            .output()?)
    };
    assert!(ids(&sandbox.query(";sig")?).contains(&"snippets:signature"));
    let today = jiff::Zoned::now().strftime("%Y-%m-%d").to_string();
    let date = launch(&["snippets:today"])?;
    assert_eq!(String::from_utf8(date.stdout)?, format!("{today}\n"));
    let reply = launch(&["snippets:reply", "--arg", "Grace", "--arg", "casual"])?;
    assert_eq!(
        String::from_utf8(reply.stdout)?,
        "Hi Grace (casual),\n\nBest,\nAda\n"
    );
    let missing = launch(&["snippets:reply"])?;
    assert!(!missing.status.success());
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(
        stderr.contains("snippet needs name, argument 2"),
        "{stderr}"
    );
    let wrong = launch(&["snippets:reply", "--arg", "G", "--arg", "rude"])?;
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("formal, casual"));
    Ok(())
}

#[test]
fn index_then_query_finds_files_by_name_and_folder() -> TestResult {
    let sandbox = Sandbox::new()?;
    let tree = tempfile::tempdir()?;
    let report = tree.path().join("work").join("quarterly-report.md");
    std::fs::create_dir_all(report.parent().ok_or("no parent")?)?;
    std::fs::write(&report, "q3")?;
    std::fs::create_dir_all(tree.path().join("work").join("node_modules"))?;
    std::fs::write(tree.path().join("work/node_modules/quarterly-dep.js"), "")?;
    let root = tree.path().to_string_lossy().replace('\\', "/");
    sandbox.add_config(&format!("[files]\nroots = [\"{root}\"]\n"))?;

    let before = sandbox.query("quarterly")?;
    assert!(
        !ids(&before).iter().any(|id| id.starts_with("files:")),
        "{before:#}"
    );
    let built = stdout(sandbox.cmd()?.arg("index"))?;
    assert!(
        built.starts_with("indexed 1 files and 1 folders"),
        "{built}"
    );

    // The index keeps a root as configured (forward slashes here, also on Windows) and joins
    // what it finds below it with the OS separator.
    let indexed = PathBuf::from(&root)
        .join("work")
        .join("quarterly-report.md");
    let id = format!("files:{}", indexed.display());
    for text in ["quarterly", "work report"] {
        let found = sandbox.query(text)?;
        assert_eq!(first(&found)["id"], id.as_str(), "{found:#}");
    }
    let dry = stdout(
        sandbox
            .cmd()?
            .args(["launch", &id, "--query", "quarterly", "--dry-run"]),
    )?;
    let action: Value = serde_json::from_str(&dry)?;
    assert_eq!(action["kind"], "open");
    assert_eq!(action["target"], indexed.display().to_string());
    let doctor = stdout(sandbox.cmd()?.arg("doctor"))?;
    assert!(doctor.contains("(2 entries)"), "{doctor}");
    Ok(())
}

/// Copy path and move to trash on an indexed file; the home trash is followed on Linux and
/// the BSDs (macOS and Windows trash through Finder and the shell, not testable here).
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn file_actions_copy_the_path_and_move_to_the_trash() -> TestResult {
    let sandbox = Sandbox::new()?;
    let tree = tempfile::tempdir()?;
    let report = tree.path().join("work").join("quarterly report.md");
    std::fs::create_dir_all(report.parent().ok_or("no parent")?)?;
    std::fs::write(&report, "q3")?;
    let data_home = tree.path().join("data-home");
    sandbox.add_config(&format!(
        "[files]\nroots = [\"{}\"]\n",
        tree.path().join("work").display()
    ))?;
    stdout(sandbox.cmd()?.arg("index"))?;
    let id = format!("files:{}", report.display());
    let launch = |action: &str, yes: bool| -> Result<std::process::Output, Box<dyn Error>> {
        let mut cmd = sandbox.cmd()?;
        cmd.env("XDG_DATA_HOME", &data_home).args([
            "launch",
            &id,
            "--query",
            "quarterly",
            "--action",
            action,
        ]);
        if yes {
            cmd.arg("--yes");
        }
        Ok(cmd.output()?)
    };

    let copied = launch("copy", false)?;
    assert_eq!(
        String::from_utf8(copied.stdout)?,
        format!("{}\n", report.display())
    );
    let unknown = String::from_utf8(launch("nope", false)?.stderr)?;
    assert!(
        unknown.contains("has no nope action (it has: open, reveal, copy, trash)"),
        "{unknown}"
    );
    let unconfirmed = launch("trash", false)?;
    assert!(!unconfirmed.status.success());
    assert!(report.is_file(), "trashed without --yes");

    let trashed = launch("trash", true)?;
    assert!(trashed.status.success(), "{trashed:?}");
    assert!(!report.exists());
    let trash = data_home.join("Trash");
    let kept = std::fs::read_to_string(trash.join("files").join("quarterly report.md"))?;
    assert_eq!(kept, "q3");
    let record = std::fs::read_to_string(trash.join("info").join("quarterly report.md.trashinfo"))?;
    assert!(
        record.contains("/work/quarterly%20report.md\nDeletionDate=20"),
        "{record}"
    );
    Ok(())
}

#[test]
fn clipboard_add_list_retention_and_clear() -> TestResult {
    let sandbox = Sandbox::new()?;
    sandbox.add_config("[clipboard]\nmax_entries = 3\n")?;
    for text in ["one", "two\nlines", "  ", "three", "one", "four"] {
        sandbox
            .cmd()?
            .args(["clipboard", "add"])
            .write_stdin(text)
            .assert()
            .success();
    }
    // Blank text is skipped, `one` moved to the front, `two` cut by max_entries.
    let listed = stdout(sandbox.cmd()?.args(["clipboard", "list"]))?;
    assert_eq!(listed, "four\none\nthree\n");
    let json: Value = serde_json::from_str(&stdout(sandbox.cmd()?.args([
        "clipboard",
        "list",
        "--json",
        "--limit",
        "2",
    ]))?)?;
    assert_eq!(json[1]["text"], "one");
    assert_eq!(json[1]["copies"], 2);
    assert_eq!(json.as_array().map(Vec::len), Some(2));

    stdout(sandbox.cmd()?.args(["clipboard", "clear"]))?;
    assert_eq!(stdout(sandbox.cmd()?.args(["clipboard", "list"]))?, "");
    Ok(())
}

/// `wl-paste --watch` runs `clipboard add` with `CLIPBOARD_STATE` set: a password manager's
/// copy (`sensitive`) and an emptied clipboard (`nil`, `clear`) are never recorded (CLP-04).
#[test]
fn clipboard_add_skips_what_wl_paste_calls_sensitive() -> TestResult {
    let sandbox = Sandbox::new()?;
    for (state, text) in [
        ("data", "kept"),
        ("sensitive", "hunter2"),
        ("nil", "gone"),
        ("clear", "cleared"),
        ("data", "also kept"),
    ] {
        sandbox
            .cmd()?
            .env("CLIPBOARD_STATE", state)
            .args(["clipboard", "add"])
            .write_stdin(text)
            .assert()
            .success();
    }
    let listed = stdout(sandbox.cmd()?.args(["clipboard", "list"]))?;
    assert_eq!(listed, "also kept\nkept\n");
    Ok(())
}

#[test]
fn paste_refuses_missing_entries_non_text_results_and_no_session() -> TestResult {
    let sandbox = Sandbox::new()?;
    let failure = |args: &[&str]| -> Result<String, Box<dyn Error>> {
        let output = sandbox
            .cmd()?
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .args(args)
            .output()?;
        assert!(!output.status.success(), "{args:?} succeeded");
        Ok(String::from_utf8(output.stderr)?)
    };
    sandbox
        .cmd()?
        .args(["clipboard", "add"])
        .write_stdin("only one")
        .assert()
        .success();
    let missing = failure(&["clipboard", "paste", "3"])?;
    assert!(
        missing.contains("no entry 3: the history holds 1"),
        "{missing}"
    );
    // Checked before anything runs: pasting a power command must not run it.
    let power = failure(&[
        "launch",
        "system:lock",
        "--query",
        "lock",
        "--paste",
        "--yes",
    ])?;
    assert!(
        power.contains("only text results (copy, snippet) can be pasted, not run"),
        "{power}"
    );
    // Without DISPLAY and WAYLAND_DISPLAY a Linux session has no clipboard; a Mac always has
    // its pasteboard, and the private one the tests use is never pasted; Windows copies and
    // leaves Ctrl+V to the user.
    let pasted = sandbox
        .cmd()?
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .args(["launch", "emoji:1f604", "--query", "smile", "--paste"])
        .output()?;
    let stderr = String::from_utf8(pasted.stderr)?;
    if cfg!(target_os = "macos") {
        assert!(pasted.status.success(), "{stderr}");
        assert!(
            stderr.contains("copied, not pasted: a private pasteboard"),
            "{stderr}"
        );
    } else if cfg!(windows) {
        assert!(pasted.status.success(), "{stderr}");
        assert!(
            stderr.contains("copied, not pasted: synthetic paste on Windows"),
            "{stderr}"
        );
    } else {
        assert!(!pasted.status.success());
        assert!(stderr.contains("no graphical session"), "{stderr}");
    }
    Ok(())
}

/// `clipboard watch` on macOS polls the pasteboard: copies are listed newest first, and a
/// password manager's concealed copy never is (C1, CLP-04). A pasteboard of its own keeps the
/// other tests and the user's clipboard out of it.
#[cfg(target_os = "macos")]
#[test]
fn clipboard_watch_records_pasteboard_copies_but_not_secrets() -> TestResult {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::time::Duration;

    use quickhop_platform_macos::pasteboard::Pasteboard;

    const BOARD: &str = "dev.quickhop.test.watch";
    let sandbox = Sandbox::new()?;
    let board = Pasteboard::named(BOARD);
    assert!(board.write_text("copied before the watcher started", false));
    let mut watch = std::process::Command::new(env!("CARGO_BIN_EXE_quickhop"))
        .env("HOME", "/nonexistent-quickhop-home")
        .env("QUICKHOP_CONFIG", sandbox.dir.path().join("config.toml"))
        .env("QUICKHOP_DATA", sandbox.dir.path().join("data"))
        .env("QUICKHOP_PASTEBOARD", BOARD)
        .args(["clipboard", "watch"])
        .stdout(Stdio::piped())
        .spawn()?;
    let mut banner = String::new();
    BufReader::new(watch.stdout.take().ok_or("no stdout")?).read_line(&mut banner)?;
    assert_eq!(
        banner,
        "watching the private macOS clipboard (Ctrl+C stops)\n"
    );
    // The watcher polls every 250 ms; a copy between two polls replaces the previous one.
    let settle = || std::thread::sleep(Duration::from_millis(700));
    settle();
    for (text, concealed) in [
        ("searching rust", false),
        ("hunter2", true),
        ("on python", false),
        ("with quickhop", false),
    ] {
        assert!(board.write_text(text, concealed));
        settle();
    }
    watch.kill()?;
    watch.wait()?;
    let listed = stdout(sandbox.cmd()?.args(["clipboard", "list"]))?;
    assert_eq!(listed, "with quickhop\non python\nsearching rust\n");
    Ok(())
}

/// The daemon over a private runtime directory, with no display to watch.
#[cfg(unix)]
mod daemon {
    use std::io::{BufRead, BufReader};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Stdio};

    use super::{Sandbox, TestResult, stdout};

    /// Stops whatever daemon runs in `runtime` when the test ends, however it ends.
    struct Cleanup<'a>(&'a Sandbox, PathBuf, Option<Child>);

    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            if let Ok(mut cmd) = client(self.0, &self.1) {
                let _ = cmd.args(["daemon", "stop"]).output();
            }
            if let Some(child) = self.2.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn client(
        sandbox: &Sandbox,
        runtime: &Path,
    ) -> Result<assert_cmd::Command, Box<dyn std::error::Error>> {
        let mut cmd = sandbox.cmd()?;
        cmd.env("QUICKHOP_RUNTIME", runtime)
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY");
        Ok(cmd)
    }

    /// Starts `quickhop daemon` and returns it with its pid once it listens.
    fn start(
        sandbox: &Sandbox,
        runtime: &Path,
    ) -> Result<(Child, String), Box<dyn std::error::Error>> {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_quickhop"))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env("HOME", "/nonexistent-quickhop-home")
            .env("QUICKHOP_CONFIG", sandbox.dir.path().join("config.toml"))
            .env("QUICKHOP_DATA", sandbox.dir.path().join("data"))
            .env("QUICKHOP_RUNTIME", runtime)
            .env("QUICKHOP_PASTEBOARD", super::PASTEBOARD)
            .arg("daemon")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut line = String::new();
        BufReader::new(child.stdout.take().ok_or("no stdout")?).read_line(&mut line)?;
        let pid = line
            .strip_prefix("quickhop daemon ")
            .and_then(|rest| rest.split_whitespace().next())
            .ok_or_else(|| format!("unexpected banner {line:?}"))?
            .to_owned();
        assert!(line.contains(" listening on "), "{line}");
        Ok((child, pid))
    }

    #[test]
    fn one_daemon_per_user_serves_status_open_reload_and_stop() -> TestResult {
        let sandbox = Sandbox::new()?;
        let runtime = sandbox.dir.path().join("run");
        let (child, pid) = start(&sandbox, &runtime)?;
        let mut cleanup = Cleanup(&sandbox, runtime.clone(), Some(child));

        let second = client(&sandbox, &runtime)?.arg("daemon").output()?;
        assert!(!second.status.success());
        let error = String::from_utf8(second.stderr)?;
        assert!(
            error.contains(&format!("already running (pid {pid})")),
            "{error}"
        );

        let status = stdout(client(&sandbox, &runtime)?.args(["daemon", "status"]))?;
        assert!(
            status.starts_with(&format!("running (pid {pid})")),
            "{status}"
        );
        let opened =
            stdout(client(&sandbox, &runtime)?.args(["open", "emoji:1f604", "--query", "smile"]))?;
        assert_eq!(opened, "😄\n");

        // A broken config is refused and the daemon keeps the old one.
        sandbox.add_config("max_results = 0\n")?;
        let reload = client(&sandbox, &runtime)?.arg("reload").output()?;
        assert!(!reload.status.success());
        assert!(String::from_utf8(reload.stderr)?.contains("max_results"));
        let still = stdout(client(&sandbox, &runtime)?.args(["daemon", "status"]))?;
        assert!(
            still.starts_with(&format!("running (pid {pid})")),
            "{still}"
        );

        assert_eq!(
            stdout(client(&sandbox, &runtime)?.args(["daemon", "stop"]))?,
            "stopping\n"
        );
        let mut child = cleanup.2.take().ok_or("no child")?;
        // Bounded, so a daemon that outlives `stop` fails the test rather than hanging it.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let ended = loop {
            if let Some(ended) = child.try_wait()? {
                break ended;
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("the daemon still ran 10 s after `daemon stop`".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert!(ended.success());
        assert!(!runtime.join("daemon.sock").exists());
        let after = client(&sandbox, &runtime)?
            .args(["daemon", "status"])
            .output()?;
        assert!(String::from_utf8(after.stderr)?.contains("not running"));
        Ok(())
    }

    #[test]
    fn a_killed_daemon_is_replaced_and_open_starts_one() -> TestResult {
        let sandbox = Sandbox::new()?;
        let runtime = sandbox.dir.path().join("run");
        let (mut child, first) = start(&sandbox, &runtime)?;
        let _cleanup = Cleanup(&sandbox, runtime.clone(), None);
        child.kill()?;
        child.wait()?;
        assert!(
            runtime.join("daemon.sock").exists(),
            "a killed daemon leaves its socket"
        );

        // No daemon answers, so `open` starts one, which replaces the stale socket.
        let opened =
            stdout(client(&sandbox, &runtime)?.args(["open", "emoji:1f604", "--query", "smile"]))?;
        assert_eq!(opened, "😄\n");
        let status = stdout(client(&sandbox, &runtime)?.args(["daemon", "status"]))?;
        assert!(status.starts_with("running (pid "), "{status}");
        assert!(
            !status.starts_with(&format!("running (pid {first})")),
            "{status}"
        );
        Ok(())
    }
}

#[test]
fn launching_records_frecency() -> TestResult {
    let sandbox = Sandbox::new()?;
    let id = "emoji:1f62c";
    let before = sandbox.query("gr")?;
    assert_ne!(first(&before)["id"], id, "{before:#}");
    assert_eq!(
        stdout(sandbox.cmd()?.args(["launch", id, "--query", "gr"]))?,
        "😬\n"
    );
    let after = sandbox.query("gr")?;
    assert_eq!(first(&after)["id"], id, "{after:#}");
    Ok(())
}

#[test]
fn launch_runs_script_commands() -> TestResult {
    let sandbox = Sandbox::new()?;
    let hello = stdout(sandbox.cmd()?.args([
        "launch",
        &format!("scripts:hello.{SCRIPT_EXT}"),
        "--arg",
        "quickhop",
    ]))?;
    assert_eq!(hello, "hello quickhop\n");
    let refused = sandbox
        .cmd()?
        .args(["launch", &format!("scripts:danger.{SCRIPT_EXT}")])
        .output()?;
    assert!(!refused.status.success());
    assert_eq!(
        stdout(
            sandbox
                .cmd()?
                .args(["launch", &format!("scripts:danger.{SCRIPT_EXT}"), "--yes"])
        )?,
        "boom\n"
    );
    Ok(())
}

#[test]
fn run_finds_scripts_by_path_stem_and_title() -> TestResult {
    let sandbox = Sandbox::new()?;
    let file = fixtures()
        .join(SCRIPTS_DIR)
        .join(format!("hello.{SCRIPT_EXT}"));
    let by_path = stdout(sandbox.cmd()?.arg("run").arg(&file).arg("path"))?;
    assert_eq!(by_path, "hello path\n");
    assert_eq!(
        stdout(sandbox.cmd()?.args(["run", "hello"]))?,
        "hello world\n"
    );
    assert_eq!(
        stdout(sandbox.cmd()?.args(["run", "hello cli", "--", "-x"]))?,
        "hello -x\n"
    );
    let missing = sandbox.cmd()?.args(["run", "no-such-script"]).output()?;
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no script command"));
    Ok(())
}

#[test]
fn run_needs_confirmation_when_the_script_asks() -> TestResult {
    let sandbox = Sandbox::new()?;
    let refused = sandbox.cmd()?.args(["run", "danger"]).output()?;
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--yes"));
    assert_eq!(
        stdout(sandbox.cmd()?.args(["run", "danger", "--yes"]))?,
        "boom\n"
    );
    Ok(())
}

#[test]
fn run_loads_the_default_scripts_dir_next_to_the_config() -> TestResult {
    let sandbox = Sandbox::new()?;
    let scripts = sandbox.dir.path().join("scripts");
    std::fs::create_dir_all(&scripts)?;
    let name = format!("hello.{SCRIPT_EXT}");
    let mine = format!("mine.{SCRIPT_EXT}");
    std::fs::copy(fixtures().join(SCRIPTS_DIR).join(&name), scripts.join(mine))?;
    // Only the default directory: drop `[scripts] dirs` from the sandbox config.
    std::fs::write(sandbox.dir.path().join("config.toml"), "")?;
    assert_eq!(
        stdout(sandbox.cmd()?.args(["run", "mine"]))?,
        "hello world\n"
    );
    let doctor = stdout(sandbox.cmd()?.arg("doctor"))?;
    assert!(
        doctor.contains(&format!("scripts: {}", scripts.display())),
        "{doctor}"
    );
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn macos_moves_the_legacy_config_to_xdg() -> TestResult {
    let home = tempfile::tempdir()?;
    let legacy = home.path().join("Library/Application Support/quickhop");
    std::fs::create_dir_all(&legacy)?;
    std::fs::write(legacy.join("config.toml"), "max_results = 4\n")?;
    std::fs::write(legacy.join("frecency.sqlite"), "")?;
    let output = quickhop()?
        .env("HOME", home.path())
        .env_remove("XDG_DATA_HOME")
        .args(["config", "path"])
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let new_config = home.path().join(".config/quickhop/config.toml");
    assert_eq!(
        String::from_utf8(output.stdout)?.trim(),
        new_config.display().to_string()
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("moved"));
    assert_eq!(std::fs::read_to_string(&new_config)?, "max_results = 4\n");
    assert!(
        home.path()
            .join(".local/share/quickhop/frecency.sqlite")
            .is_file()
    );
    assert!(!legacy.join("config.toml").exists());
    Ok(())
}
