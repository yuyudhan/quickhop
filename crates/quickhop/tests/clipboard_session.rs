//! `quickhop clipboard watch` in real display servers: Xvfb with `xclip`, and headless sway
//! with `wl-copy`. Ignored by default since they need those programs; run them on Linux with
//! `cargo test -p quickhop --test clipboard_session -- --ignored`.
#![cfg(all(unix, not(target_os = "macos")))]

use std::error::Error;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

type TestResult = Result<(), Box<dyn Error>>;

/// Child processes killed when the test ends, however it ends.
struct Killed(Vec<Child>);

impl Drop for Killed {
    fn drop(&mut self) {
        for child in self.0.iter_mut().rev() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// quickhop with a private data directory and no user configuration in reach.
fn quickhop(data: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_quickhop"));
    cmd.env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("HOME", "/nonexistent-quickhop-home")
        .env("QUICKHOP_CONFIG", data.join("config.toml"))
        .env("QUICKHOP_DATA", data);
    cmd
}

/// Starts `quickhop clipboard watch` from `cmd` (`quickhop` plus the display variables) and
/// waits until it watches.
fn watch(mut cmd: Command) -> Result<Child, Box<dyn Error>> {
    let mut child = cmd
        .args(["clipboard", "watch"])
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut line = String::new();
    BufReader::new(stdout).read_line(&mut line)?;
    assert!(line.starts_with("watching the"), "{line}");
    // The banner comes just before the watcher subscribes.
    std::thread::sleep(Duration::from_millis(700));
    Ok(child)
}

/// Copies `text` with `program` (`xclip` or `wl-copy`), which keeps serving it in the
/// background until another owner takes over.
fn copy(program: &mut Command, text: &str) -> TestResult {
    let mut child = program.stdin(Stdio::piped()).spawn()?;
    child
        .stdin
        .take()
        .ok_or("no stdin")?
        .write_all(text.as_bytes())?;
    child.wait()?;
    Ok(())
}

/// The history texts, newest first, once `done` holds for them (or as they are after 10 s).
fn history(data: &Path, done: impl Fn(&[String]) -> bool) -> Result<Vec<String>, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let out = quickhop(data)
            .args(["clipboard", "list", "--json", "--limit", "100"])
            .output()?;
        let entries: Value = serde_json::from_slice(&out.stdout)?;
        let texts: Vec<String> = entries
            .as_array()
            .ok_or("not an array")?
            .iter()
            .filter_map(|e| e["text"].as_str().map(str::to_owned))
            .collect();
        if done(&texts) || Instant::now() > deadline {
            return Ok(texts);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Copies each text with `program`, waiting until the history shows it first (copies by a
/// person are seconds apart; one that is replaced before quickhop read it is lost).
fn copy_all(data: &Path, texts: &[&str], program: impl Fn() -> Command) -> TestResult {
    for text in texts {
        copy(&mut program(), text)?;
        let latest = history(data, |t| t.first().is_some_and(|first| first == text))?;
        assert_eq!(
            latest.first().map(String::as_str),
            Some(*text),
            "{latest:?}"
        );
    }
    Ok(())
}

#[test]
#[ignore = "needs Xvfb and xclip"]
fn x11_copies_are_listed_newest_first() -> TestResult {
    let data = tempfile::tempdir()?;
    let mut xvfb = Command::new("Xvfb")
        .args([
            "-displayfd",
            "1",
            "-screen",
            "0",
            "640x480x24",
            "-nolisten",
            "tcp",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut number = String::new();
    BufReader::new(xvfb.stdout.take().ok_or("no stdout")?).read_line(&mut number)?;
    let display = format!(":{}", number.trim());
    let mut children = Killed(vec![xvfb]);
    let mut watcher = quickhop(data.path());
    watcher.env("DISPLAY", &display);
    children.0.push(watch(watcher)?);

    // Large texts arrive in INCR chunks.
    let big: String = (0..300_000_u32)
        .map(|i| char::from(b'a' + u8::try_from(i % 26).unwrap_or(0)))
        .collect();
    copy_all(
        data.path(),
        &["alpha", "bravo two", "charlie", &big, "alpha"],
        || {
            let mut xclip = Command::new("xclip");
            xclip
                .args(["-selection", "clipboard"])
                .env("DISPLAY", &display);
            xclip
        },
    )?;
    let texts = history(data.path(), |_| true)?;
    let short: Vec<&str> = texts.iter().map(|t| &t[..t.len().min(10)]).collect();
    assert_eq!(short, ["alpha", "abcdefghij", "charlie", "bravo two"]);
    assert_eq!(texts[1], big);
    Ok(())
}

#[test]
#[ignore = "needs sway and wl-clipboard"]
fn wayland_copies_are_listed_newest_first() -> TestResult {
    let data = tempfile::tempdir()?;
    let runtime = tempfile::tempdir()?;
    let config = data.path().join("sway.conf");
    std::fs::write(&config, "output HEADLESS-1 resolution 640x480\n")?;
    let sway = Command::new("sway")
        .arg("-c")
        .arg(&config)
        .env("XDG_RUNTIME_DIR", runtime.path())
        .env("WLR_BACKENDS", "headless")
        .env("WLR_LIBINPUT_NO_DEVICES", "1")
        .env("WLR_RENDERER", "pixman")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut children = Killed(vec![sway]);
    let deadline = Instant::now() + Duration::from_secs(10);
    let socket = loop {
        let found = std::fs::read_dir(runtime.path())?
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            // `wayland-1`, not its `wayland-1.lock`.
            .find(|name| {
                name.strip_prefix("wayland-")
                    .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
            });
        if let Some(socket) = found {
            break socket;
        }
        assert!(Instant::now() < deadline, "sway did not start");
        std::thread::sleep(Duration::from_millis(100));
    };
    let mut watcher = quickhop(data.path());
    watcher
        .env("XDG_RUNTIME_DIR", runtime.path())
        .env("WAYLAND_DISPLAY", &socket);
    children.0.push(watch(watcher)?);

    copy_all(data.path(), &["alpha", "bravo two", "charlie"], || {
        let mut wl_copy = Command::new("wl-copy");
        wl_copy
            .env("XDG_RUNTIME_DIR", runtime.path())
            .env("WAYLAND_DISPLAY", &socket);
        wl_copy
    })?;
    assert_eq!(
        history(data.path(), |_| true)?,
        ["charlie", "bravo two", "alpha"]
    );
    Ok(())
}
