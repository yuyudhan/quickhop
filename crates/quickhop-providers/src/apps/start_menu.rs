//! Windows Start Menu shortcuts (`.lnk`) and packaged (UWP / MSIX) apps.
//!
//! Shortcuts are opened through the shell, so their binary format is never parsed: the
//! file name is the title. Packaged apps have no shortcut file; they are listed by
//! PowerShell's `Get-StartApps` (no COM or `unsafe` needed) and opened as
//! `shell:AppsFolder\<AppUserModelID>`.

use std::path::{Path, PathBuf};
use std::process::Command;

use quickhop_core::{Action, Item, ItemId};
use serde::Deserialize;

use super::NAME;
use crate::env::Env;

/// The per-user and all-users `Start Menu\Programs` folders.
pub fn default_dirs(env: &Env) -> Vec<PathBuf> {
    let programs = Path::new("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");
    ["APPDATA", "ProgramData"]
        .iter()
        .filter_map(|v| env.path(v))
        .map(|d| d.join(&programs))
        .collect()
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<PathBuf>, warnings: &mut Vec<String>) {
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
            collect(&path, root, out, warnings);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
        {
            out.push(path);
        }
    }
}

/// Every shortcut under `dirs` except uninstallers, first title winning, sorted by name.
/// The subtitle is the Start Menu folder.
pub fn scan(dirs: &[PathBuf]) -> (Vec<Item>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in dirs {
        let mut paths = Vec::new();
        collect(dir, dir, &mut paths, &mut warnings);
        for path in paths {
            let Some(title) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            if title.to_lowercase().contains("uninstall") || !seen.insert(title.to_lowercase()) {
                continue;
            }
            let folder = path
                .parent()
                .and_then(|p| p.strip_prefix(dir).ok())
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .filter(|f| !f.is_empty());
            let mut item = Item::new(
                ItemId::new(NAME, format!("lnk:{}", title.to_lowercase())),
                title,
                Action::Open {
                    target: path.to_string_lossy().into_owned(),
                },
            );
            if let Some(folder) = folder {
                item = item.with_subtitle(folder);
            }
            items.push(item);
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

/// One row of `Get-StartApps`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StartApp {
    /// Display name.
    #[serde(rename = "Name")]
    pub name: String,
    /// `AppUserModelID`; packaged apps contain `!`.
    #[serde(rename = "AppID")]
    pub app_id: String,
}

/// Why packaged apps could not be listed.
#[derive(Debug, thiserror::Error)]
pub enum PackagedError {
    /// PowerShell could not be started.
    #[error("cannot run powershell: {0}")]
    Spawn(#[from] std::io::Error),
    /// PowerShell failed.
    #[error("Get-StartApps failed: {0}")]
    Failed(String),
    /// The output was not the expected JSON.
    #[error("unexpected Get-StartApps output: {0}")]
    Json(#[from] serde_json::Error),
}

/// Parses `Get-StartApps | ConvertTo-Json`: an array, or a single object when there is
/// one app, or nothing when there are none.
///
/// # Errors
/// When the text is not that JSON.
pub fn parse_start_apps(json: &str) -> Result<Vec<StartApp>, serde_json::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        Many(Vec<StartApp>),
        One(StartApp),
    }
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(match serde_json::from_str(json)? {
        OneOrMany::Many(apps) => apps,
        OneOrMany::One(app) => vec![app],
    })
}

/// Runs `Get-StartApps` through PowerShell.
///
/// # Errors
/// When PowerShell cannot run or prints something unexpected.
pub fn packaged_apps() -> Result<Vec<StartApp>, PackagedError> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-StartApps | ConvertTo-Json -Compress",
        ])
        .output()?;
    if !output.status.success() {
        return Err(PackagedError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(parse_start_apps(&String::from_utf8_lossy(&output.stdout))?)
}

/// Adds the packaged apps (ids with `!`) whose names no shortcut already uses, keeping
/// `items` sorted by name.
pub fn merge_packaged(items: &mut Vec<Item>, apps: Vec<StartApp>) {
    let mut names: std::collections::HashSet<String> =
        items.iter().map(|i| i.title.to_lowercase()).collect();
    for app in apps {
        if !app.app_id.contains('!') || !names.insert(app.name.to_lowercase()) {
            continue;
        }
        items.push(
            Item::new(
                ItemId::new(NAME, format!("uwp:{}", app.app_id)),
                app.name,
                Action::Open {
                    target: format!("shell:AppsFolder\\{}", app.app_id),
                },
            )
            .with_subtitle("App"),
        );
    }
    items.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_apps_json_shapes() {
        assert_eq!(parse_start_apps("").unwrap().len(), 0);
        let one = parse_start_apps(
            r#"{"Name":"Calculator","AppID":"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"}"#,
        )
        .unwrap();
        assert_eq!(one[0].name, "Calculator");
        let many =
            parse_start_apps(r#"[{"Name":"A","AppID":"a!x"},{"Name":"B","AppID":"C:\\b.exe"}]"#)
                .unwrap();
        assert_eq!(many.len(), 2);
        assert!(parse_start_apps("not json").is_err());
    }

    #[test]
    fn merge_keeps_only_new_packaged_apps() {
        let mut items = vec![Item::new(
            ItemId::new(NAME, "lnk:calculator"),
            "Calculator",
            Action::Open {
                target: "c.lnk".into(),
            },
        )];
        let apps = parse_start_apps(
            r#"[{"Name":"Calculator","AppID":"calc!App"},{"Name":"Photos","AppID":"photos!App"},{"Name":"Tool","AppID":"C:\\t.exe"}]"#,
        )
        .unwrap();
        merge_packaged(&mut items, apps);
        let titles: Vec<_> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, vec!["Calculator", "Photos"]);
        assert_eq!(
            items[1].default_action(),
            Some(&Action::Open {
                target: "shell:AppsFolder\\photos!App".into()
            })
        );
    }

    #[test]
    fn default_dirs_use_appdata_and_programdata() {
        let env = Env::new("windows", [("APPDATA", "C:/u"), ("ProgramData", "C:/p")]);
        let dirs = default_dirs(&env);
        assert_eq!(dirs.len(), 2);
        assert!(dirs[0].starts_with("C:/u") && dirs[0].ends_with("Programs"));
    }
}
