//! Icon lookup per the freedesktop Icon Theme Specification:
//! <https://specifications.freedesktop.org/icon-theme-spec/latest/>.
//!
//! The configured theme is searched first, then the themes it `Inherits`, then `hicolor`,
//! then the unthemed `pixmaps` fallback. Within a theme, a directory whose size matches
//! exactly wins; otherwise the closest size does.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::env::Env;

/// Icon size the launcher asks for, in pixels.
pub const ICON_SIZE: u32 = 48;

const EXTENSIONS: [&str; 3] = ["png", "svg", "xpm"];

/// Which theme to use and where themes live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconSearch {
    /// Preferred theme name; `hicolor` is always searched after it.
    pub theme: String,
    /// Base directories holding themes, in precedence order.
    pub base_dirs: Vec<PathBuf>,
    /// Directories holding unthemed icons, searched last.
    pub pixmaps: Vec<PathBuf>,
}

impl IconSearch {
    /// The spec's directories for `env`. An empty `theme` means the GTK setting
    /// (`gtk-icon-theme-name` in `gtk-3.0/settings.ini`), else `hicolor`.
    pub fn for_env(theme: &str, env: &Env) -> Self {
        let mut base_dirs = Vec::new();
        if let Some(home) = env.home() {
            base_dirs.push(home.join(".icons"));
        }
        let data = super::desktop::data_dirs(env);
        base_dirs.extend(data.iter().map(|d| d.join("icons")));
        let theme = if theme.is_empty() {
            gtk_theme(env).unwrap_or_default()
        } else {
            theme.to_owned()
        };
        Self {
            theme,
            base_dirs,
            pixmaps: vec![PathBuf::from("/usr/share/pixmaps")],
        }
    }
}

fn gtk_theme(env: &Env) -> Option<String> {
    let config = env
        .path("XDG_CONFIG_HOME")
        .or_else(|| env.home().map(|h| h.join(".config")))?;
    let text = std::fs::read_to_string(config.join("gtk-3.0/settings.ini")).ok()?;
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "gtk-icon-theme-name").then(|| value.trim().trim_matches('"').to_owned())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Fixed,
    Scalable,
    Threshold,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThemeDir {
    path: String,
    size: u32,
    scale: u32,
    min: u32,
    max: u32,
    threshold: u32,
    kind: Kind,
}

impl ThemeDir {
    fn matches(&self, size: u32) -> bool {
        if self.scale != 1 {
            return false;
        }
        match self.kind {
            Kind::Fixed => self.size == size,
            Kind::Scalable => (self.min..=self.max).contains(&size),
            Kind::Threshold => self.size.abs_diff(size) <= self.threshold,
        }
    }

    fn distance(&self, size: u32) -> u32 {
        let (low, high) = match self.kind {
            Kind::Fixed => (self.size, self.size),
            Kind::Scalable => (self.min, self.max),
            Kind::Threshold => (
                self.size.saturating_sub(self.threshold),
                self.size + self.threshold,
            ),
        };
        let (low, high) = (low * self.scale, high * self.scale);
        if size < low {
            low - size
        } else {
            size.saturating_sub(high)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Theme {
    name: String,
    inherits: Vec<String>,
    dirs: Vec<ThemeDir>,
}

/// Parses an `index.theme` file.
fn parse_index(name: &str, text: &str) -> Theme {
    let mut sections: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
    let mut current = "";
    for line in text.lines().map(str::trim) {
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = section;
        } else if let Some((key, value)) = line.split_once('=') {
            sections
                .entry(current)
                .or_default()
                .insert(key.trim(), value.trim());
        }
    }
    let main = sections.get("Icon Theme");
    let list = |key: &str| -> Vec<String> {
        main.and_then(|m| m.get(key))
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut dir_names = list("Directories");
    dir_names.extend(list("ScaledDirectories"));
    let dirs = dir_names
        .into_iter()
        .filter_map(|path| {
            let section = sections.get(path.as_str())?;
            let number = |key: &str| section.get(key).and_then(|v| v.parse::<u32>().ok());
            let size = number("Size")?;
            let kind = match section.get("Type").copied() {
                Some("Fixed") => Kind::Fixed,
                Some("Scalable") => Kind::Scalable,
                _ => Kind::Threshold,
            };
            Some(ThemeDir {
                size,
                scale: number("Scale").unwrap_or(1),
                min: number("MinSize").unwrap_or(size),
                max: number("MaxSize").unwrap_or(size),
                threshold: number("Threshold").unwrap_or(2),
                kind,
                path,
            })
        })
        .collect();
    Theme {
        name: name.to_owned(),
        inherits: list("Inherits"),
        dirs,
    }
}

/// Resolves icon names to files, caching theme indexes and directory listings.
pub struct IconResolver {
    search: IconSearch,
    chain: Vec<Theme>,
    listings: HashMap<PathBuf, HashSet<OsString>>,
}

impl IconResolver {
    /// Loads the theme chain for `search`.
    pub fn new(search: &IconSearch) -> Self {
        let mut resolver = Self {
            search: search.clone(),
            chain: Vec::new(),
            listings: HashMap::new(),
        };
        let mut pending = vec![search.theme.clone()];
        let mut visited = HashSet::new();
        while let Some(name) = pending.pop() {
            if name.is_empty() || name == "hicolor" || !visited.insert(name.clone()) {
                continue;
            }
            if let Some(theme) = resolver.read_theme(&name) {
                pending.extend(theme.inherits.iter().rev().cloned());
                resolver.chain.push(theme);
            }
        }
        if let Some(hicolor) = resolver.read_theme("hicolor") {
            resolver.chain.push(hicolor);
        }
        resolver
    }

    fn read_theme(&self, name: &str) -> Option<Theme> {
        self.search.base_dirs.iter().find_map(|base| {
            let text = std::fs::read_to_string(base.join(name).join("index.theme")).ok()?;
            Some(parse_index(name, &text))
        })
    }

    fn has(&mut self, dir: &Path, file: &str) -> bool {
        self.listings
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                std::fs::read_dir(dir)
                    .map(|entries| {
                        entries
                            .filter_map(|e| e.ok().map(|e| e.file_name()))
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .contains(OsString::from(file).as_os_str())
    }

    fn find(&mut self, dir: &Path, name: &str) -> Option<PathBuf> {
        EXTENSIONS.iter().find_map(|ext| {
            let file = format!("{name}.{ext}");
            self.has(dir, &file).then(|| dir.join(file))
        })
    }

    fn in_theme(&mut self, theme_index: usize, name: &str) -> Option<PathBuf> {
        let theme = self.chain[theme_index].clone();
        let bases = self.search.base_dirs.clone();
        for dir in theme.dirs.iter().filter(|d| d.matches(ICON_SIZE)) {
            for base in &bases {
                if let Some(path) = self.find(&base.join(&theme.name).join(&dir.path), name) {
                    return Some(path);
                }
            }
        }
        let mut best: Option<(u32, PathBuf)> = None;
        for dir in &theme.dirs {
            let distance = dir.distance(ICON_SIZE);
            if best.as_ref().is_some_and(|(d, _)| *d <= distance) {
                continue;
            }
            for base in &bases {
                if let Some(path) = self.find(&base.join(&theme.name).join(&dir.path), name) {
                    best = Some((distance, path));
                    break;
                }
            }
        }
        best.map(|(_, path)| path)
    }

    /// The file for icon `name`: an existing absolute path as is; otherwise the best match
    /// in the theme chain, then in the pixmaps directories.
    pub fn lookup(&mut self, name: &str) -> Option<PathBuf> {
        let path = Path::new(name);
        if path.is_absolute() {
            return path.is_file().then(|| path.to_path_buf());
        }
        // Names must not carry an extension, but many entries do.
        let name = EXTENSIONS
            .iter()
            .find_map(|ext| name.strip_suffix(&format!(".{ext}")))
            .unwrap_or(name);
        for index in 0..self.chain.len() {
            if let Some(found) = self.in_theme(index, name) {
                return Some(found);
            }
        }
        let pixmaps = self.search.pixmaps.clone();
        pixmaps.iter().find_map(|dir| self.find(dir, name))
    }

    /// Names of the themes searched, in order.
    pub fn chain(&self) -> Vec<&str> {
        self.chain.iter().map(|t| t.name.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_directories_and_inherits() {
        let theme = parse_index(
            "t",
            "[Icon Theme]\nInherits=a, b\nDirectories=48x48/apps,scalable/apps,bad\n\
             [48x48/apps]\nSize=48\nType=Fixed\n[scalable/apps]\nSize=64\nMinSize=8\nMaxSize=512\nType=Scalable\n",
        );
        assert_eq!(theme.inherits, vec!["a", "b"]);
        assert_eq!(theme.dirs.len(), 2);
        assert!(theme.dirs[0].matches(48));
        assert!(theme.dirs[1].matches(48));
        assert_eq!(theme.dirs[0].distance(32), 16);
    }

    #[test]
    fn threshold_dirs_match_near_sizes() {
        let dir = ThemeDir {
            path: "x".into(),
            size: 46,
            scale: 1,
            min: 46,
            max: 46,
            threshold: 2,
            kind: Kind::Threshold,
        };
        assert!(dir.matches(48));
        assert!(!dir.matches(49));
        assert_eq!(dir.distance(64), 16);
    }
}
