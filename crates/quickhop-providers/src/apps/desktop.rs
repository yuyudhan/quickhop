//! XDG desktop entries (`.desktop` files), per the Desktop Entry Specification 1.5:
//! <https://specifications.freedesktop.org/desktop-entry-spec/latest/>.
//!
//! Handles localised keys (`Name[de_DE]`), string escapes, `Keywords`/`GenericName`,
//! `Exec` quoting and field codes, `TryExec`, `NoDisplay`/`Hidden`, `OnlyShowIn`/
//! `NotShowIn`, `Terminal`, and desktop-file-id precedence between directories.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use quickhop_core::{Action, Item, ItemId};

use super::NAME;
use super::icons::IconResolver;
use crate::env::Env;

/// A parsed `[Desktop Entry]` group, localised for the session.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DesktopEntry {
    /// `Type`.
    pub kind: String,
    /// `Name`.
    pub name: String,
    /// `GenericName`.
    pub generic_name: Option<String>,
    /// `Comment`.
    pub comment: Option<String>,
    /// `Keywords`.
    pub keywords: Vec<String>,
    /// `Exec`, unescaped but not yet split.
    pub exec: Option<String>,
    /// `TryExec`.
    pub try_exec: Option<String>,
    /// `Icon`.
    pub icon: Option<String>,
    /// `NoDisplay`.
    pub no_display: bool,
    /// `Hidden`.
    pub hidden: bool,
    /// `OnlyShowIn`.
    pub only_show_in: Vec<String>,
    /// `NotShowIn`.
    pub not_show_in: Vec<String>,
    /// `Terminal`.
    pub terminal: bool,
}

/// Why an `Exec` value could not be turned into a command.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExecError {
    /// A double quote was opened but never closed.
    #[error("unterminated quote in Exec")]
    UnterminatedQuote,
    /// Nothing is left after removing field codes.
    #[error("empty Exec")]
    Empty,
}

/// The `applications` directories in precedence order: `$XDG_DATA_HOME` (default
/// `~/.local/share`), then each of `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`).
pub fn default_dirs(env: &Env) -> Vec<PathBuf> {
    data_dirs(env)
        .into_iter()
        .map(|d| d.join("applications"))
        .collect()
}

/// `$XDG_DATA_HOME` followed by `$XDG_DATA_DIRS`, with the spec's defaults.
pub fn data_dirs(env: &Env) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = env
        .path("XDG_DATA_HOME")
        .or_else(|| env.home().map(|h| h.join(".local/share")))
    {
        dirs.push(home);
    }
    let system = env
        .var("XDG_DATA_DIRS")
        .unwrap_or("/usr/local/share:/usr/share");
    dirs.extend(
        system
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from),
    );
    dirs
}

/// The session's locale names in match order, from `LC_ALL`, `LC_MESSAGES` or `LANG`:
/// for `de_DE.UTF-8@euro` that is `de_DE@euro`, `de_DE`, `de@euro`, `de`.
pub fn locales(env: &Env) -> Vec<String> {
    let Some(raw) = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|v| env.var(v))
    else {
        return Vec::new();
    };
    let (rest, modifier) = raw
        .split_once('@')
        .map_or((raw, None), |(r, m)| (r, Some(m)));
    let rest = rest.split('.').next().unwrap_or(rest);
    let (lang, country) = rest
        .split_once('_')
        .map_or((rest, None), |(l, c)| (l, Some(c)));
    if lang.is_empty() || lang == "C" || lang == "POSIX" {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let (Some(c), Some(m)) = (country, modifier) {
        out.push(format!("{lang}_{c}@{m}"));
    }
    if let Some(c) = country {
        out.push(format!("{lang}_{c}"));
    }
    if let Some(m) = modifier {
        out.push(format!("{lang}@{m}"));
    }
    out.push(lang.to_owned());
    out
}

/// Undoes the spec's string escapes (`\s`, `\n`, `\t`, `\r`, `\\`); with `list`, also
/// splits on unescaped `;` (and keeps `\;` as a literal semicolon).
fn unescape(value: &str, list: bool) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('s') => current.push(' '),
                Some('n') => current.push('\n'),
                Some('t') => current.push('\t'),
                Some('r') => current.push('\r'),
                Some(';') if list => current.push(';'),
                Some(other) => {
                    if other != '\\' {
                        current.push('\\');
                    }
                    current.push(other);
                }
                None => current.push('\\'),
            },
            ';' if list => items.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    if !list || !current.is_empty() {
        items.push(current);
    }
    if list {
        items.retain(|i| !i.trim().is_empty());
        for item in &mut items {
            *item = item.trim().to_owned();
        }
    }
    items
}

/// Parses the `[Desktop Entry]` group of `text`, picking the best of `locales` for
/// localised keys. `None` when the group or `Name` is missing.
pub fn parse(text: &str, locales: &[String]) -> Option<DesktopEntry> {
    // key -> locale ("" for unlocalised) -> raw value
    let mut keys: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
    let mut in_entry = false;
    let mut seen_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            seen_entry |= in_entry;
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim_end();
        let (base, locale) = match key.split_once('[') {
            Some((base, rest)) => (base, rest.strip_suffix(']').unwrap_or(rest)),
            None => (key, ""),
        };
        keys.entry(base)
            .or_default()
            .entry(locale)
            .or_insert(value.trim_start());
    }
    if !seen_entry {
        return None;
    }
    let raw = |key: &str| -> Option<&str> {
        let values = keys.get(key)?;
        locales
            .iter()
            .find_map(|l| values.get(l.as_str()).copied())
            .or_else(|| values.get("").copied())
    };
    let string = |key: &str| {
        raw(key)
            .map(|v| unescape(v, false).concat())
            .filter(|v| !v.is_empty())
    };
    let list = |key: &str| raw(key).map(|v| unescape(v, true)).unwrap_or_default();
    let boolean = |key: &str| raw(key).is_some_and(|v| v.trim() == "true");
    Some(DesktopEntry {
        kind: string("Type").unwrap_or_default(),
        name: string("Name")?,
        generic_name: string("GenericName"),
        comment: string("Comment"),
        keywords: list("Keywords"),
        exec: string("Exec"),
        try_exec: string("TryExec"),
        icon: string("Icon"),
        no_display: boolean("NoDisplay"),
        hidden: boolean("Hidden"),
        only_show_in: list("OnlyShowIn"),
        not_show_in: list("NotShowIn"),
        terminal: boolean("Terminal"),
    })
}

/// Splits an `Exec` value into arguments per the spec's quoting rules: inside double
/// quotes, `\"`, `` \` ``, `\$` and `\\` are literal characters.
fn split_exec(exec: &str) -> Result<Vec<String>, ExecError> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.peek() {
                            Some(&n @ ('"' | '`' | '$' | '\\')) => {
                                current.push(n);
                                chars.next();
                            }
                            _ => current.push('\\'),
                        },
                        Some(other) => current.push(other),
                        None => return Err(ExecError::UnterminatedQuote),
                    }
                }
            }
            c if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            _ => {
                started = true;
                current.push(c);
            }
        }
    }
    if started {
        args.push(current);
    }
    Ok(args)
}

/// Expands field codes in one argument: `%c` the name, `%k` the file, `%%` a percent sign;
/// file/URL codes (`%f %F %u %U`) and deprecated ones (`%d %D %n %N %v %m`) vanish because
/// the launcher passes no files.
fn expand_codes(arg: &str, name: &str, file: &Path) -> String {
    let mut out = String::new();
    let mut chars = arg.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('c') => out.push_str(name),
            Some('k') => out.push_str(&file.to_string_lossy()),
            Some(_) | None => {}
        }
    }
    out
}

/// The command an entry runs: `Exec` split and with field codes expanded. `%i` becomes
/// `--icon <Icon>` (or nothing without an icon); an argument that was only a file code is
/// dropped. `Terminal=true` entries run inside `$TERMINAL` (default `x-terminal-emulator`)
/// with `-e`.
///
/// # Errors
/// On an unterminated quote or when nothing is left to run.
pub fn command(
    entry: &DesktopEntry,
    file: &Path,
    env: &Env,
) -> Result<(String, Vec<String>), ExecError> {
    let mut argv = Vec::new();
    for arg in split_exec(entry.exec.as_deref().unwrap_or_default())? {
        match arg.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
            "%i" => {
                if let Some(icon) = &entry.icon {
                    argv.push("--icon".to_owned());
                    argv.push(icon.clone());
                }
            }
            _ => argv.push(expand_codes(&arg, &entry.name, file)),
        }
    }
    if argv.is_empty() {
        return Err(ExecError::Empty);
    }
    if entry.terminal {
        let terminal = env
            .var("TERMINAL")
            .unwrap_or("x-terminal-emulator")
            .to_owned();
        argv.splice(0..0, ["-e".to_owned()]);
        return Ok((terminal, argv));
    }
    let program = argv.remove(0);
    Ok((program, argv))
}

/// The desktop names of the session (`XDG_CURRENT_DESKTOP`, colon-separated).
pub fn current_desktops(env: &Env) -> Vec<String> {
    env.var("XDG_CURRENT_DESKTOP")
        .map(|v| {
            v.split(':')
                .filter(|d| !d.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Whether `program` is an absolute executable or found on `PATH`.
pub fn program_exists(program: &str, env: &Env) -> bool {
    let path = Path::new(program);
    if path.is_absolute() {
        return is_executable(path);
    }
    env.search_path()
        .iter()
        .any(|dir| is_executable(&dir.join(program)))
}

/// Whether the entry should be listed in this session: an `Application` that is not
/// `Hidden`/`NoDisplay`, allowed by `OnlyShowIn`/`NotShowIn` for `desktops`, and whose
/// `TryExec` program exists.
pub fn is_listed(entry: &DesktopEntry, desktops: &[String], env: &Env) -> bool {
    let shown_here = |list: &[String]| {
        list.iter()
            .any(|d| desktops.iter().any(|c| c.eq_ignore_ascii_case(d)))
    };
    entry.kind == "Application"
        && !entry.hidden
        && !entry.no_display
        && (entry.only_show_in.is_empty() || shown_here(&entry.only_show_in))
        && !shown_here(&entry.not_show_in)
        && entry
            .try_exec
            .as_deref()
            .is_none_or(|p| program_exists(p, env))
}

fn collect_files(
    dir: &Path,
    root: &Path,
    out: &mut Vec<(String, PathBuf)>,
    warnings: &mut Vec<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            if dir != root || error.kind() != std::io::ErrorKind::NotFound {
                warnings.push(format!("{}: {error}", dir.display()));
            }
            return;
        }
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_files(&path, root, out, warnings);
        } else if path.extension().is_some_and(|e| e == "desktop") {
            let id = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("-");
            out.push((id, path));
        }
    }
}

/// Turns a listed entry into an item.
///
/// # Errors
/// When its `Exec` cannot be split (see [`command`]).
pub fn to_item(
    id: &str,
    entry: &DesktopEntry,
    file: &Path,
    env: &Env,
    icons: &mut IconResolver,
) -> Result<Item, ExecError> {
    let (program, args) = command(entry, file, env)?;
    let mut keywords: Vec<String> = entry.generic_name.iter().cloned().collect();
    keywords.extend(entry.keywords.iter().cloned());
    if let Some(binary) = Path::new(&program)
        .file_name()
        .map(|b| b.to_string_lossy().into_owned())
        && !binary.eq_ignore_ascii_case(&entry.name)
    {
        keywords.push(binary);
    }
    let mut item = Item::new(
        ItemId::new(NAME, id),
        entry.name.clone(),
        Action::Run {
            program,
            args,
            confirm: false,
        },
    )
    .with_keywords(keywords);
    if let Some(subtitle) = entry.generic_name.as_ref().or(entry.comment.as_ref()) {
        item = item.with_subtitle(subtitle.clone());
    }
    if let Some(icon) = &entry.icon {
        item = item.with_icon(
            icons
                .lookup(icon)
                .map_or_else(|| icon.clone(), |p| p.to_string_lossy().into_owned()),
        );
    }
    Ok(item)
}

/// Scans `dirs` (earlier ones win for the same desktop-file id) and returns the listed
/// applications sorted by name, plus warnings for unreadable or broken files.
pub fn scan(dirs: &[PathBuf], env: &Env, icons: &mut IconResolver) -> (Vec<Item>, Vec<String>) {
    let locales = locales(env);
    let desktops = current_desktops(env);
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for dir in dirs {
        let mut files = Vec::new();
        collect_files(dir, dir, &mut files, &mut warnings);
        for (id, path) in files {
            // A higher-precedence directory's file shadows this one even when hidden.
            if !seen.insert(id.clone()) {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(error) => {
                    warnings.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            let Some(entry) = parse(&text, &locales) else {
                warnings.push(format!(
                    "{}: no [Desktop Entry] with a Name",
                    path.display()
                ));
                continue;
            };
            if !is_listed(&entry, &desktops, env) {
                continue;
            }
            match to_item(&id, &entry, &path, env, icons) {
                Ok(item) => items.push(item),
                Err(error) => warnings.push(format!("{}: {error}", path.display())),
            }
        }
    }
    items.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    (items, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str) -> DesktopEntry {
        parse(text, &[]).unwrap()
    }

    fn env() -> Env {
        Env::new("linux", [("HOME", "/h")])
    }

    fn argv(exec: &str) -> Result<(String, Vec<String>), ExecError> {
        let e = DesktopEntry {
            name: "Foo Bar".into(),
            exec: Some(exec.into()),
            icon: Some("foo".into()),
            ..DesktopEntry::default()
        };
        command(&e, Path::new("/apps/foo.desktop"), &env())
    }

    #[test]
    fn localised_keys_pick_the_best_locale() {
        let text = "[Desktop Entry]\nName=Files\nName[de]=Dateien\nName[de_AT]=Dateien AT\n";
        let env = Env::new("linux", [("LANG", "de_DE.UTF-8")]);
        assert_eq!(locales(&env), vec!["de_DE", "de"]);
        assert_eq!(parse(text, &locales(&env)).unwrap().name, "Dateien");
        assert_eq!(parse(text, &["fr".into()]).unwrap().name, "Files");
        let at = Env::new("linux", [("LC_ALL", "de_AT@euro"), ("LANG", "en_US")]);
        assert_eq!(locales(&at), vec!["de_AT@euro", "de_AT", "de@euro", "de"]);
        assert_eq!(locales(&Env::new("linux", [("LANG", "C.UTF-8")])).len(), 0);
    }

    #[test]
    fn only_the_desktop_entry_group_counts() {
        let text =
            "[Desktop Action new]\nName=New Window\n[Desktop Entry]\nType=Application\nName=App\n";
        assert_eq!(entry(text).name, "App");
        assert!(parse("Name=Loose\n", &[]).is_none());
        assert!(parse("[Desktop Entry]\nType=Application\n", &[]).is_none());
    }

    #[test]
    fn lists_and_escapes() {
        let e = entry(
            "[Desktop Entry]\nName=A\\sB\nKeywords=web;internet\\;www;  ;\nOnlyShowIn=GNOME;\n",
        );
        assert_eq!(e.name, "A B");
        assert_eq!(e.keywords, vec!["web", "internet;www"]);
        assert_eq!(e.only_show_in, vec!["GNOME"]);
    }

    #[test]
    fn exec_quoting_and_field_codes() {
        assert_eq!(argv("foo %U").unwrap(), ("foo".into(), vec![]));
        assert_eq!(
            argv(r#""/opt/My App/run" --name="%c" "a \"q\" \$x" 100%%"#).unwrap(),
            (
                "/opt/My App/run".into(),
                vec!["--name=Foo Bar".into(), "a \"q\" $x".into(), "100%".into()]
            )
        );
        assert_eq!(
            argv("foo %i %k").unwrap().1,
            vec!["--icon", "foo", "/apps/foo.desktop"]
        );
        assert_eq!(argv("foo --file=%f").unwrap().1, vec!["--file="]);
        assert_eq!(argv("\"unterminated"), Err(ExecError::UnterminatedQuote));
        assert_eq!(argv("%F"), Err(ExecError::Empty));
    }

    #[test]
    fn terminal_entries_run_in_a_terminal() {
        let e = DesktopEntry {
            name: "Top".into(),
            exec: Some("htop".into()),
            terminal: true,
            ..DesktopEntry::default()
        };
        let env = Env::new("linux", [("TERMINAL", "alacritty")]);
        assert_eq!(
            command(&e, Path::new("x"), &env).unwrap(),
            ("alacritty".into(), vec!["-e".into(), "htop".into()])
        );
    }

    #[test]
    fn visibility_rules() {
        let base = entry("[Desktop Entry]\nType=Application\nName=A\nExec=a\n");
        let gnome = vec!["ubuntu".to_owned(), "GNOME".to_owned()];
        assert!(is_listed(&base, &gnome, &env()));
        let only_kde = DesktopEntry {
            only_show_in: vec!["KDE".into()],
            ..base.clone()
        };
        assert!(!is_listed(&only_kde, &gnome, &env()));
        assert!(is_listed(&only_kde, &["kde".into()], &env()));
        let not_gnome = DesktopEntry {
            not_show_in: vec!["GNOME".into()],
            ..base.clone()
        };
        assert!(!is_listed(&not_gnome, &gnome, &env()));
        for hidden in [
            DesktopEntry {
                no_display: true,
                ..base.clone()
            },
            DesktopEntry {
                hidden: true,
                ..base.clone()
            },
            DesktopEntry {
                kind: "Link".into(),
                ..base.clone()
            },
            DesktopEntry {
                try_exec: Some("/nonexistent/quickhop-test".into()),
                ..base.clone()
            },
        ] {
            assert!(!is_listed(&hidden, &gnome, &env()), "{hidden:?}");
        }
    }

    #[test]
    fn default_dirs_follow_xdg() {
        assert_eq!(
            default_dirs(&env()),
            vec![
                PathBuf::from("/h/.local/share/applications"),
                PathBuf::from("/usr/local/share/applications"),
                PathBuf::from("/usr/share/applications"),
            ]
        );
        let custom = Env::new(
            "linux",
            [("XDG_DATA_HOME", "/d"), ("XDG_DATA_DIRS", "/s1::/s2")],
        );
        assert_eq!(
            default_dirs(&custom),
            vec![
                PathBuf::from("/d/applications"),
                PathBuf::from("/s1/applications"),
                PathBuf::from("/s2/applications"),
            ]
        );
    }
}
