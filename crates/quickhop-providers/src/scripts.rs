//! Raycast-compatible script commands: executable files whose comments carry
//! `@raycast.*` metadata, from the directories in `[scripts] dirs` (none by default).
//! Format: <https://github.com/raycast/script-commands/blob/master/documentation/OVERVIEW.md>.
//!
//! Required headers: `schemaVersion` (1), `title`, `mode` (`fullOutput`, `compact`,
//! `silent`, `inline`). Optional: `packageName`, `icon`, `description`,
//! `currentDirectoryPath`, `needsConfirmation`, `refreshTime` (inline only) and
//! `argument1`..`argument3` (JSON; not allowed in inline mode). Inline scripts run when the
//! provider loads and show their first output line as the subtitle.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use quickhop_core::{
    Action, Cancel, Item, ItemId, Provider, ProviderError, Query, ScriptArgument,
    ScriptArgumentKind, ScriptMode,
};
use serde::Deserialize;

/// Provider name.
pub const NAME: &str = "scripts";

/// Files larger than this are not script commands.
const MAX_SCRIPT_BYTES: u64 = 512 * 1024;

/// Raycast's limit on arguments.
pub const MAX_ARGUMENTS: usize = 3;

/// Why a file is not a valid script command.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// A required header is missing.
    #[error("missing @raycast.{0}")]
    Missing(&'static str),
    /// `schemaVersion` is not 1.
    #[error("unsupported @raycast.schemaVersion {0}")]
    Schema(String),
    /// `mode` is not one of the four modes.
    #[error("unknown @raycast.mode {0}")]
    Mode(String),
    /// An `argumentN` header is not valid JSON of the expected shape.
    #[error("@raycast.argument{0}: {1}")]
    Argument(usize, String),
    /// `refreshTime` is not like `10s`, `5m`, `1h` or `1d`.
    #[error("invalid @raycast.refreshTime {0}")]
    RefreshTime(String),
    /// Inline scripts cannot take arguments.
    #[error("inline scripts cannot take arguments")]
    InlineArguments,
}

/// The parsed metadata of one script command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptCommand {
    /// The script file.
    pub path: PathBuf,
    /// `title`.
    pub title: String,
    /// `mode`.
    pub mode: ScriptMode,
    /// `packageName`.
    pub package_name: Option<String>,
    /// `description`.
    pub description: Option<String>,
    /// `icon`: an emoji, a URL, or a path (made absolute against the script's folder).
    pub icon: Option<String>,
    /// `currentDirectoryPath`, made absolute against the script's folder.
    pub cwd: Option<PathBuf>,
    /// `needsConfirmation`.
    pub needs_confirmation: bool,
    /// `refreshTime`, for inline scripts.
    pub refresh: Option<Duration>,
    /// `argument1`..`argument3`, in order.
    pub arguments: Vec<ScriptArgument>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawArgument {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    placeholder: String,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    percent_encoded: bool,
    #[serde(default)]
    secure: bool,
    #[serde(default)]
    data: Vec<RawChoice>,
}

#[derive(Deserialize)]
struct RawChoice {
    value: String,
}

/// The `@raycast.<key> <value>` pairs in `text`, from comment lines starting with `#`,
/// `//`, `--`, `;` or `'`.
pub fn headers(text: &str) -> Vec<(&str, &str)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let rest = ["#", "//", "--", ";", "'"]
                .iter()
                .find_map(|m| line.strip_prefix(m))?;
            let rest = rest.trim_start_matches(['#', '/', '-', ';']).trim_start();
            let rest = rest.strip_prefix("@raycast.")?;
            let (key, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            Some((key, value.trim()))
        })
        .collect()
}

fn parse_refresh(value: &str) -> Option<Duration> {
    let unit = value.chars().last()?;
    let number: u64 = value[..value.len() - unit.len_utf8()].parse().ok()?;
    let secs = match unit {
        's' => number,
        'm' => number.checked_mul(60)?,
        'h' => number.checked_mul(3600)?,
        'd' => number.checked_mul(86_400)?,
        _ => return None,
    };
    (secs > 0).then(|| Duration::from_secs(secs))
}

fn parse_argument(index: usize, json: &str) -> Result<ScriptArgument, ParseError> {
    let raw: RawArgument =
        serde_json::from_str(json).map_err(|e| ParseError::Argument(index, e.to_string()))?;
    let kind = match raw.kind.as_str() {
        "text" if raw.secure => ScriptArgumentKind::Password,
        "text" => ScriptArgumentKind::Text,
        "password" => ScriptArgumentKind::Password,
        "dropdown" => ScriptArgumentKind::Dropdown,
        other => return Err(ParseError::Argument(index, format!("unknown type {other}"))),
    };
    let choices: Vec<String> = raw.data.into_iter().map(|c| c.value).collect();
    if kind == ScriptArgumentKind::Dropdown && choices.is_empty() {
        return Err(ParseError::Argument(index, "dropdown without data".into()));
    }
    Ok(ScriptArgument {
        kind,
        placeholder: raw.placeholder,
        optional: raw.optional,
        percent_encoded: raw.percent_encoded,
        choices,
    })
}

fn resolve(dir: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    }
}

/// Parses the script at `path` with contents `text`.
///
/// # Errors
/// When a required header is missing or a header is invalid.
pub fn parse(path: &Path, text: &str) -> Result<ScriptCommand, ParseError> {
    let headers = headers(text);
    let get = |key: &str| {
        headers
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
            .filter(|v| !v.is_empty())
    };
    let schema = get("schemaVersion").ok_or(ParseError::Missing("schemaVersion"))?;
    if schema != "1" {
        return Err(ParseError::Schema(schema.into()));
    }
    let title = get("title").ok_or(ParseError::Missing("title"))?;
    let mode = match get("mode").ok_or(ParseError::Missing("mode"))? {
        "fullOutput" => ScriptMode::FullOutput,
        "compact" => ScriptMode::Compact,
        "silent" => ScriptMode::Silent,
        "inline" => ScriptMode::Inline,
        other => return Err(ParseError::Mode(other.into())),
    };
    let refresh = get("refreshTime")
        .map(|v| parse_refresh(v).ok_or_else(|| ParseError::RefreshTime(v.into())))
        .transpose()?;
    let mut arguments = Vec::new();
    for index in 1..=MAX_ARGUMENTS {
        if let Some(json) = get(&format!("argument{index}")) {
            arguments.push(parse_argument(index, json)?);
        }
    }
    if mode == ScriptMode::Inline && !arguments.is_empty() {
        return Err(ParseError::InlineArguments);
    }
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let icon = get("icon").map(|icon| {
        let looks_like_file = icon.contains('.') && !icon.contains("://");
        if looks_like_file {
            resolve(dir, icon).to_string_lossy().into_owned()
        } else {
            icon.to_owned()
        }
    });
    Ok(ScriptCommand {
        path: path.to_path_buf(),
        title: title.into(),
        mode,
        package_name: get("packageName").map(str::to_owned),
        description: get("description").map(str::to_owned),
        icon,
        cwd: get("currentDirectoryPath").map(|v| resolve(dir, v)),
        needs_confirmation: get("needsConfirmation") == Some("true"),
        refresh,
        arguments,
    })
}

/// The command's action.
pub fn action(script: &ScriptCommand) -> Action {
    Action::Script {
        path: script.path.to_string_lossy().into_owned(),
        mode: script.mode,
        cwd: script
            .cwd
            .as_ref()
            .map(|c| c.to_string_lossy().into_owned()),
        arguments: script.arguments.clone(),
        confirm: script.needs_confirmation,
    }
}

/// Why a script could not be run.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The action is not a script.
    #[error("not a script command")]
    NotAScript,
    /// A required argument is missing, or too many were given.
    #[error("{0}")]
    Arguments(String),
    /// The process could not be started or read.
    #[error("cannot run {path}: {source}")]
    Io {
        /// The script.
        path: String,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// It ran longer than allowed and was killed.
    #[error("{0} timed out")]
    Timeout(String),
}

/// What a finished script printed, shaped for its mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptOutput {
    /// Whether it exited with status 0.
    pub success: bool,
    /// Everything on standard output.
    pub stdout: String,
    /// Everything on standard error.
    pub stderr: String,
    /// What the launcher shows: all output for `fullOutput`, the last line for `compact`
    /// and `silent`, the first line for `inline`. Failed scripts show the last line of
    /// standard error when standard output is empty.
    pub message: String,
}

/// How `path` is started: directly on Unix (the kernel reads the shebang); on Windows by
/// extension (`.ps1`, `.bat`/`.cmd`) or else by the shebang's interpreter, default `sh`.
pub fn invocation(path: &Path, first_line: &str, windows: bool) -> (String, Vec<String>) {
    let file = path.to_string_lossy().into_owned();
    if !windows {
        return (file, Vec::new());
    }
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "ps1" => (
            "powershell.exe".into(),
            vec![
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                file,
            ],
        ),
        "bat" | "cmd" => ("cmd.exe".into(), vec!["/C".into(), file]),
        _ => {
            let mut words = first_line
                .strip_prefix("#!")
                .unwrap_or_default()
                .split_whitespace();
            let interpreter = words
                .next()
                .map(|w| w.rsplit('/').next().unwrap_or(w).to_owned());
            let (program, mut args): (String, Vec<String>) = match interpreter.as_deref() {
                Some("env") => match words.next() {
                    Some(program) => (program.to_owned(), words.map(str::to_owned).collect()),
                    None => ("sh".into(), Vec::new()),
                },
                Some(program) => (program.to_owned(), words.map(str::to_owned).collect()),
                None => ("sh".into(), Vec::new()),
            };
            args.push(file);
            (program, args)
        }
    }
}

fn check_arguments(specs: &[ScriptArgument], values: &[String]) -> Result<Vec<String>, RunError> {
    if values.len() > specs.len() {
        return Err(RunError::Arguments(format!(
            "takes {} argument(s), got {}",
            specs.len(),
            values.len()
        )));
    }
    let mut out = Vec::new();
    for (index, spec) in specs.iter().enumerate() {
        let value = values.get(index).map(String::as_str).unwrap_or_default();
        if value.is_empty() && !spec.optional {
            return Err(RunError::Arguments(format!(
                "argument {} ({}) is required",
                index + 1,
                spec.placeholder
            )));
        }
        if spec.kind == ScriptArgumentKind::Dropdown
            && !value.is_empty()
            && !spec.choices.iter().any(|c| c == value)
        {
            return Err(RunError::Arguments(format!(
                "argument {} must be one of {}",
                index + 1,
                spec.choices.join(", ")
            )));
        }
        out.push(if spec.percent_encoded {
            utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
        } else {
            value.to_owned()
        });
    }
    Ok(out)
}

fn shape(mode: ScriptMode, success: bool, stdout: &str, stderr: &str) -> String {
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    let message = match mode {
        ScriptMode::FullOutput => stdout.trim_end().to_owned(),
        ScriptMode::Compact | ScriptMode::Silent => {
            lines.last().copied().unwrap_or_default().trim().to_owned()
        }
        ScriptMode::Inline => lines.first().copied().unwrap_or_default().trim().to_owned(),
    };
    if message.is_empty() && !success {
        return stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_default()
            .trim()
            .to_owned();
    }
    message
}

fn read_pipe(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    if let Some(mut pipe) = pipe {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            // A read error leaves whatever arrived before it.
            let _ = pipe.read_to_end(&mut bytes);
            let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
        });
    }
    receiver
}

/// Runs a script action with `values` for its arguments, killing it after `timeout`.
///
/// # Errors
/// On a non-script action, bad arguments, a process that cannot start, or a timeout.
pub fn run(
    action: &Action,
    values: &[String],
    timeout: Duration,
) -> Result<ScriptOutput, RunError> {
    let Action::Script {
        path,
        mode,
        cwd,
        arguments,
        ..
    } = action
    else {
        return Err(RunError::NotAScript);
    };
    let checked = check_arguments(arguments, values)?;
    let io = |source| RunError::Io {
        path: path.clone(),
        source,
    };
    let script = Path::new(path);
    let first_line = std::fs::read_to_string(script)
        .map(|t| t.lines().next().unwrap_or_default().to_owned())
        .unwrap_or_default();
    let (program, mut argv) = invocation(script, &first_line, cfg!(windows));
    argv.extend(checked);
    let dir = cwd.as_ref().map_or_else(
        || {
            script
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        },
        PathBuf::from,
    );
    let mut child = Command::new(program)
        .args(argv)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(io)?;
    let stdout = read_pipe(child.stdout.take());
    let stderr = read_pipe(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(io)? {
            break status;
        }
        if Instant::now() >= deadline {
            // Already exited or not killable: either way it is abandoned.
            let _ = child.kill();
            let _ = child.wait();
            return Err(RunError::Timeout(path.clone()));
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = stdout
        .recv_timeout(Duration::from_secs(1))
        .unwrap_or_default();
    let stderr = stderr
        .recv_timeout(Duration::from_secs(1))
        .unwrap_or_default();
    Ok(ScriptOutput {
        success: status.success(),
        message: shape(*mode, status.success(), &stdout, &stderr),
        stdout,
        stderr,
    })
}

/// Turns a script into an item; `inline` is the first output line of an inline script.
pub fn to_item(script: &ScriptCommand, inline: Option<&str>) -> Item {
    let key = script
        .path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut item = Item::new(ItemId::new(NAME, key), script.title.clone(), action(script))
        .with_keywords(
            script
                .package_name
                .iter()
                .chain(script.description.iter())
                .cloned(),
        );
    if let Some(subtitle) = inline
        .filter(|s| !s.is_empty())
        .or(script.package_name.as_deref())
        .or(script.description.as_deref())
    {
        item = item.with_subtitle(subtitle);
    }
    if let Some(icon) = &script.icon {
        item = item.with_icon(icon.clone());
    }
    item
}

/// The script commands found when the provider was loaded.
pub struct ScriptsProvider {
    items: Vec<Item>,
}

impl ScriptsProvider {
    /// Scans each directory (not recursively; hidden files skipped). Inline scripts are
    /// run once with `inline_timeout`, or not at all when it is `None`. Invalid scripts,
    /// unreadable directories and failing inline scripts become warnings.
    pub fn load(dirs: &[PathBuf], inline_timeout: Option<Duration>) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let mut items = Vec::new();
        for dir in dirs {
            let entries = match std::fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(error) => {
                    warnings.push(format!("{}: {error}", dir.display()));
                    continue;
                }
            };
            let mut paths: Vec<PathBuf> =
                entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
            paths.sort();
            for path in paths {
                let hidden = path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'));
                let small_file = path
                    .metadata()
                    .is_ok_and(|m| m.is_file() && m.len() <= MAX_SCRIPT_BYTES);
                if hidden || !small_file {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&path) else {
                    warnings.push(format!("{}: unreadable", path.display()));
                    continue;
                };
                let text = String::from_utf8_lossy(&bytes);
                if !text.contains("@raycast.") {
                    continue;
                }
                let script = match parse(&path, &text) {
                    Ok(script) => script,
                    Err(error) => {
                        warnings.push(format!("{}: {error}", path.display()));
                        continue;
                    }
                };
                let inline = match (script.mode, inline_timeout) {
                    (ScriptMode::Inline, Some(timeout)) => {
                        match run(&action(&script), &[], timeout) {
                            Ok(output) => Some(output.message),
                            Err(error) => {
                                warnings.push(error.to_string());
                                None
                            }
                        }
                    }
                    _ => None,
                };
                items.push(to_item(&script, inline.as_deref()));
            }
        }
        (Self { items }, warnings)
    }

    /// Every script command found.
    pub fn all(&self) -> &[Item] {
        &self.items
    }
}

impl Provider for ScriptsProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn items(&self, _query: &Query, cancel: &Cancel) -> Result<Vec<Item>, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled {
                provider: NAME.into(),
            });
        }
        Ok(self.items.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "#!/bin/bash\n# @raycast.schemaVersion 1\n# @raycast.title Say Hi\n";

    fn script(extra: &str) -> Result<ScriptCommand, ParseError> {
        parse(Path::new("/s/hi.sh"), &format!("{HEADER}{extra}"))
    }

    #[test]
    fn parses_headers_in_any_comment_style() {
        let text = "// @raycast.title A\n-- @raycast.mode compact\n#  @raycast.icon 🤖\nnot @raycast.x y\n";
        assert_eq!(
            headers(text),
            vec![("title", "A"), ("mode", "compact"), ("icon", "🤖")]
        );
    }

    #[test]
    fn all_four_modes() {
        for (name, mode) in [
            ("fullOutput", ScriptMode::FullOutput),
            ("compact", ScriptMode::Compact),
            ("silent", ScriptMode::Silent),
            ("inline", ScriptMode::Inline),
        ] {
            assert_eq!(
                script(&format!("# @raycast.mode {name}\n")).unwrap().mode,
                mode
            );
        }
        assert_eq!(
            script("# @raycast.mode loud\n"),
            Err(ParseError::Mode("loud".into()))
        );
    }

    #[test]
    fn required_headers() {
        assert_eq!(
            parse(Path::new("x"), "# @raycast.title A\n"),
            Err(ParseError::Missing("schemaVersion"))
        );
        assert_eq!(
            parse(Path::new("x"), "# @raycast.schemaVersion 2\n"),
            Err(ParseError::Schema("2".into()))
        );
        assert_eq!(script(""), Err(ParseError::Missing("mode")));
    }

    #[test]
    fn optional_headers_and_arguments() {
        let s = script(concat!(
            "# @raycast.mode fullOutput\n",
            "# @raycast.packageName Greetings\n",
            "# @raycast.icon images/hi.png\n",
            "# @raycast.currentDirectoryPath ~\n",
            "# @raycast.needsConfirmation true\n",
            "# @raycast.argument1 { \"type\": \"text\", \"placeholder\": \"Name\", \"percentEncoded\": true }\n",
            "# @raycast.argument2 { \"type\": \"dropdown\", \"placeholder\": \"Tone\", \"optional\": true, \"data\": [{\"title\": \"Loud\", \"value\": \"loud\"}] }\n",
            "# @raycast.argument3 { \"type\": \"text\", \"placeholder\": \"Secret\", \"secure\": true }\n",
        ))
        .unwrap();
        assert_eq!(s.package_name.as_deref(), Some("Greetings"));
        assert_eq!(
            s.icon,
            Some(
                Path::new("/s")
                    .join("images/hi.png")
                    .to_string_lossy()
                    .into_owned()
            )
        );
        assert_eq!(s.cwd, Some(Path::new("/s").join("~")));
        assert!(s.needs_confirmation);
        assert_eq!(s.arguments.len(), 3);
        assert!(s.arguments[0].percent_encoded);
        assert_eq!(s.arguments[1].choices, vec!["loud"]);
        assert_eq!(s.arguments[2].kind, ScriptArgumentKind::Password);
        assert!(action(&s).needs_confirmation());
    }

    #[test]
    fn invalid_optional_headers() {
        assert!(matches!(
            script("# @raycast.mode compact\n# @raycast.argument1 {bad}\n"),
            Err(ParseError::Argument(1, _))
        ));
        assert!(matches!(
            script("# @raycast.mode compact\n# @raycast.argument1 {\"type\":\"dropdown\"}\n"),
            Err(ParseError::Argument(1, _))
        ));
        assert_eq!(
            script("# @raycast.mode inline\n# @raycast.argument1 {\"type\":\"text\"}\n"),
            Err(ParseError::InlineArguments)
        );
        assert_eq!(
            script("# @raycast.mode inline\n# @raycast.refreshTime soon\n"),
            Err(ParseError::RefreshTime("soon".into()))
        );
        let ok = script("# @raycast.mode inline\n# @raycast.refreshTime 5m\n").unwrap();
        assert_eq!(ok.refresh, Some(Duration::from_secs(300)));
    }

    #[test]
    fn argument_checks() {
        let s = script(concat!(
            "# @raycast.mode compact\n",
            "# @raycast.argument1 { \"type\": \"text\", \"placeholder\": \"q\", \"percentEncoded\": true }\n",
            "# @raycast.argument2 { \"type\": \"dropdown\", \"placeholder\": \"d\", \"optional\": true, \"data\": [{\"title\": \"A\", \"value\": \"a\"}] }\n",
        ))
        .unwrap();
        assert_eq!(
            check_arguments(&s.arguments, &["a b".into()]).unwrap(),
            vec!["a%20b", ""]
        );
        assert!(check_arguments(&s.arguments, &[]).is_err());
        assert!(check_arguments(&s.arguments, &["x".into(), "z".into()]).is_err());
        assert!(check_arguments(&s.arguments, &["x".into(), "a".into(), "y".into()]).is_err());
    }

    #[test]
    fn output_shapes_per_mode() {
        let out = "\nfirst\nmiddle\nlast\n\n";
        assert_eq!(
            shape(ScriptMode::FullOutput, true, out, ""),
            "\nfirst\nmiddle\nlast"
        );
        assert_eq!(shape(ScriptMode::Compact, true, out, ""), "last");
        assert_eq!(shape(ScriptMode::Silent, true, out, ""), "last");
        assert_eq!(shape(ScriptMode::Inline, true, out, ""), "first");
        assert_eq!(
            shape(ScriptMode::Compact, false, "", "warn\nboom\n"),
            "boom"
        );
    }

    #[test]
    fn windows_invocation() {
        let ps = invocation(Path::new("C:/s/a.ps1"), "", true);
        assert_eq!(ps.0, "powershell.exe");
        assert_eq!(invocation(Path::new("a.cmd"), "", true).0, "cmd.exe");
        assert_eq!(
            invocation(Path::new("a.py"), "#!/usr/bin/env python3 -u", true),
            ("python3".into(), vec!["-u".into(), "a.py".into()])
        );
        assert_eq!(
            invocation(Path::new("a.sh"), "#!/bin/bash", true),
            ("bash".into(), vec!["a.sh".into()])
        );
        assert_eq!(
            invocation(Path::new("a"), "echo", true),
            ("sh".into(), vec!["a".into()])
        );
        assert_eq!(
            invocation(Path::new("/s/a.sh"), "#!/bin/bash", false),
            ("/s/a.sh".into(), vec![])
        );
    }
}
