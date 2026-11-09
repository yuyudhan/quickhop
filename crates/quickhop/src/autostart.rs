//! `quickhop autostart on|off|status`: start the daemon at login, through an XDG autostart
//! entry on Linux and the BSDs (honoured by GNOME, KDE, Xfce, Cinnamon, MATE, and turned into
//! a systemd user unit by `systemd-xdg-autostart-generator`) or a `LaunchAgent` on macOS.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The `LaunchAgent` label, also the reverse-DNS id of the app.
pub(crate) const LABEL: &str = "io.github.yuyudhan.quickhop";

/// What `quickhop autostart` does.
#[derive(Debug, Clone, Copy, clap::Subcommand)]
pub(crate) enum AutostartAction {
    /// Start the daemon at every login, from this quickhop binary.
    On,
    /// Stop starting it at login.
    Off,
    /// Print whether it starts at login, and which binary.
    Status,
}

/// Why autostart could not be changed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AutostartError {
    #[error("launch at login is not supported on {0} yet")]
    Unsupported(String),
    #[error("cannot find the home folder: HOME is unset")]
    NoHome,
    #[error("{path}: {source}")]
    File {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot find this program's path: {0}")]
    Exe(io::Error),
}

/// Where the login entry lives on `os`.
///
/// # Errors
/// On Windows (not supported yet) or without `HOME` / `XDG_CONFIG_HOME`.
pub(crate) fn entry_path(
    os: &str,
    var: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, AutostartError> {
    let get = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    match os {
        "windows" => Err(AutostartError::Unsupported(os.to_owned())),
        "macos" => get("HOME")
            .map(|home| {
                home.join("Library/LaunchAgents")
                    .join(format!("{LABEL}.plist"))
            })
            .ok_or(AutostartError::NoHome),
        _ => get("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| get("HOME").map(|home| home.join(".config")))
            .map(|config| config.join("autostart").join("quickhop.desktop"))
            .ok_or(AutostartError::NoHome),
    }
}

/// `arg` quoted for a desktop entry's `Exec` key: in double quotes when it holds anything
/// but safe characters, with `"`, `` ` ``, `$` and `\` escaped, and `%` doubled.
fn exec_quote(arg: &str) -> String {
    // `%` is not reserved; doubling it is its own escape.
    let safe = |c: char| c.is_ascii_alphanumeric() || "/._-+,:@=%".contains(c);
    let doubled = arg.replace('%', "%%");
    if !doubled.is_empty() && doubled.chars().all(safe) {
        return doubled;
    }
    let mut quoted = String::from("\"");
    for c in doubled.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    // The desktop-entry string type escapes backslashes once more.
    quoted.replace('\\', "\\\\")
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The login entry that runs `exe daemon` on `os`.
pub(crate) fn entry_text(os: &str, exe: &Path) -> String {
    let exe = exe.to_string_lossy();
    if os == "macos" {
        return format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">
<plist version=\"1.0\">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
",
            xml_escape(&exe)
        );
    }
    format!(
        "[Desktop Entry]
Type=Application
Name=quickhop
Comment=Keyboard launcher: clipboard history and file index in the background
Exec={} daemon
Icon=quickhop
Terminal=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
",
        exec_quote(&exe)
    )
}

/// Runs `action` for the running binary.
///
/// # Errors
/// When the entry cannot be written or removed, or the OS is not supported yet.
pub(crate) fn run(action: AutostartAction, out: &mut impl Write) -> Result<(), AutostartError> {
    let os = std::env::consts::OS;
    let path = entry_path(os, |name| std::env::var(name).ok())?;
    let file_error = |source| AutostartError::File {
        path: path.clone(),
        source,
    };
    match action {
        AutostartAction::On => {
            let exe = std::env::current_exe().map_err(AutostartError::Exe)?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(file_error)?;
            }
            std::fs::write(&path, entry_text(os, &exe)).map_err(file_error)?;
            let _ = writeln!(
                out,
                "quickhop daemon starts at login: {} runs {} daemon",
                path.display(),
                exe.display()
            );
        }
        AutostartAction::Off => match std::fs::remove_file(&path) {
            Ok(()) => {
                let _ = writeln!(out, "removed {}", path.display());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let _ = writeln!(out, "already off ({} does not exist)", path.display());
            }
            Err(error) => return Err(file_error(error)),
        },
        AutostartAction::Status => {
            let _ = if path.is_file() {
                writeln!(out, "on: {}", path.display())
            } else {
                writeln!(out, "off: {} does not exist", path.display())
            };
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    /// `name` under an absolute root of the OS the tests run on: on Windows only a drive
    /// makes a path absolute.
    fn abs(name: &str) -> String {
        let root = if cfg!(windows) { r"C:\" } else { "/" };
        format!("{root}{name}")
    }

    #[test]
    fn entry_paths_per_os() {
        let (c, h) = (abs("c"), abs("h"));
        assert_eq!(
            entry_path("linux", vars(&[("XDG_CONFIG_HOME", &c), ("HOME", &h)])).unwrap(),
            PathBuf::from(&c).join("autostart").join("quickhop.desktop")
        );
        assert_eq!(
            entry_path("freebsd", vars(&[("XDG_CONFIG_HOME", "rel"), ("HOME", &h)])).unwrap(),
            PathBuf::from(&h)
                .join(".config")
                .join("autostart")
                .join("quickhop.desktop")
        );
        let user = abs("Users/a");
        assert_eq!(
            entry_path("macos", vars(&[("HOME", &user)])).unwrap(),
            PathBuf::from(&user).join("Library/LaunchAgents/io.github.yuyudhan.quickhop.plist")
        );
        assert!(matches!(
            entry_path("linux", vars(&[])),
            Err(AutostartError::NoHome)
        ));
        assert!(matches!(
            entry_path("windows", vars(&[("HOME", &h)])),
            Err(AutostartError::Unsupported(_))
        ));
    }

    #[test]
    fn exec_arguments_are_quoted_per_the_desktop_entry_spec() {
        assert_eq!(exec_quote("/usr/bin/quickhop"), "/usr/bin/quickhop");
        assert_eq!(
            exec_quote("/opt/my apps/quickhop"),
            "\"/opt/my apps/quickhop\""
        );
        assert_eq!(exec_quote("/x/100%/q"), "/x/100%%/q");
        assert_eq!(
            exec_quote("/a\"b$c`d\\e"),
            "\"/a\\\\\"b\\\\$c\\\\`d\\\\\\\\e\""
        );
    }

    #[test]
    fn entries_run_the_daemon() {
        let desktop = entry_text("linux", Path::new("/home/a b/.cargo/bin/quickhop"));
        assert!(desktop.starts_with("[Desktop Entry]\nType=Application\n"));
        assert!(desktop.contains("\nExec=\"/home/a b/.cargo/bin/quickhop\" daemon\n"));
        let plist = entry_text("macos", Path::new("/Applications/Q&A/quickhop"));
        assert!(plist.contains(
            "<string>/Applications/Q&amp;A/quickhop</string>\n        <string>daemon</string>"
        ));
        assert!(plist.contains("<string>io.github.yuyudhan.quickhop</string>"));
    }
}
