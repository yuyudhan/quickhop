//! System commands: lock, log out, suspend, reboot, shut down. Each item only describes
//! the OS command ([`Action::Run`] with `confirm: true`); running it is the caller's job
//! and needs an explicit confirmation.

use quickhop_core::{Action, Cancel, Item, ItemId, Provider, ProviderError, Query};

/// Provider name.
pub const NAME: &str = "system";

/// One power or session command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Lock the screen.
    Lock,
    /// End the session.
    Logout,
    /// Suspend to RAM (sleep).
    Suspend,
    /// Restart the machine.
    Reboot,
    /// Power off.
    Shutdown,
}

impl Command {
    /// Every command, in display order.
    pub const ALL: [Self; 5] = [
        Self::Lock,
        Self::Logout,
        Self::Suspend,
        Self::Reboot,
        Self::Shutdown,
    ];

    /// Stable key used in the item id.
    pub fn key(self) -> &'static str {
        match self {
            Self::Lock => "lock",
            Self::Logout => "logout",
            Self::Suspend => "suspend",
            Self::Reboot => "reboot",
            Self::Shutdown => "shutdown",
        }
    }

    fn title(self, os: &str) -> &'static str {
        match (self, os) {
            (Self::Lock, _) => "Lock Screen",
            (Self::Logout, "windows") => "Sign Out",
            (Self::Logout, _) => "Log Out",
            (Self::Suspend, "macos" | "windows") => "Sleep",
            (Self::Suspend, _) => "Suspend",
            (Self::Reboot, _) => "Restart",
            (Self::Shutdown, _) => "Shut Down",
        }
    }

    fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::Lock => &["lock", "screen lock"],
            Self::Logout => &["logout", "log off", "sign out", "exit session"],
            Self::Suspend => &["suspend", "sleep"],
            Self::Reboot => &["reboot", "restart"],
            Self::Shutdown => &["shutdown", "power off", "poweroff", "halt"],
        }
    }

    /// The program and arguments that perform this command on `os`.
    pub fn argv(self, os: &str) -> (&'static str, Vec<&'static str>) {
        match os {
            "macos" => match self {
                Self::Lock => ("pmset", vec!["displaysleepnow"]),
                Self::Logout => (
                    "osascript",
                    vec!["-e", "tell application \"System Events\" to log out"],
                ),
                Self::Suspend => ("pmset", vec!["sleepnow"]),
                Self::Reboot => (
                    "osascript",
                    vec!["-e", "tell application \"System Events\" to restart"],
                ),
                Self::Shutdown => (
                    "osascript",
                    vec!["-e", "tell application \"System Events\" to shut down"],
                ),
            },
            "windows" => match self {
                Self::Lock => ("rundll32.exe", vec!["user32.dll,LockWorkStation"]),
                Self::Logout => ("shutdown.exe", vec!["/l"]),
                Self::Suspend => (
                    "rundll32.exe",
                    vec!["powrprof.dll,SetSuspendState", "0,1,0"],
                ),
                Self::Reboot => ("shutdown.exe", vec!["/r", "/t", "0"]),
                Self::Shutdown => ("shutdown.exe", vec!["/s", "/t", "0"]),
            },
            // systemd-logind; `self` is the caller's session.
            _ => match self {
                Self::Lock => ("loginctl", vec!["lock-session"]),
                Self::Logout => ("loginctl", vec!["terminate-session", "self"]),
                Self::Suspend => ("systemctl", vec!["suspend"]),
                Self::Reboot => ("systemctl", vec!["reboot"]),
                Self::Shutdown => ("systemctl", vec!["poweroff"]),
            },
        }
    }
}

/// The commands for one OS.
pub struct SystemProvider {
    items: Vec<Item>,
}

impl SystemProvider {
    /// Commands for `os` (a [`std::env::consts::OS`] value); unknown Unixes use logind.
    pub fn for_os(os: &str) -> Self {
        let items = Command::ALL
            .iter()
            .map(|&command| {
                let (program, args) = command.argv(os);
                let description = std::iter::once(program)
                    .chain(args.iter().copied())
                    .collect::<Vec<_>>()
                    .join(" ");
                Item::new(
                    ItemId::new(NAME, command.key()),
                    command.title(os),
                    Action::Run {
                        program: program.into(),
                        args: args.into_iter().map(str::to_owned).collect(),
                        confirm: true,
                    },
                )
                .with_subtitle(description)
                .with_keywords(command.keywords().iter().copied())
            })
            .collect();
        Self { items }
    }
}

impl Provider for SystemProvider {
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

    #[test]
    fn every_os_has_every_command_behind_confirmation() {
        for os in ["linux", "macos", "windows", "freebsd"] {
            let provider = SystemProvider::for_os(os);
            let items = provider.items(&Query::new(""), &Cancel::new()).unwrap();
            assert_eq!(items.len(), Command::ALL.len(), "{os}");
            assert!(
                items
                    .iter()
                    .all(|i| i.actions.iter().all(Action::needs_confirmation)),
                "{os}"
            );
        }
    }

    #[test]
    fn commands_per_os() {
        assert_eq!(Command::Reboot.argv("linux"), ("systemctl", vec!["reboot"]));
        assert_eq!(Command::Suspend.argv("macos"), ("pmset", vec!["sleepnow"]));
        assert_eq!(
            Command::Shutdown.argv("windows"),
            ("shutdown.exe", vec!["/s", "/t", "0"])
        );
        assert_eq!(Command::Lock.title("linux"), "Lock Screen");
        assert_eq!(Command::Logout.title("windows"), "Sign Out");
    }
}
