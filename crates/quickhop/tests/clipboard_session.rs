//! `quickhop clipboard watch` and `paste` in real display servers: Xvfb with `xclip`, and
//! headless sway with `wl-copy`; a GTK window stands in for the focused editor.
//! Ignored by default since they need those programs; run them on Linux with
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

/// Starts Xvfb on a free display and returns it with the display name.
fn xvfb() -> Result<(Child, String), Box<dyn Error>> {
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
    Ok((xvfb, format!(":{}", number.trim())))
}

/// Starts a headless sway with its sockets in `runtime` and returns it with the Wayland
/// display name.
fn sway(runtime: &Path) -> Result<(Child, String), Box<dyn Error>> {
    let config = runtime.join("sway.conf");
    std::fs::write(&config, "output HEADLESS-1 resolution 640x480\n")?;
    let sway = Command::new("sway")
        .arg("-c")
        .arg(&config)
        .env("XDG_RUNTIME_DIR", runtime)
        .env("WLR_BACKENDS", "headless")
        .env("WLR_LIBINPUT_NO_DEVICES", "1")
        .env("WLR_RENDERER", "pixman")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let sway = Killed(vec![sway]);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let found = std::fs::read_dir(runtime)?
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            // `wayland-1`, not its `wayland-1.lock`.
            .find(|name| {
                name.strip_prefix("wayland-")
                    .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
            });
        if let Some(socket) = found {
            let mut sway = sway;
            let child = sway.0.pop().ok_or("no sway")?;
            return Ok((child, socket));
        }
        if Instant::now() > deadline {
            return Err("sway did not start".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Starts the GTK text window of `tests/fixtures/text_window.py` with `envs` and waits (up to
/// 10 s) until it has the keyboard focus; its text is written to `out`.
fn text_window(envs: &[(&str, &str)], out: &Path) -> Result<Child, Box<dyn Error>> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/text_window.py");
    let mut child = Command::new("python3")
        .arg(script)
        .arg(out)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .envs(envs.iter().copied())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = sender.send(line);
    });
    match receiver.recv_timeout(Duration::from_secs(10)) {
        Ok(line) if line.trim() == "ready" => Ok(child),
        other => {
            let _ = child.kill();
            Err(format!("the text window never took the focus: {other:?}").into())
        }
    }
}

/// Records `text` in the history as if copied.
fn add(data: &Path, text: &str) -> TestResult {
    let mut child = quickhop(data)
        .args(["clipboard", "add"])
        .stdin(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("no stdin")?
        .write_all(text.as_bytes())?;
    assert!(child.wait()?.success());
    Ok(())
}

/// The file's text once it equals `expected`, or as it is after 10 s.
fn written(out: &Path, expected: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = std::fs::read_to_string(out).unwrap_or_default();
        if text == expected || Instant::now() > deadline {
            return text;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "needs Xvfb and xclip"]
fn x11_copies_are_listed_newest_first() -> TestResult {
    let data = tempfile::tempdir()?;
    let (xvfb, display) = xvfb()?;
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

/// Owns the clipboard of the X server `display` from a thread, offering `offers` (target name
/// and bytes) as a password manager does with its extra hint target, until another owner
/// takes over or the server goes away.
fn own_x11(display: &str, offers: &[(&str, &[u8])]) -> Result<(), Box<dyn Error>> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{
        AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SELECTION_NOTIFY_EVENT,
        SelectionNotifyEvent, WindowClass,
    };
    use x11rb::wrapper::ConnectionExt as _;

    let (conn, screen) = x11rb::connect(Some(display))?;
    let root = conn.setup().roots.get(screen).ok_or("no screen")?.root;
    let window = conn.generate_id()?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new(),
    )?;
    let atom = |name: &str| -> Result<u32, Box<dyn Error>> {
        Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
    };
    let clipboard = atom("CLIPBOARD")?;
    let targets = atom("TARGETS")?;
    let offered = offers
        .iter()
        .map(|(name, bytes)| Ok((atom(name)?, bytes.to_vec())))
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    conn.set_selection_owner(window, clipboard, x11rb::CURRENT_TIME)?;
    conn.flush()?;
    std::thread::spawn(move || {
        let serve = || -> Result<(), Box<dyn Error>> {
            loop {
                let request = match conn.wait_for_event()? {
                    Event::SelectionRequest(request) => request,
                    Event::SelectionClear(_) => return Ok(()),
                    _ => continue,
                };
                let mut property = request.property;
                if request.target == targets {
                    let list: Vec<u32> = std::iter::once(targets)
                        .chain(offered.iter().map(|(target, _)| *target))
                        .collect();
                    conn.change_property32(
                        PropMode::REPLACE,
                        request.requestor,
                        property,
                        AtomEnum::ATOM,
                        &list,
                    )?;
                } else if let Some((target, bytes)) =
                    offered.iter().find(|(target, _)| *target == request.target)
                {
                    conn.change_property8(
                        PropMode::REPLACE,
                        request.requestor,
                        property,
                        *target,
                        bytes,
                    )?;
                } else {
                    property = x11rb::NONE;
                }
                conn.send_event(
                    false,
                    request.requestor,
                    EventMask::NO_EVENT,
                    SelectionNotifyEvent {
                        response_type: SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: request.time,
                        requestor: request.requestor,
                        selection: request.selection,
                        target: request.target,
                        property,
                    },
                )?;
                conn.flush()?;
            }
        };
        // The server goes away when the test ends.
        let _ = serve();
    });
    Ok(())
}

#[test]
#[ignore = "needs Xvfb and xclip"]
fn x11_password_manager_copies_are_never_recorded() -> TestResult {
    let data = tempfile::tempdir()?;
    let (xvfb, display) = xvfb()?;
    let mut children = Killed(vec![xvfb]);
    let mut watcher = quickhop(data.path());
    watcher.env("DISPLAY", &display);
    children.0.push(watch(watcher)?);
    let xclip = || {
        let mut xclip = Command::new("xclip");
        xclip
            .args(["-selection", "clipboard"])
            .env("DISPLAY", &display);
        xclip
    };

    copy_all(data.path(), &["before"], xclip)?;
    // KeePassXC's copy: the password as text, and the hint.
    own_x11(
        &display,
        &[
            ("UTF8_STRING", b"hunter2"),
            (quickhop_core::clipboard::PASSWORD_MANAGER_HINT, b"secret"),
        ],
    )?;
    // Long enough for the watcher to read it, were it to record it.
    std::thread::sleep(Duration::from_secs(1));
    copy_all(data.path(), &["after"], xclip)?;
    assert_eq!(history(data.path(), |_| true)?, ["after", "before"]);
    Ok(())
}

#[test]
#[ignore = "needs sway and wl-clipboard"]
fn wayland_copies_are_listed_newest_first() -> TestResult {
    let data = tempfile::tempdir()?;
    let runtime = tempfile::tempdir()?;
    let (sway, socket) = sway(runtime.path())?;
    let mut children = Killed(vec![sway]);
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

#[test]
#[ignore = "needs Xvfb, xclip and python3-gi with GTK 3"]
fn x11_paste_types_into_the_focused_window() -> TestResult {
    let data = tempfile::tempdir()?;
    let out = data.path().join("typed.txt");
    let (xvfb, display) = xvfb()?;
    let mut children = Killed(vec![xvfb]);
    children.0.push(text_window(
        &[("DISPLAY", &display), ("GDK_BACKEND", "x11")],
        &out,
    )?);
    add(data.path(), "pasted ✓ text")?;
    let pasted = quickhop(data.path())
        .env("DISPLAY", &display)
        .args(["clipboard", "paste"])
        .output()?;
    assert!(pasted.status.success(), "{pasted:?}");
    assert_eq!(String::from_utf8(pasted.stderr)?, "");
    assert_eq!(written(&out, "pasted ✓ text"), "pasted ✓ text");
    Ok(())
}

#[test]
#[ignore = "needs sway and wl-clipboard"]
fn wayland_paste_leaves_the_text_on_the_clipboard_and_says_so() -> TestResult {
    let data = tempfile::tempdir()?;
    let runtime = tempfile::tempdir()?;
    let (sway, socket) = sway(runtime.path())?;
    let _children = Killed(vec![sway]);
    add(data.path(), "pasted ✓ text")?;
    let pasted = quickhop(data.path())
        .env("XDG_RUNTIME_DIR", runtime.path())
        .env("WAYLAND_DISPLAY", &socket)
        .args(["clipboard", "paste"])
        .output()?;
    assert!(pasted.status.success(), "{pasted:?}");
    let said = String::from_utf8(pasted.stderr)?;
    assert!(said.contains("copied, not pasted"), "{said}");
    let clipboard = Command::new("wl-paste")
        .arg("--no-newline")
        .env("XDG_RUNTIME_DIR", runtime.path())
        .env("WAYLAND_DISPLAY", &socket)
        .output()?;
    assert_eq!(String::from_utf8(clipboard.stdout)?, "pasted ✓ text");
    Ok(())
}
