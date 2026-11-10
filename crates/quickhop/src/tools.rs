//! The helper programs quickhop runs on Linux and the BSDs (the clipboard tools, `xdg-open`,
//! the power commands): which ones this graphical session needs, and how to install a missing
//! one with this distribution's package manager, named from `/etc/os-release`.

use std::path::Path;

/// The graphical session the clipboard is reached through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Display {
    /// `WAYLAND_DISPLAY` is set: `wl-copy` and `wl-paste`, even when `DISPLAY` is also set for
    /// `XWayland`.
    Wayland,
    /// Only `DISPLAY` is set: `xclip` and the X11 protocol.
    X11,
    /// Neither is set (a console or SSH login).
    None,
}

impl Display {
    /// The session `var` describes.
    pub(crate) fn from_vars(var: impl Fn(&str) -> Option<String>) -> Self {
        let set = |name: &str| var(name).is_some_and(|v| !v.is_empty());
        if set("WAYLAND_DISPLAY") {
            Self::Wayland
        } else if set("DISPLAY") {
            Self::X11
        } else {
            Self::None
        }
    }

    /// The session as `doctor` names it.
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Self::Wayland => "wayland",
            Self::X11 => "x11",
            Self::None => "none (no WAYLAND_DISPLAY or DISPLAY)",
        }
    }
}

/// What a Linux or BSD helper is needed for in a `display` session; `None` when it is not used
/// there.
pub(crate) fn purpose(tool: &str, display: Display) -> Option<&'static str> {
    match (tool, display) {
        ("xdg-open", _) => Some("needed to open files and links"),
        ("wl-copy", Display::Wayland) => Some(
            "needed to copy in this Wayland session; xclip is not used here, even with DISPLAY \
             set for XWayland",
        ),
        ("wl-paste", Display::Wayland) => {
            Some("needed for clipboard history in this Wayland session")
        }
        ("xclip", Display::X11) => Some("needed to copy on X11"),
        ("loginctl" | "systemctl", _) => Some("needed for the lock, sleep and power commands"),
        _ => None,
    }
}

/// A distribution family, by the package manager that installs helpers there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Distro {
    /// Debian, Ubuntu and their derivatives: `apt`.
    Debian,
    /// Fedora, RHEL and their rebuilds: `dnf`.
    Fedora,
    /// Arch and its derivatives: `pacman`.
    Arch,
    /// openSUSE and SLES: `zypper`.
    Suse,
    /// Alpine: `apk`.
    Alpine,
    /// Void: `xbps-install`.
    Void,
}

impl Distro {
    /// The families named when the distribution is unknown.
    const COMMON: [Self; 3] = [Self::Debian, Self::Fedora, Self::Arch];

    /// The family of one `ID` or `ID_LIKE` word of os-release.
    fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "debian" | "ubuntu" | "linuxmint" | "pop" | "elementary" | "raspbian" | "kali"
            | "zorin" | "neon" => Self::Debian,
            "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "nobara" | "ol" => Self::Fedora,
            "arch" | "manjaro" | "endeavouros" | "garuda" | "cachyos" | "artix" => Self::Arch,
            "suse" | "opensuse" | "sles" | "opensuse-leap" | "opensuse-tumbleweed" => Self::Suse,
            "alpine" => Self::Alpine,
            "void" => Self::Void,
            _ => return None,
        })
    }

    /// The command that installs `package`.
    fn install(self, package: &str) -> String {
        match self {
            Self::Debian => format!("sudo apt install {package}"),
            Self::Fedora => format!("sudo dnf install {package}"),
            Self::Arch => format!("sudo pacman -S {package}"),
            Self::Suse => format!("sudo zypper install {package}"),
            Self::Alpine => format!("sudo apk add {package}"),
            Self::Void => format!("sudo xbps-install {package}"),
        }
    }
}

/// The family of the distribution an os-release file `text` describes: its `ID`, else the
/// first known word of `ID_LIKE`.
pub(crate) fn distro(text: &str) -> Option<Distro> {
    let mut id = None;
    let mut like = "";
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        match key.trim() {
            "ID" => id = Some(value),
            "ID_LIKE" => like = value,
            _ => {}
        }
    }
    id.into_iter()
        .chain(like.split_whitespace())
        .find_map(Distro::from_id)
}

/// The package that provides `tool` (the same name on every family listed).
fn package(tool: &str) -> &str {
    match tool {
        "wl-copy" | "wl-paste" => "wl-clipboard",
        "xdg-open" => "xdg-utils",
        "loginctl" | "systemctl" => "systemd",
        other => other,
    }
}

/// How to install `tool` on the distribution os-release `text` describes: its one command,
/// else those of the common families.
pub(crate) fn install_hint(tool: &str, os_release: Option<&str>) -> String {
    let package = package(tool);
    match os_release.and_then(distro) {
        Some(distro) => distro.install(package),
        None => Distro::COMMON
            .iter()
            .map(|distro| distro.install(package))
            .collect::<Vec<_>>()
            .join(" / "),
    }
}

/// This system's os-release file, when it has one.
pub(crate) fn os_release() -> Option<String> {
    ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|path| std::fs::read_to_string(Path::new(path)).ok())
}

/// The message for a helper `program` that could not be started (`error`): what it is for in
/// this session and, on Linux and the BSDs, the command that installs it. macOS needs no
/// helpers.
#[cfg(any(test, not(target_os = "macos")))]
pub(crate) fn cannot_start(program: &str, purpose: &str, error: &std::io::Error) -> String {
    let mut text = format!("cannot start {program} ({purpose}): {error}");
    if cfg!(all(unix, not(target_os = "macos"))) {
        text.push_str("; install it: ");
        text.push_str(&install_hint(program, os_release().as_deref()));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn wayland_wins_over_xwaylands_display() {
        assert_eq!(
            Display::from_vars(vars(&[("WAYLAND_DISPLAY", "wayland-1"), ("DISPLAY", ":0")])),
            Display::Wayland
        );
        assert_eq!(Display::from_vars(vars(&[("DISPLAY", ":0")])), Display::X11);
        assert_eq!(
            Display::from_vars(vars(&[("WAYLAND_DISPLAY", ""), ("DISPLAY", "")])),
            Display::None
        );
    }

    #[test]
    fn only_this_sessions_clipboard_tool_is_needed() {
        assert_eq!(
            purpose("xclip", Display::X11),
            Some("needed to copy on X11")
        );
        assert_eq!(purpose("xclip", Display::Wayland), None);
        assert_eq!(purpose("wl-copy", Display::X11), None);
        assert!(
            purpose("wl-copy", Display::Wayland).is_some_and(|p| p.contains("XWayland")),
            "the XWayland hint"
        );
        assert_eq!(purpose("wl-paste", Display::None), None);
        assert!(purpose("xdg-open", Display::None).is_some());
    }

    #[test]
    fn one_install_command_per_distribution_id() {
        let cases = [
            ("ID=debian\n", "sudo apt install xclip"),
            ("ID=ubuntu\nID_LIKE=debian\n", "sudo apt install xclip"),
            (
                "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n",
                "sudo apt install xclip",
            ),
            ("ID=fedora\n", "sudo dnf install xclip"),
            (
                "ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n",
                "sudo dnf install xclip",
            ),
            ("ID=arch\n", "sudo pacman -S xclip"),
            ("ID=endeavouros\nID_LIKE=arch\n", "sudo pacman -S xclip"),
            (
                "ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"\n",
                "sudo zypper install xclip",
            ),
            ("ID=alpine\n", "sudo apk add xclip"),
            ("ID=void\n", "sudo xbps-install xclip"),
            // An unknown ID falls back to its ID_LIKE.
            ("ID=mydistro\nID_LIKE=ubuntu\n", "sudo apt install xclip"),
        ];
        for (os_release, command) in cases {
            assert_eq!(
                install_hint("xclip", Some(os_release)),
                command,
                "{os_release}"
            );
        }
    }

    #[test]
    fn unknown_distributions_get_the_common_commands() {
        let all = "sudo apt install wl-clipboard / sudo dnf install wl-clipboard / \
                   sudo pacman -S wl-clipboard";
        assert_eq!(install_hint("wl-paste", None), all);
        assert_eq!(install_hint("wl-copy", Some("ID=nixos\n")), all);
        assert_eq!(
            install_hint("xdg-open", Some("ID=debian")),
            "sudo apt install xdg-utils"
        );
    }

    #[test]
    fn a_helper_that_cannot_start_says_what_for() {
        let error = std::io::Error::from(std::io::ErrorKind::NotFound);
        let text = cannot_start("xclip", "it copies text on X11", &error);
        assert!(
            text.starts_with("cannot start xclip (it copies text on X11): "),
            "{text}"
        );
        if cfg!(all(unix, not(target_os = "macos"))) {
            assert!(text.contains("; install it: sudo "), "{text}");
        } else {
            assert!(!text.contains("install it"), "{text}");
        }
    }
}
