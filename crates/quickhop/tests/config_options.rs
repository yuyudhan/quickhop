//! Every option of `config.toml` and `snippets.toml` (docs/configuration.md), driven through
//! the real loader: the `quickhop` binary reads a private file (`QUICKHOP_CONFIG`) over a
//! private home, data and runtime directory, and each test checks what the option changes.
//!
//! Per option: a wrong type or out-of-range value fails with the key in the message, the
//! value changes the code path it controls, and the daemon applies it on `quickhop reload`
//! where it can. Window-only effects are in the `quickhop-uitest` crate.

use std::error::Error;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The private pasteboard the clipboard commands use on macOS; other systems ignore it.
const PASTEBOARD: &str = "dev.quickhop.test.config-options";

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

/// `path` as a TOML string body: forward slashes, also on Windows.
fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// `[apps] dirs` over the fixture apps, so no test depends on what the machine has installed.
fn fixture_apps() -> String {
    format!(
        "[apps]\ndirs = [\"{}\"]\n",
        toml_path(&fixtures().join("apps"))
    )
}

/// A private configuration, home, data and runtime directory.
struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    /// A sandbox whose `config.toml` is exactly `config`.
    fn new(config: &str) -> TestResult<Self> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("home"))?;
        let sandbox = Self { dir };
        sandbox.set_config(config)?;
        Ok(sandbox)
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn home(&self) -> PathBuf {
        self.path("home")
    }

    fn set_config(&self, config: &str) -> TestResult {
        std::fs::write(self.path("config.toml"), config)?;
        Ok(())
    }

    fn set_snippets(&self, snippets: &str) -> TestResult {
        std::fs::write(self.path("snippets.toml"), snippets)?;
        Ok(())
    }

    /// `quickhop` with nothing of the user's in reach.
    fn std_cmd(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_quickhop"));
        cmd.env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            .env_remove("XDG_CURRENT_DESKTOP")
            .env_remove("APPDATA")
            .env_remove("LOCALAPPDATA")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("QUICKHOP_WL_WATCH")
            .env_remove("CLIPBOARD_STATE")
            .env("HOME", self.home())
            .env("USERPROFILE", self.home())
            .env("QUICKHOP_CONFIG", self.path("config.toml"))
            .env("QUICKHOP_DATA", self.path("data"))
            .env("QUICKHOP_RUNTIME", self.path("run"))
            .env("QUICKHOP_PASTEBOARD", PASTEBOARD);
        cmd
    }

    fn cmd(&self) -> Command {
        Command::from_std(self.std_cmd())
    }

    /// Standard output of a successful `quickhop <args>`.
    fn ok(&self, args: &[&str]) -> TestResult<String> {
        let output = self.cmd().args(args).output()?;
        if !output.status.success() {
            return Err(format!("quickhop {args:?} failed: {output:?}").into());
        }
        Ok(String::from_utf8(output.stdout)?)
    }

    /// Standard error of a failing `quickhop <args>`.
    fn fails(&self, args: &[&str]) -> TestResult<String> {
        let output = self.cmd().args(args).output()?;
        if output.status.success() {
            return Err(format!("quickhop {args:?} succeeded: {output:?}").into());
        }
        Ok(String::from_utf8(output.stderr)?)
    }

    /// `quickhop config check` refuses the file; its message.
    fn refused(&self, config: &str) -> TestResult<String> {
        self.set_config(config)?;
        self.fails(&["config", "check"])
    }

    /// The ids `quickhop query --json text` returns, best first.
    fn ids(&self, text: &str) -> TestResult<Vec<String>> {
        let json: Value = serde_json::from_str(&self.ok(&["query", "--json", text])?)?;
        Ok(json["results"]
            .as_array()
            .ok_or("no results array")?
            .iter()
            .filter_map(|result| result["id"].as_str().map(str::to_owned))
            .collect())
    }
}

/// A wrong type is refused naming the line and showing the key as written.
#[test]
fn every_key_refuses_a_wrong_type_with_its_line() -> TestResult {
    let sandbox = Sandbox::new("")?;
    let quicklink = "[[quicklinks]]\nname = \"N\"\nurl = \"u\"\n";
    let command = "[[commands]]\nid = \"system:lock\"\n";
    let cases: Vec<(String, &str)> = vec![
        ("hotkey = 5\n".into(), "hotkey = 5"),
        ("max_results = \"8\"\n".into(), "max_results = \"8\""),
        ("[providers]\napps = 1\n".into(), "apps = 1"),
        ("[providers]\ncalc = \"no\"\n".into(), "calc = \"no\""),
        ("[providers]\nemoji = 0\n".into(), "emoji = 0"),
        ("[providers]\nsystem = []\n".into(), "system = []"),
        ("[providers]\nquicklinks = 1\n".into(), "quicklinks = 1"),
        ("[providers]\nscripts = 1\n".into(), "scripts = 1"),
        ("[providers]\nsnippets = 1\n".into(), "snippets = 1"),
        ("[providers]\nfiles = 1\n".into(), "files = 1"),
        ("[apps]\ndirs = \"/x\"\n".into(), "dirs = \"/x\""),
        ("[apps]\nicon_theme = 1\n".into(), "icon_theme = 1"),
        ("[scripts]\ndirs = 1\n".into(), "dirs = 1"),
        ("[files]\nroots = \"/x\"\n".into(), "roots = \"/x\""),
        ("[files]\nhidden = \"yes\"\n".into(), "hidden = \"yes\""),
        ("[files]\nexclude = \"x\"\n".into(), "exclude = \"x\""),
        ("[clipboard]\nenabled = 1\n".into(), "enabled = 1"),
        (
            "[clipboard]\nmax_entries = \"9\"\n".into(),
            "max_entries = \"9\"",
        ),
        (
            "[clipboard]\nmax_age_days = 1.5\n".into(),
            "max_age_days = 1.5",
        ),
        ("[[quicklinks]]\nname = 1\nurl = \"u\"\n".into(), "name = 1"),
        ("[[quicklinks]]\nname = \"N\"\nurl = 1\n".into(), "url = 1"),
        (format!("{quicklink}alias = 1\n"), "alias = 1"),
        ("[[commands]]\nid = 1\n".into(), "id = 1"),
        (format!("{command}alias = 1\n"), "alias = 1"),
        (format!("{command}enabled = \"no\"\n"), "enabled = \"no\""),
        (format!("{command}hotkey = 1\n"), "hotkey = 1"),
        (format!("{command}order = \"1\"\n"), "order = \"1\""),
    ];
    for (config, line) in &cases {
        let error = sandbox.refused(config)?;
        assert!(
            error.contains("TOML parse error at line"),
            "{config}: {error}"
        );
        assert!(error.contains(line), "{config}: {error}");
        assert!(error.contains("invalid "), "{config}: {error}");
    }
    Ok(())
}

/// Values of the right type but outside what the key allows, and missing required keys.
#[test]
fn out_of_range_values_are_refused_naming_the_key() -> TestResult {
    let sandbox = Sandbox::new("")?;
    let launcher = default_hotkey(&sandbox.ok(&["config", "default"])?)?;
    let cases = [
        (
            "max_results = 0\n",
            "max_results must be between 1 and 100, got 0",
        ),
        (
            "max_results = 101\n",
            "max_results must be between 1 and 100, got 101",
        ),
        ("hotkey = \"\"\n", "hotkey: "),
        ("hotkey = \"Alt+Spacebar\"\n", "hotkey: "),
        ("hotkey = \"Space\"\n", "hotkey: "),
        (
            "[clipboard]\nmax_entries = 0\n",
            "clipboard.max_entries must be between 1 and 100000, got 0",
        ),
        (
            "[clipboard]\nmax_entries = 100001\n",
            "clipboard.max_entries must be between 1 and 100000, got 100001",
        ),
        ("[clipboard]\nmax_age_days = -1\n", "max_age_days = -1"),
        (
            "[[quicklinks]]\nname = \" \"\nurl = \"u\"\n",
            "quicklinks need a name and a url",
        ),
        (
            "[[quicklinks]]\nname = \"N\"\nurl = \"\"\n",
            "quicklinks need a name and a url",
        ),
        (
            "[[quicklinks]]\nname = \"N\"\nurl = \"u\"\nalias = \"two words\"\n",
            "quicklink N: alias must be one word",
        ),
        ("[[quicklinks]]\nurl = \"u\"\n", "missing field `name`"),
        ("[[quicklinks]]\nname = \"N\"\n", "missing field `url`"),
        ("[[commands]]\nalias = \"x\"\n", "missing field `id`"),
        (
            "[[commands]]\nid = \"lock\"\n",
            "command id \"lock\" must be provider:key",
        ),
        (
            "[[commands]]\nid = \"system:lock\"\n[[commands]]\nid = \"system:lock\"\n",
            "command system:lock is listed twice",
        ),
        (
            "[[commands]]\nid = \"system:lock\"\nalias = \"l k\"\n",
            "command system:lock: alias must be one word",
        ),
        (
            "[[commands]]\nid = \"system:lock\"\nhotkey = \"Hyper+L\"\n",
            "command system:lock: ",
        ),
        (
            "[[commands]]\nid = \"system:lock\"\nhotkey = \"Ctrl+Alt+L\"\n\
             [[commands]]\nid = \"system:sleep\"\nhotkey = \"alt+ctrl+l\"\n",
            "is already the hotkey of command system:lock",
        ),
        (
            "[[commands]]\nid = \"system:lock\"\norder = -1\n",
            "order = -1",
        ),
        (
            "[[commands]]\nid = \"system:no-such-command\"\n",
            "unknown command id system:no-such-command",
        ),
    ];
    for (config, expected) in cases {
        let error = sandbox.refused(config)?;
        assert!(error.contains(expected), "{config}: {error}");
    }
    let taken = format!("[[commands]]\nid = \"system:lock\"\nhotkey = \"{launcher}\"\n");
    let error = sandbox.refused(&taken)?;
    assert!(
        error.contains("is already the hotkey of the launcher"),
        "{error}"
    );
    Ok(())
}

/// The launcher hotkey in `quickhop config default`, which differs between a release and a
/// development build.
fn default_hotkey(default_toml: &str) -> TestResult<String> {
    let value = default_toml
        .lines()
        .find_map(|line| line.strip_prefix("hotkey = "))
        .ok_or("no hotkey in the defaults")?;
    Ok(value.trim_matches('"').to_owned())
}

/// A typo in a key fails in every table instead of being ignored.
#[test]
fn unknown_keys_are_refused_in_every_table() -> TestResult {
    let sandbox = Sandbox::new("")?;
    for table in [
        "",
        "[providers]\n",
        "[apps]\n",
        "[scripts]\n",
        "[files]\n",
        "[clipboard]\n",
        "[[quicklinks]]\nname = \"N\"\nurl = \"u\"\n",
        "[[commands]]\nid = \"system:lock\"\n",
    ] {
        let error = sandbox.refused(&format!("{table}colour = 1\n"))?;
        assert!(error.contains("unknown field `colour`"), "{table}: {error}");
    }
    let error = sandbox.refused("[look]\ntheme = \"dark\"\n")?;
    assert!(error.contains("unknown field `look`"), "{error}");
    Ok(())
}

/// No file, an empty one and one of comments only all mean the defaults; a file that cannot
/// be read fails every command that needs it.
#[test]
fn empty_missing_and_unreadable_files() -> TestResult {
    let sandbox = Sandbox::new("")?;
    let defaults = sandbox.ok(&["config", "default"])?;
    assert!(defaults.contains("max_results = 8\n"), "{defaults}");
    sandbox.set_config(&defaults)?;
    assert!(sandbox.ok(&["config", "check"])?.ends_with(": ok\n"));

    for config in ["", "# nothing set\n\n"] {
        sandbox.set_config(config)?;
        assert!(sandbox.ok(&["config", "check"])?.ends_with(": ok\n"));
        assert_eq!(sandbox.ids("e")?.len(), 8, "the default max_results");
    }

    std::fs::remove_file(sandbox.path("config.toml"))?;
    assert_eq!(sandbox.ids("e")?.len(), 8, "no file: the defaults");
    assert_eq!(
        sandbox.ok(&["config", "path"])?.trim_end(),
        sandbox.path("config.toml").display().to_string()
    );
    let missing = sandbox.fails(&["config", "check"])?;
    assert!(missing.contains("cannot read"), "{missing}");
    assert!(missing.contains("config.toml"), "{missing}");

    std::fs::create_dir(sandbox.path("config.toml"))?;
    let unreadable = sandbox.fails(&["query", "e"])?;
    assert!(unreadable.contains("cannot read"), "{unreadable}");
    Ok(())
}

#[test]
fn max_results_caps_every_search_and_limit_overrides_it() -> TestResult {
    let sandbox = Sandbox::new("max_results = 3\n")?;
    assert_eq!(sandbox.ids("e")?.len(), 3);
    let limited: Value =
        serde_json::from_str(&sandbox.ok(&["query", "--json", "--limit", "5", "e"])?)?;
    assert_eq!(limited["results"].as_array().map(Vec::len), Some(5));
    sandbox.set_config("max_results = 100\n")?;
    assert!(sandbox.ids("e")?.len() > 8);
    Ok(())
}

/// What each `[providers]` switch finds; `files` needs an index of `tree`.
fn provider_cases() -> [(&'static str, &'static str, &'static str); 8] {
    [
        ("apps", "firefox", "apps:"),
        ("calc", "2+2", "calc:result"),
        ("emoji", "smile", "emoji:1f604"),
        ("system", "lock", "system:lock"),
        ("quicklinks", "cr tokio", "quicklinks:crates"),
        ("scripts", "hello cli", "scripts:hello."),
        ("snippets", ";re", "snippets:reply"),
        ("files", "quarterly", "files:"),
    ]
}

#[test]
fn each_provider_switch_turns_its_results_off() -> TestResult {
    let sandbox = Sandbox::new("")?;
    let tree = sandbox.home().join("tree");
    std::fs::create_dir_all(&tree)?;
    std::fs::write(tree.join("quarterly-report.md"), "q3")?;
    let base = format!(
        "{}[scripts]\ndirs = [\"{}\"]\n[files]\nroots = [\"{}\"]\n\n\
         [[quicklinks]]\nname = \"Crates\"\nurl = \"https://crates.io/search?q={{query}}\"\nalias = \"cr\"\n",
        fixture_apps(),
        toml_path(&fixtures().join(SCRIPTS_DIR)),
        toml_path(&tree),
    );
    sandbox.set_config(&base)?;
    sandbox.set_snippets("[[snippets]]\nname = \"Reply\"\nkeyword = \";re\"\ntext = \"Hi\"\n")?;
    sandbox.ok(&["index"])?;

    let found = |ids: &[String], prefix: &str| ids.iter().any(|id| id.starts_with(prefix));
    for (key, text, prefix) in provider_cases() {
        let ids = sandbox.ids(text)?;
        assert!(found(&ids, prefix), "{key} on: {text} -> {ids:?}");
    }
    for (key, text, prefix) in provider_cases() {
        sandbox.set_config(&format!("{base}\n[providers]\n{key} = false\n"))?;
        let ids = sandbox.ids(text)?;
        assert!(!found(&ids, prefix), "{key} off: {text} -> {ids:?}");
        let doctor = sandbox.ok(&["doctor"])?;
        assert!(
            doctor.contains(&format!("provider {key}: disabled")),
            "{key}: {doctor}"
        );
    }
    Ok(())
}

/// `[apps] dirs` replaces the OS folders: an empty folder finds no application.
#[test]
fn apps_dirs_replace_the_os_defaults() -> TestResult {
    let sandbox = Sandbox::new(&fixture_apps())?;
    assert!(
        sandbox
            .ids("firefox")?
            .iter()
            .any(|id| id.starts_with("apps:"))
    );
    let empty = sandbox.path("no-apps");
    std::fs::create_dir(&empty)?;
    sandbox.set_config(&format!("[apps]\ndirs = [\"{}\"]\n", toml_path(&empty)))?;
    assert!(
        !sandbox
            .ids("firefox")?
            .iter()
            .any(|id| id.starts_with("apps:"))
    );
    let doctor = sandbox.ok(&["doctor"])?;
    assert!(
        doctor.contains("provider apps: enabled (0 items)"),
        "{doctor}"
    );
    Ok(())
}

/// `[scripts] dirs` adds folders after `scripts/` next to the config, with `~` expanded.
#[test]
fn scripts_dirs_expand_home_after_the_default_dir() -> TestResult {
    let sandbox = Sandbox::new("[scripts]\ndirs = [\"~/raycast\"]\n")?;
    let mine = sandbox.home().join("raycast");
    std::fs::create_dir_all(&mine)?;
    let hello = format!("hello.{SCRIPT_EXT}");
    std::fs::copy(fixtures().join(SCRIPTS_DIR).join(&hello), mine.join(&hello))?;
    assert_eq!(sandbox.ok(&["run", "hello"])?, "hello world\n");
    assert!(
        sandbox
            .ids("hello cli")?
            .contains(&format!("scripts:{hello}"))
    );

    let default = sandbox.path("scripts");
    std::fs::create_dir_all(&default)?;
    let doctor = sandbox.ok(&["doctor"])?;
    let order = format!(
        "scripts: {}\nscripts: {}\n",
        default.display(),
        mine.display()
    );
    assert!(doctor.contains(&order), "the default dir first: {doctor}");
    sandbox.set_config("")?;
    assert!(sandbox.fails(&["run", "hello"])?.contains("hello"));
    Ok(())
}

/// `[files] roots` (with `~`), `hidden` and `exclude` decide what `quickhop index` keeps.
#[test]
fn files_roots_hidden_and_exclude_shape_the_index() -> TestResult {
    let sandbox = Sandbox::new("")?;
    let tree = sandbox.home().join("tree");
    std::fs::create_dir_all(tree.join("private"))?;
    std::fs::write(tree.join("quarterly-report.md"), "")?;
    std::fs::write(tree.join(".quarterly-hidden.md"), "")?;
    std::fs::write(tree.join("private").join("quarterly-secret.md"), "")?;
    let outside = sandbox.path("outside-the-roots");
    std::fs::create_dir_all(&outside)?;
    std::fs::write(outside.join("quarterly-elsewhere.md"), "")?;

    let names = |extra: &str| -> TestResult<Vec<String>> {
        sandbox.set_config(&format!("[files]\nroots = [\"~/tree\"]\n{extra}"))?;
        sandbox.ok(&["index"])?;
        let mut names: Vec<String> = sandbox
            .ids("quarterly")?
            .iter()
            .filter_map(|id| id.strip_prefix("files:"))
            .filter_map(|path| Path::new(path).file_name()?.to_str().map(str::to_owned))
            .collect();
        names.sort();
        Ok(names)
    };
    assert_eq!(
        names("")?,
        ["quarterly-report.md", "quarterly-secret.md"],
        "roots only, no hidden files"
    );
    assert_eq!(
        names("hidden = true\n")?,
        [
            ".quarterly-hidden.md",
            "quarterly-report.md",
            "quarterly-secret.md"
        ]
    );
    assert_eq!(names("exclude = [\"private\"]\n")?, ["quarterly-report.md"]);
    Ok(())
}

/// The name is the title, `{query}` takes the percent-encoded rest of the query (every
/// occurrence), and the alias or the first word of the name pins the link.
#[test]
fn quicklinks_name_url_and_alias() -> TestResult {
    let sandbox = Sandbox::new(
        "[[quicklinks]]\nname = \"Crates\"\nurl = \"https://crates.io/search?q={query}\"\nalias = \"cr\"\n\n\
         [[quicklinks]]\nname = \"Docs\"\nurl = \"https://docs.rs/{query}/latest/{query}\"\n",
    )?;
    let first = |text: &str| -> TestResult<Value> {
        let json: Value = serde_json::from_str(&sandbox.ok(&["query", "--json", text])?)?;
        Ok(json["results"][0].clone())
    };
    let crates = first("cr a b&c")?;
    assert_eq!(crates["id"], "quicklinks:crates");
    assert_eq!(crates["tier"], "pinned");
    assert_eq!(crates["title"], "Crates: a b&c");
    assert_eq!(
        crates["actions"][0]["target"],
        "https://crates.io/search?q=a%20b%26c"
    );
    let docs = first("docs serde")?;
    assert_eq!(docs["id"], "quicklinks:docs");
    assert_eq!(
        docs["actions"][0]["target"],
        "https://docs.rs/serde/latest/serde"
    );
    let dry: Value = serde_json::from_str(&sandbox.ok(&[
        "launch",
        "quicklinks:crates",
        "--query",
        "cr tokio",
        "--dry-run",
    ])?)?;
    assert_eq!(dry["target"], "https://crates.io/search?q=tokio");
    Ok(())
}

/// `[[commands]] hotkey` is checked (above) and accepted, but nothing registers it yet, as
/// docs/configuration.md says; the item itself is unaffected.
#[test]
fn command_hotkeys_are_accepted_without_changing_the_item() -> TestResult {
    let sandbox = Sandbox::new("[[commands]]\nid = \"system:lock\"\nhotkey = \"Ctrl+Alt+L\"\n")?;
    assert!(sandbox.ok(&["config", "check"])?.ends_with(": ok\n"));
    assert_eq!(
        sandbox.ids("lock")?.first().map(String::as_str),
        Some("system:lock")
    );
    Ok(())
}

/// `[clipboard] max_age_days` drops what was not copied again in time; 0 keeps it.
#[test]
fn clipboard_max_age_days_drops_old_entries() -> TestResult {
    const DAY_MS: i64 = 24 * 60 * 60 * 1000;
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    )?;
    for (days, expected) in [
        (0, "new\nten days old\n"),
        (7, "new\n"),
        (30, "new\nten days old\n"),
    ] {
        let sandbox = Sandbox::new(&format!("[clipboard]\nmax_age_days = {days}\n"))?;
        std::fs::create_dir_all(sandbox.path("data"))?;
        let mut state = quickhop_store::state::State::open(&sandbox.path("data/state.sqlite"))?;
        state.record_clip("ten days old", None, now - 10 * DAY_MS)?;
        drop(state);
        sandbox
            .cmd()
            .args(["clipboard", "add"])
            .write_stdin("new")
            .assert()
            .success();
        assert_eq!(
            sandbox.ok(&["clipboard", "list"])?,
            expected,
            "max_age_days = {days}"
        );
    }
    Ok(())
}

/// The output of `command` once it ended, or an error when it still runs after 5 s (it is
/// killed then).
fn bounded(command: &mut std::process::Command) -> TestResult<std::process::Output> {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait()?.is_none() {
        if Instant::now() > deadline {
            child.kill()?;
            let output = child.wait_with_output()?;
            return Err(format!(
                "still running after 5 s; it printed {:?}",
                String::from_utf8_lossy(&output.stdout)
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(child.wait_with_output()?)
}

/// `[clipboard] enabled = false` hides the history command and stops every watcher from
/// recording: `quickhop clipboard watch` refuses, and the `clipboard add` that Wayland's
/// `wl-paste --watch` runs records nothing. Adding by hand still works.
#[test]
fn clipboard_enabled_false_stops_recording() -> TestResult {
    let sandbox = Sandbox::new("[clipboard]\nenabled = false\n")?;
    assert!(
        !sandbox
            .ids("clipboard history")?
            .contains(&"views:clipboard".to_owned())
    );
    let watch = bounded(sandbox.std_cmd().args(["clipboard", "watch"]))?;
    assert!(!watch.status.success(), "{watch:?}");
    let error = String::from_utf8(watch.stderr)?;
    assert!(error.contains("[clipboard] enabled = false"), "{error}");
    sandbox
        .cmd()
        .args(["clipboard", "add"])
        .env("QUICKHOP_WL_WATCH", "1")
        .env("CLIPBOARD_STATE", "data")
        .write_stdin("from wl-paste")
        .assert()
        .success();
    sandbox
        .cmd()
        .args(["clipboard", "add"])
        .write_stdin("by hand")
        .assert()
        .success();
    assert_eq!(sandbox.ok(&["clipboard", "list"])?, "by hand\n");
    Ok(())
}

/// `config/dev/`, the example `just dev-config` installs.
fn dev_example() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/dev")
}

/// Every `table.key` set in `text` (`[[array]]` keys as `array[].key`), comments skipped.
fn keys_set(text: &str) -> std::collections::BTreeSet<String> {
    let mut table = String::new();
    let mut keys = std::collections::BTreeSet::new();
    for line in text.lines().map(str::trim) {
        if let Some(array) = line.strip_prefix("[[").and_then(|l| l.strip_suffix("]]")) {
            table = format!("{array}[].");
        } else if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            table = format!("{name}.");
        } else if let Some((key, _)) = line.split_once(" = ")
            && !line.starts_with('#')
        {
            keys.insert(format!("{table}{key}"));
        }
    }
    keys
}

/// The example sets every key the defaults have, and every key of its array tables.
#[test]
fn the_dev_example_sets_every_option() -> TestResult {
    let sandbox = Sandbox::new("")?;
    let example = keys_set(&std::fs::read_to_string(dev_example().join("config.toml"))?);
    // The defaults hold `quicklinks = []` and `commands = []`; the example has entries instead.
    let mut expected = keys_set(&sandbox.ok(&["config", "default"])?);
    expected.remove("quicklinks");
    expected.remove("commands");
    for key in ["name", "url", "alias"] {
        expected.insert(format!("quicklinks[].{key}"));
    }
    for key in ["id", "alias", "enabled", "hotkey", "order"] {
        expected.insert(format!("commands[].{key}"));
    }
    let missing: Vec<&String> = expected.difference(&example).collect();
    assert!(
        missing.is_empty(),
        "config/dev/config.toml lacks {missing:?}"
    );
    let snippets = keys_set(&std::fs::read_to_string(
        dev_example().join("snippets.toml"),
    )?);
    for key in ["snippets[].name", "snippets[].text", "snippets[].keyword"] {
        assert!(
            snippets.contains(key),
            "config/dev/snippets.toml lacks {key}"
        );
    }
    Ok(())
}

/// The example as installed passes `config check` and each of its settings shows.
#[cfg(unix)]
#[test]
fn the_dev_example_passes_check_and_applies() -> TestResult {
    let sandbox = Sandbox::new("")?;
    std::fs::copy(
        dev_example().join("config.toml"),
        sandbox.path("config.toml"),
    )?;
    std::fs::copy(
        dev_example().join("snippets.toml"),
        sandbox.path("snippets.toml"),
    )?;
    let extra = sandbox.home().join(".config/quickhop-dev/scripts-extra");
    std::fs::create_dir_all(&extra)?;
    std::fs::copy(
        dev_example().join("scripts-extra/dev-hello.sh"),
        extra.join("dev-hello.sh"),
    )?;
    assert!(sandbox.ok(&["config", "check"])?.ends_with(": ok\n"));
    assert_eq!(
        sandbox.ok(&["run", "dev-hello"])?,
        "hello from the dev config\n"
    );
    assert_eq!(sandbox.ids("e")?.len(), 10, "max_results = 10");
    assert_eq!(
        sandbox.ids("lk")?.first().map(String::as_str),
        Some("system:lock")
    );
    assert_eq!(
        sandbox.ids(";re")?.first().map(String::as_str),
        Some("snippets:reply")
    );
    assert_eq!(
        sandbox.ids("gh x")?.first().map(String::as_str),
        Some("quicklinks:github search")
    );
    assert!(
        !sandbox
            .ids("shutdown")?
            .contains(&"system:shutdown".to_owned())
    );
    let root = sandbox.ids("")?;
    assert_eq!(
        root.get(..2),
        Some(&["system:lock".to_owned(), "emoji:1f604".to_owned()][..])
    );
    Ok(())
}

const SNIPPETS: &str = "[[snippets]]\nname = \"Reply\"\nkeyword = \";re\"\n\
                        text = \"Hi {argument name=\\\"who\\\" default=\\\"Ada\\\"}\"\n";

/// `name` makes the id, `keyword` finds it, `text` is what it expands to.
#[test]
fn snippets_name_keyword_and_text() -> TestResult {
    let sandbox = Sandbox::new("")?;
    sandbox.set_snippets(SNIPPETS)?;
    assert_eq!(
        sandbox.ids(";re")?.first().map(String::as_str),
        Some("snippets:reply")
    );
    assert_eq!(sandbox.ok(&["launch", "snippets:reply"])?, "Hi Ada\n");
    assert_eq!(
        sandbox.ok(&["launch", "snippets:reply", "--arg", "Bo"])?,
        "Hi Bo\n"
    );
    Ok(())
}

/// A broken `snippets.toml` fails `quickhop config check`, naming the file and the key, and
/// is a warning (not silence) in a search.
#[test]
fn config_check_validates_snippets_toml_too() -> TestResult {
    let sandbox = Sandbox::new("")?;
    for (snippets, expected) in [
        (format!("{SNIPPETS}colour = 1\n"), "unknown field `colour`"),
        (
            "[[snippets]]\nname = \"N\"\n".to_owned(),
            "missing field `text`",
        ),
        (
            "[[snippets]]\nname = \"N\"\ntext = 1\n".to_owned(),
            "text = 1",
        ),
        (
            "[[snippets]]\nname = \"N\"\ntext = \"a\"\n[[snippets]]\nname = \"n\"\ntext = \"b\"\n"
                .to_owned(),
            "snippet \"n\" is defined twice",
        ),
    ] {
        sandbox.set_snippets(&snippets)?;
        let error = sandbox.fails(&["config", "check"])?;
        assert!(error.contains("snippets.toml"), "{snippets}: {error}");
        assert!(error.contains(expected), "{snippets}: {error}");
        let json: Value = serde_json::from_str(&sandbox.ok(&["query", "--json", "n"])?)?;
        let warnings = json["warnings"].to_string();
        assert!(warnings.contains("snippets.toml"), "{warnings}");
    }
    sandbox.set_snippets(SNIPPETS)?;
    assert!(sandbox.ok(&["config", "check"])?.ends_with(": ok\n"));
    Ok(())
}

/// `quickhop reload` makes a running daemon apply the options it owns: the clipboard
/// watcher and its limits, the file index and its roots, and the settings `open` uses.
#[cfg(unix)]
mod reload {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Stdio};
    use std::time::{Duration, Instant};

    use super::{Sandbox, TestResult, toml_path};

    /// The daemon of `sandbox`, stopped and reaped when dropped, however the test ends.
    struct Daemon<'a> {
        sandbox: &'a Sandbox,
        child: Child,
    }

    impl<'a> Daemon<'a> {
        fn start(sandbox: &'a Sandbox, window: bool) -> TestResult<Self> {
            let mut command = sandbox.std_cmd();
            command.arg("daemon");
            if !window {
                command.arg("--no-window");
            }
            let mut child = command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?;
            let mut banner = String::new();
            BufReader::new(child.stdout.take().ok_or("no stdout")?).read_line(&mut banner)?;
            let daemon = Self { sandbox, child };
            if !banner.contains(" listening on ") {
                return Err(format!("unexpected banner {banner:?}").into());
            }
            Ok(daemon)
        }

        fn status(&self) -> TestResult<String> {
            self.sandbox.ok(&["daemon", "status"])
        }

        /// The status once `check` holds, within 10 s.
        fn status_until(&self, what: &str, check: impl Fn(&str) -> bool) -> TestResult<String> {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let status = self.status()?;
                if check(&status) {
                    return Ok(status);
                }
                if Instant::now() > deadline {
                    return Err(format!("waited for {what}; status: {status}").into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        fn reload(&self, config: &str) -> TestResult<String> {
            self.sandbox.set_config(config)?;
            self.sandbox.ok(&["reload"])
        }
    }

    impl Drop for Daemon<'_> {
        fn drop(&mut self) {
            let _ = self.sandbox.cmd().args(["daemon", "stop"]).output();
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if matches!(self.child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    const CLIPBOARD_OFF: &str = "clipboard: off ([clipboard] enabled = false)";

    #[test]
    fn reload_switches_the_clipboard_watcher() -> TestResult {
        let sandbox = Sandbox::new("[providers]\nfiles = false\n")?;
        let daemon = Daemon::start(&sandbox, false)?;
        assert!(!daemon.status()?.contains(CLIPBOARD_OFF));
        let off = daemon.reload("[providers]\nfiles = false\n[clipboard]\nenabled = false\n")?;
        assert!(off.contains(CLIPBOARD_OFF), "{off}");
        let on = daemon.reload("[providers]\nfiles = false\n")?;
        assert!(!on.contains(CLIPBOARD_OFF), "{on}");
        Ok(())
    }

    #[test]
    fn reload_restarts_the_file_index_over_the_new_roots() -> TestResult {
        let sandbox = Sandbox::new("")?;
        let (one, two) = (sandbox.home().join("one"), sandbox.home().join("two"));
        std::fs::create_dir_all(&one)?;
        std::fs::create_dir_all(&two)?;
        let roots = |roots: &[&std::path::Path]| {
            let list: Vec<String> = roots
                .iter()
                .map(|root| format!("\"{}\"", toml_path(root)))
                .collect();
            format!("[files]\nroots = [{}]\n", list.join(", "))
        };
        sandbox.set_config(&roots(&[&one]))?;
        // On macOS the daemon keeps an index only once `quickhop index` built one.
        sandbox.ok(&["index"])?;
        let daemon = Daemon::start(&sandbox, false)?;
        daemon.status_until("1 root", |s| s.contains(" 1 roots"))?;
        daemon.reload(&roots(&[&one, &two]))?;
        daemon.status_until("2 roots", |s| s.contains(" 2 roots"))?;
        let off = daemon.reload(&format!("{}[providers]\nfiles = false\n", roots(&[&one])))?;
        assert!(
            off.contains("file index: off ([providers] files = false)"),
            "{off}"
        );
        Ok(())
    }

    #[test]
    fn reload_changes_what_open_finds() -> TestResult {
        let sandbox = Sandbox::new("[providers]\nfiles = false\n")?;
        let daemon = Daemon::start(&sandbox, false)?;
        assert_eq!(
            sandbox.ok(&["open", "emoji:1f604", "--query", "smile"])?,
            "😄\n"
        );
        daemon.reload(
            "[providers]\nfiles = false\n[[commands]]\nid = \"emoji:1f604\"\nenabled = false\n",
        )?;
        sandbox.fails(&["open", "emoji:1f604", "--query", "smile"])?;
        daemon.reload("[providers]\nfiles = false\nemoji = false\n")?;
        sandbox.fails(&["open", "emoji:1f604", "--query", "smile"])?;
        daemon.reload("[providers]\nfiles = false\n")?;
        assert_eq!(
            sandbox.ok(&["open", "emoji:1f604", "--query", "smile"])?,
            "😄\n"
        );
        Ok(())
    }

    /// A reloaded `max_entries` applies to the next copy the running watcher records.
    #[cfg(target_os = "macos")]
    #[test]
    fn reload_applies_clipboard_limits_to_the_running_watcher() -> TestResult {
        use quickhop_platform_macos::pasteboard::Pasteboard;

        let sandbox = Sandbox::new("[providers]\nfiles = false\n")?;
        let board = Pasteboard::named(super::PASTEBOARD);
        let daemon = Daemon::start(&sandbox, false)?;
        daemon.status_until("the watcher", |s| s.contains("clipboard: watching"))?;
        daemon.reload("[providers]\nfiles = false\n[clipboard]\nmax_entries = 2\n")?;
        // The watcher polls every 250 ms; a copy between two polls replaces the previous one.
        for text in ["first copy", "second copy", "third copy"] {
            assert!(board.write_text(text, false));
            std::thread::sleep(Duration::from_millis(700));
        }
        assert_eq!(
            sandbox.ok(&["clipboard", "list"])?,
            "third copy\nsecond copy\n"
        );
        Ok(())
    }

    /// The launcher hotkey is registered with macOS and replaced on reload, while the window
    /// stays hidden (no key is pressed and nothing is shown).
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "registers a global hotkey: run in a logged-in macOS session with --ignored"]
    fn reload_replaces_the_registered_hotkey() -> TestResult {
        let sandbox = Sandbox::new("hotkey = \"Ctrl+Alt+Cmd+F14\"\n[providers]\nfiles = false\n")?;
        let daemon = Daemon::start(&sandbox, true)?;
        let bound = daemon.status_until("the hotkey", |s| !s.contains("hotkey: starting"))?;
        assert!(bound.contains("window: hidden"), "{bound}");
        assert!(bound.ends_with("hotkey: Cmd+Ctrl+Alt+F14\n"), "{bound}");
        let reloaded =
            daemon.reload("hotkey = \"ctrl+alt+cmd+f15\"\n[providers]\nfiles = false\n")?;
        assert!(
            reloaded.ends_with("hotkey: Cmd+Ctrl+Alt+F15\n"),
            "{reloaded}"
        );
        let refused = sandbox.refused("hotkey = \"Ctrl+Nope\"\n")?;
        assert!(refused.contains("hotkey: "), "{refused}");
        let reload = sandbox.fails(&["reload"])?;
        assert!(reload.contains("hotkey: "), "{reload}");
        assert!(daemon.status()?.ends_with("hotkey: Cmd+Ctrl+Alt+F15\n"));
        Ok(())
    }
}
