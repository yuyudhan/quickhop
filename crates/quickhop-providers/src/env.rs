//! The process environment, captured once so providers can be tested with a fake one.

use std::collections::HashMap;
use std::path::PathBuf;

use quickhop_core::Flavour;

/// OS name, build flavour and environment variables a provider reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    /// A [`std::env::consts::OS`] value: `linux`, `macos`, `windows`, ...
    pub os: String,
    /// Whose directories the providers read: this build's ([`Flavour::CURRENT`]) unless a
    /// test picks one.
    pub flavour: Flavour,
    vars: HashMap<String, String>,
}

impl Env {
    /// The running process's OS and variables (non-UTF-8 values are skipped).
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS.into(),
            flavour: Flavour::CURRENT,
            vars: std::env::vars_os()
                .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
                .collect(),
        }
    }

    /// A fake environment for `os` with exactly `vars`, for this build's flavour.
    pub fn new<K: Into<String>, V: Into<String>>(
        os: &str,
        vars: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        Self {
            os: os.into(),
            flavour: Flavour::CURRENT,
            vars: vars
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    /// A variable's value, `None` when unset or empty.
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// A variable as a path.
    pub fn path(&self, name: &str) -> Option<PathBuf> {
        self.var(name).map(PathBuf::from)
    }

    /// The user's home directory: `HOME`, or `USERPROFILE` on Windows.
    pub fn home(&self) -> Option<PathBuf> {
        self.path("HOME").or_else(|| self.path("USERPROFILE"))
    }

    /// Directories listed in `PATH`.
    pub fn search_path(&self) -> Vec<PathBuf> {
        self.var("PATH")
            .map(|p| std::env::split_paths(p).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_values_count_as_unset() {
        let env = Env::new("linux", [("A", ""), ("HOME", "/h")]);
        assert_eq!(env.var("A"), None);
        assert_eq!(env.home(), Some(PathBuf::from("/h")));
        assert_eq!(env.search_path().len(), 0);
    }

    #[test]
    fn windows_home_falls_back_to_userprofile() {
        let env = Env::new("windows", [("USERPROFILE", "C:/u")]);
        assert_eq!(env.home(), Some(PathBuf::from("C:/u")));
    }
}
