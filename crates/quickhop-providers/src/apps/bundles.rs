//! macOS application bundles (`*.app`), described by `Contents/Info.plist`.

use std::path::{Path, PathBuf};

use quickhop_core::{Action, Item, ItemId};

use super::NAME;
use crate::env::Env;

/// How deep [`scan`] descends into plain folders (`/Applications/Utilities` is depth 1).
const MAX_DEPTH: usize = 2;

/// `/Applications`, `/System/Applications` and `~/Applications`.
pub fn default_dirs(env: &Env) -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = env.home() {
        dirs.push(home.join("Applications"));
    }
    dirs
}

/// What the launcher needs from one bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// The `.app` directory.
    pub path: PathBuf,
    /// `CFBundleDisplayName`, else `CFBundleName`, else the folder name.
    pub name: String,
    /// `CFBundleIdentifier`.
    pub identifier: Option<String>,
    /// `CFBundleExecutable`.
    pub executable: Option<String>,
    /// The icon file in `Contents/Resources`, when it exists.
    pub icon: Option<PathBuf>,
    /// `LSBackgroundOnly` / `LSUIElement`: agents without a Dock icon.
    pub background: bool,
}

/// Reads `path`'s `Info.plist` (XML or binary). A bundle without a readable plist keeps
/// its folder name.
pub fn read_bundle(path: &Path) -> Bundle {
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let info = plist::Value::from_file(path.join("Contents/Info.plist")).ok();
    let dict = info.as_ref().and_then(plist::Value::as_dictionary);
    let string = |key: &str| {
        dict.and_then(|d| d.get(key))
            .and_then(plist::Value::as_string)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let flag = |key: &str| {
        dict.and_then(|d| d.get(key)).is_some_and(|v| {
            v.as_boolean().unwrap_or(false)
                || v.as_string()
                    .is_some_and(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        })
    };
    let icon = string("CFBundleIconFile").and_then(|file| {
        let resources = path.join("Contents/Resources");
        [
            resources.join(&file),
            resources.join(format!("{file}.icns")),
        ]
        .into_iter()
        .find(|p| p.is_file())
    });
    Bundle {
        path: path.to_path_buf(),
        name: string("CFBundleDisplayName")
            .or_else(|| string("CFBundleName"))
            .unwrap_or(stem),
        identifier: string("CFBundleIdentifier"),
        executable: string("CFBundleExecutable"),
        icon,
        background: flag("LSBackgroundOnly") || flag("LSUIElement"),
    }
}

fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>, warnings: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            if depth > 0 || error.kind() != std::io::ErrorKind::NotFound {
                warnings.push(format!("{}: {error}", dir.display()));
            }
            return;
        }
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths.into_iter().filter(|p| p.is_dir()) {
        if path.extension().is_some_and(|e| e == "app") {
            out.push(path);
        } else if depth < MAX_DEPTH {
            collect(&path, depth + 1, out, warnings);
        }
    }
}

/// Turns a bundle into an item that opens it.
pub fn to_item(bundle: &Bundle) -> Item {
    let key = bundle
        .identifier
        .clone()
        .unwrap_or_else(|| bundle.path.to_string_lossy().into_owned());
    let stem = bundle
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned());
    let keywords: Vec<String> = [stem, bundle.executable.clone()]
        .into_iter()
        .flatten()
        .filter(|k| !k.eq_ignore_ascii_case(&bundle.name))
        .collect();
    let mut item = Item::new(
        ItemId::new(NAME, key),
        bundle.name.clone(),
        Action::Open {
            target: bundle.path.to_string_lossy().into_owned(),
        },
    )
    .with_subtitle(bundle.path.to_string_lossy().into_owned())
    .with_keywords(keywords);
    if let Some(icon) = &bundle.icon {
        item = item.with_icon(icon.to_string_lossy().into_owned());
    }
    item
}

/// Every foreground bundle under `dirs`, first occurrence of an identifier winning, sorted
/// by name.
pub fn scan(dirs: &[PathBuf]) -> (Vec<Item>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut paths = Vec::new();
    for dir in dirs {
        collect(dir, 0, &mut paths, &mut warnings);
    }
    let mut seen = std::collections::HashSet::new();
    let mut items: Vec<Item> = paths
        .iter()
        .map(|p| read_bundle(p))
        .filter(|b| !b.background)
        .map(|b| to_item(&b))
        .filter(|item| seen.insert(item.id.clone()))
        .collect();
    items.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    (items, warnings)
}
