//! Which build of quickhop this is: a release (what users install) or a development build.
//!
//! The two run side by side on one machine without touching each other: each has its own
//! configuration, data and runtime directories, daemon socket and lock, macOS bundle
//! identifier and app name (so macOS keeps a separate Accessibility grant, and rebuilding the
//! development app never invalidates the release's), login entry, log and default hotkey.
//! The build decides, never the running program: debug builds (`cargo run`, `cargo build`,
//! the tests) and builds with the `dev` feature (`just bundle-macos`) are development builds,
//! everything else (the release pipeline, `cargo install`) is a release. A release keeps
//! the names quickhop always had.

use std::fmt;

/// A build flavour; [`Flavour::CURRENT`] is this build's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavour {
    /// The released program: `~/.config/quickhop`, `io.github.yuyudhan.quickhop`, quickhop.app.
    Release,
    /// A development build: `~/.config/quickhop-dev`, `io.github.yuyudhan.quickhop.dev`,
    /// "quickhop Dev.app".
    Dev,
}

impl Flavour {
    /// This build's flavour, fixed when it was compiled.
    pub const CURRENT: Self = if cfg!(any(debug_assertions, feature = "dev")) {
        Self::Dev
    } else {
        Self::Release
    };

    /// `release` or `dev`, as `doctor` and `quickhop --version` print it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::Dev => "dev",
        }
    }

    /// The name of this flavour's directories and files: `quickhop` or `quickhop-dev`
    /// (`~/.config/<stem>/`, `~/.local/share/<stem>/`, the runtime directory, the XDG
    /// autostart entry `<stem>.desktop`, the systemd unit `<stem>.service`, the macOS log
    /// `<stem>.log`, the window's app id).
    #[must_use]
    pub const fn stem(self) -> &'static str {
        match self {
            Self::Release => "quickhop",
            Self::Dev => "quickhop-dev",
        }
    }

    /// The macOS bundle identifier, also the `LaunchAgent` label: macOS keys the
    /// Accessibility grant to it.
    #[must_use]
    pub const fn bundle_id(self) -> &'static str {
        match self {
            Self::Release => "io.github.yuyudhan.quickhop",
            Self::Dev => "io.github.yuyudhan.quickhop.dev",
        }
    }

    /// The app's name, as macOS shows it and the bundle is called (`<name>.app`).
    #[must_use]
    pub const fn app_name(self) -> &'static str {
        match self {
            Self::Release => "quickhop",
            Self::Dev => "quickhop Dev",
        }
    }

    /// The launcher hotkey when the config sets none. The two differ: both daemons
    /// register theirs globally, and the second to ask for the same one would not get it.
    #[must_use]
    pub const fn default_hotkey(self) -> &'static str {
        match self {
            Self::Release => "Alt+Space",
            Self::Dev => "Alt+Shift+Space",
        }
    }
}

impl fmt::Display for Flavour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Hotkey;

    const BOTH: [Flavour; 2] = [Flavour::Release, Flavour::Dev];

    #[test]
    fn release_keeps_the_names_quickhop_always_had() {
        let release = Flavour::Release;
        assert_eq!(release.stem(), "quickhop");
        assert_eq!(release.bundle_id(), "io.github.yuyudhan.quickhop");
        assert_eq!(release.app_name(), "quickhop");
        assert_eq!(release.default_hotkey(), "Alt+Space");
    }

    #[test]
    fn dev_differs_from_release_everywhere() {
        let (release, dev) = (Flavour::Release, Flavour::Dev);
        assert_eq!(dev.stem(), "quickhop-dev");
        assert_eq!(dev.bundle_id(), "io.github.yuyudhan.quickhop.dev");
        assert_eq!(dev.app_name(), "quickhop Dev");
        assert_ne!(dev.stem(), release.stem());
        assert_ne!(dev.bundle_id(), release.bundle_id());
        assert_ne!(dev.app_name(), release.app_name());
        let parse = |flavour: Flavour| Hotkey::parse(flavour.default_hotkey()).unwrap();
        assert_ne!(
            parse(dev),
            parse(release),
            "two daemons would fight over one hotkey"
        );
    }

    #[test]
    fn every_default_hotkey_parses_to_its_canonical_spelling() {
        for flavour in BOTH {
            let hotkey = Hotkey::parse(flavour.default_hotkey()).unwrap();
            assert_eq!(hotkey.to_string(), flavour.default_hotkey(), "{flavour}");
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    fn debug_builds_are_a_development_build() {
        // `cargo run`, `cargo build` and `cargo test` build with debug assertions, so they
        // must never be a release: that keeps them off the installed release's files.
        assert_eq!(Flavour::CURRENT, Flavour::Dev);
    }
}
