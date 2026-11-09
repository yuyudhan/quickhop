//! Raycast's own script commands as goldens: the official templates and a sample of community
//! commands from <https://github.com/raycast/script-commands> (MIT, see
//! `fixtures/raycast-goldens/LICENSE-raycast-script-commands`), parsed with the quickhop
//! parser. Explicit assertions pin Raycast's documented semantics; the snapshot pins every
//! parsed field of every file.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use quickhop_core::{ScriptArgumentKind, ScriptMode};
use quickhop_providers::scripts::{self, ScriptCommand, ScriptsProvider};

use crate::common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn goldens_dir() -> PathBuf {
    common::fixtures().join("raycast-goldens")
}

/// Every golden script, sorted by file name.
fn golden_files() -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(goldens_dir())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| !n.to_string_lossy().starts_with("LICENSE"))
        })
        .collect();
    files.sort();
    Ok(files)
}

fn parse(name: &str) -> Result<ScriptCommand, Box<dyn std::error::Error>> {
    let path = goldens_dir().join(name);
    let text = std::fs::read_to_string(&path)?;
    Ok(scripts::parse(&path, &text)?)
}

/// `value` with the goldens directory replaced by `<dir>`, so snapshots match on every machine.
fn relative(value: &str) -> String {
    let dir = goldens_dir();
    Path::new(value)
        .strip_prefix(&dir)
        .map_or_else(|_| value.to_owned(), |p| format!("<dir>/{}", p.display()))
}

fn render(file: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned());
    let text = std::fs::read_to_string(file)?;
    let s = scripts::parse(file, &text)?;
    let opt = |v: Option<&str>| v.map_or_else(|| "-".to_owned(), relative);
    let mut out = format!("## {}\n", name.unwrap_or_default());
    writeln!(out, "title: {}", s.title)?;
    writeln!(out, "mode: {:?}", s.mode)?;
    writeln!(out, "package: {}", opt(s.package_name.as_deref()))?;
    writeln!(out, "description: {}", opt(s.description.as_deref()))?;
    writeln!(out, "icon: {}", opt(s.icon.as_deref()))?;
    writeln!(out, "iconDark: {}", opt(s.icon_dark.as_deref()))?;
    writeln!(out, "author: {}", opt(s.author.as_deref()))?;
    writeln!(out, "authorURL: {}", opt(s.author_url.as_deref()))?;
    writeln!(out, "platform: {}", opt(s.platform.as_deref()))?;
    let cwd = s.cwd.as_ref().map(|c| c.to_string_lossy().into_owned());
    writeln!(out, "cwd: {}", opt(cwd.as_deref()))?;
    writeln!(out, "needsConfirmation: {}", s.needs_confirmation)?;
    writeln!(out, "refresh: {:?}", s.refresh)?;
    for (index, a) in s.arguments.iter().enumerate() {
        writeln!(
            out,
            "argument{}: {:?} {:?} optional={} percentEncoded={} choices={:?}",
            index + 1,
            a.kind,
            a.placeholder,
            a.optional,
            a.percent_encoded,
            a.choices
        )?;
    }
    Ok(out)
}

#[test]
fn every_golden_parses() -> TestResult {
    let files = golden_files()?;
    assert_eq!(files.len(), 18, "{files:?}");
    let mut out = String::new();
    for file in &files {
        out.push_str(&render(file).map_err(|e| format!("{}: {e}", file.display()))?);
        out.push('\n');
    }
    insta::assert_snapshot!("raycast_goldens", out);
    Ok(())
}

#[test]
fn template_metadata() -> TestResult {
    let s = parse("script-command.template.sh")?;
    assert_eq!(s.title, "My First Script");
    assert_eq!(s.mode, ScriptMode::FullOutput);
    assert_eq!(s.package_name.as_deref(), Some("Raycast Scripts"));
    assert_eq!(s.icon.as_deref(), Some("🤖"));
    assert_eq!(s.cwd, Some(PathBuf::from("~")));
    assert!(!s.needs_confirmation);
    assert_eq!(s.author.as_deref(), Some("Your name"));
    assert_eq!(
        s.author_url.as_deref(),
        Some("An URL for one of your social medias")
    );
    for ext in ["applescript", "cs", "js", "php", "py", "rb", "swift"] {
        let other = parse(&format!("script-command.template.{ext}"))?;
        assert_eq!(other.title, "My First Script", "{ext}");
        assert_eq!(other.mode, ScriptMode::FullOutput, "{ext}");
    }
    Ok(())
}

#[test]
fn inline_refresh_and_confirmation() -> TestResult {
    let cpu = parse("inline-cpu-usage-percent.sh")?;
    assert_eq!(cpu.mode, ScriptMode::Inline);
    assert_eq!(cpu.refresh, Some(Duration::from_secs(10)));
    let caffeinate = parse("caffeinate-status.sh")?;
    assert_eq!(caffeinate.refresh, Some(Duration::from_secs(30)));
    assert!(!caffeinate.needs_confirmation);
    let quit = parse("quit-all-apps.swift")?;
    assert_eq!(quit.mode, ScriptMode::Silent);
    assert!(quit.needs_confirmation);
    Ok(())
}

#[test]
fn arguments() -> TestResult {
    let dock = parse("dock-set-position.sh")?;
    assert_eq!(dock.arguments.len(), 1);
    assert_eq!(dock.arguments[0].kind, ScriptArgumentKind::Dropdown);
    assert_eq!(dock.arguments[0].choices, ["left", "right", "bottom"]);
    let time = parse("time-calculator.js")?;
    let optional: Vec<bool> = time.arguments.iter().map(|a| a.optional).collect();
    assert_eq!(optional, [true, false, true]);
    let kill = parse("kill-a-process-on-port.sh")?;
    assert_eq!(kill.arguments[0].kind, ScriptArgumentKind::Text);
    assert_eq!(kill.arguments[0].placeholder, "Ports (e.g. 3000 5000)");
    Ok(())
}

#[test]
fn icons_and_inferred_package() -> TestResult {
    let wifi = parse("wifi.template.applescript")?;
    let dir = goldens_dir();
    assert_eq!(
        wifi.icon.as_deref().map(PathBuf::from),
        Some(dir.join("images/wifi-dark.png"))
    );
    assert_eq!(
        wifi.icon_dark.as_deref().map(PathBuf::from),
        Some(dir.join("images/wifi.png"))
    );
    // No packageName: Raycast uses the folder name.
    let kill = parse("kill-a-process-on-port.sh")?;
    assert_eq!(kill.package_name.as_deref(), Some("raycast-goldens"));
    Ok(())
}

#[test]
fn loader_skips_templates() {
    let (provider, warnings) = ScriptsProvider::load(&[goldens_dir()], None);
    let mut titles: Vec<&str> = provider.all().iter().map(|i| i.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(titles.len(), 9, "{titles:?} {warnings:?}");
    assert!(!titles.contains(&"My First Script"), "{titles:?}");
    assert!(!titles.contains(&"Toggle Wi-Fi"), "{titles:?}");
}
