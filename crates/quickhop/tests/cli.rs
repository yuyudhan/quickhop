//! End-to-end tests of the quickhop binary.

use std::error::Error;

use assert_cmd::Command;

type TestResult = Result<(), Box<dyn Error>>;

fn quickhop() -> Result<Command, Box<dyn Error>> {
    let mut cmd = Command::cargo_bin("quickhop")?;
    cmd.env_remove("XDG_CONFIG_HOME")
        .env_remove("APPDATA")
        .env_remove("QUICKHOP_CONFIG");
    Ok(cmd)
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
            .arg("doctor"),
    )?;
    assert!(text.contains("absent, using defaults"), "{text}");
    assert!(text.contains("provider calc: enabled"), "{text}");
    Ok(())
}
