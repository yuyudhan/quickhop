//! End-to-end tests of the quickhop binary.

use std::error::Error;

use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;

type TestResult = Result<(), Box<dyn Error>>;

fn quickhop() -> Result<Command, Box<dyn Error>> {
    let mut cmd = Command::cargo_bin("quickhop")?;
    cmd.env_remove("XDG_CONFIG_HOME")
        .env_remove("APPDATA")
        .env_remove("QUICKHOP_CONFIG")
        .env_remove("QUICKHOP_DATA");
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
