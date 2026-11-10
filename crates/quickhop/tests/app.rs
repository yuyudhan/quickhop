//! quickhop.app's failures on macOS, without a screen (R21): the bundle's executable, opened
//! without arguments, reports an invalid configuration on standard error, naming the line, and
//! appends it to `~/Library/Logs/quickhop.log` (`quickhop-dev.log` for the development build
//! the tests run), then fails. `QUICKHOP_NO_ALERT` keeps the alert it shows a user off the
//! screen of whoever runs the tests.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::Command;

use quickhop_core::Flavour;

/// A copy of the built binary as the executable of its flavour's app under `dir`.
fn app_executable(dir: &Path) -> std::io::Result<std::path::PathBuf> {
    let macos = dir
        .join(format!("{}.app", Flavour::CURRENT.app_name()))
        .join("Contents/MacOS");
    std::fs::create_dir_all(&macos)?;
    let exe = macos.join("quickhop");
    std::fs::copy(env!("CARGO_BIN_EXE_quickhop"), &exe)?;
    Ok(exe)
}

#[test]
fn an_invalid_configuration_is_reported_with_its_line_and_logged() {
    let dir = tempfile::tempdir().unwrap();
    let exe = app_executable(dir.path()).unwrap();
    let home = dir.path().join("home");
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "max_results = \"x\"\n").unwrap();

    let output = Command::new(&exe)
        .env_clear()
        .env("HOME", &home)
        .env("QUICKHOP_CONFIG", &config)
        .env("QUICKHOP_DATA", dir.path().join("data"))
        .env("QUICKHOP_RUNTIME", dir.path().join("run"))
        .env("QUICKHOP_NO_ALERT", "1")
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("quickhop: invalid config"), "{stderr}");
    assert!(stderr.contains("line 1"), "{stderr}");
    let flavour = Flavour::CURRENT;
    let log = home
        .join("Library/Logs")
        .join(format!("{}.log", flavour.stem()));
    let log = std::fs::read_to_string(log).unwrap();
    assert!(
        log.contains(&format!("{}.app: invalid config", flavour.app_name())),
        "{log}"
    );
    assert!(log.contains("max_results"), "{log}");
}
