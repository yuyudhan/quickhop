# Platform support

quickhop targets macOS and Linux (X11 and Wayland) as equals, from one codebase. This page says
exactly what works where today, and which system tool or API each feature uses.

## Systems

| System | CPU | Status |
|--------|-----|--------|
| macOS | Apple silicon, Intel | Supported. Built and tested in CI and on Apple-silicon Macs. |
| Linux, X11 | x86_64, aarch64 | Supported. Tested on Ubuntu 22.04 and under Xvfb. |
| Linux, Wayland | x86_64, aarch64 | Supported; clipboard watching needs the data-control protocol (below). |
| Windows | x86_64 | The search, launch and script-command CLI builds and passes CI. The daemon, autostart and the installer are Unix-only. |

## Features by platform

| Feature | macOS | Linux X11 | Linux Wayland |
|---------|-------|-----------|---------------|
| Applications | App bundles in the Applications folders, with icons | Desktop entries in every XDG `applications` folder, icon themes | same as X11 |
| Calculator, units, emoji | ✅ | ✅ | ✅ |
| System commands | `osascript`, `pmset` | `loginctl`, `systemctl` | same as X11 |
| Quicklinks | `open` | `xdg-open` | `xdg-open` |
| Script commands | ✅ (AppleScript and Swift too) | ✅ | ✅ |
| Snippets | ✅ | ✅ | ✅ |
| File search | quickhop index once built (FSEvents keeps it current), else Spotlight (`mdfind`): 3+ characters, 200 paths, 2 s limit | quickhop index (SQLite FTS5), kept current with inotify | same as X11 |
| File actions | open, reveal in Finder, copy path, Quick Look, trash | open, reveal, copy path, trash (freedesktop trash) | same as X11 |
| Clipboard history | `clipboard watch` and the daemon poll the pasteboard every 250 ms; concealed and transient copies skipped | `clipboard watch` and the daemon (XFixes, large transfers included); copies flagged `x-kde-passwordManagerHint` skipped | `clipboard watch` and the daemon via `wl-paste --watch` on sway, Hyprland, KDE Plasma, COSMIC; password-manager copies skipped |
| Paste into the focused app | copies, then ⌘V once Accessibility is granted | copies, then Ctrl+V (XTest) | copies; you press Ctrl+V |
| Copy results | the pasteboard | `xclip` | `wl-copy` |
| Daemon and autostart | LaunchAgent in `~/Library/LaunchAgents` | XDG autostart entry in `~/.config/autostart`, or the systemd user unit from the `.deb` | same as X11 |
| Packages | `quickhop.app` (`just bundle-macos`), build from source | `.deb` and tarball (`just bundle-linux`), build from source | same as X11 |

## Permissions

- **macOS Accessibility** lets quickhop press ⌘V for `--paste`, `clipboard paste` and Clipboard
  History. Grant it in System Settings → Privacy & Security → Accessibility to the program that
  runs quickhop: quickhop.app, whose onboarding window leads there (`quickhop onboarding`), or
  your terminal while you use the command line. Without it quickhop copies and tells you so.
- **Linux** needs no permission on X11. On Wayland, clipboard watching relies on the
  compositor's data-control protocol rather than a permission prompt.

## Known limits

- **GNOME on Wayland** offers no data-control protocol, so `clipboard watch` cannot see
  copies there; `wl-paste` stops with a message saying so. `clipboard add` still records text,
  and a GNOME X11 session watches normally.
- **Synthetic paste on Wayland** is not available to ordinary clients; `--paste` and
  `clipboard paste` copy the text and print `copied, not pasted` with the reason.
- **Spotlight exclusions**: before the index exists, folders excluded from Spotlight in
  System Settings are not found on macOS. Build the index (`quickhop index`) to search them.
- **Text only**: the clipboard history keeps text up to 1 MiB per entry; images, files and
  rich text are not recorded.

## The launcher window

The launcher window lives in `quickhop-ui`, using [gpui](adr/0001-ui-toolkit.md) on Metal
(macOS) and Vulkan (Linux). On macOS it is built: `quickhop daemon` creates it hidden and
registers the global `hotkey` through Carbon's `RegisterEventHotKey`, which needs no
permission. It is a non-activating panel: the app in front stays in front while it shows.
On Linux it is still being built; until then, bind a key in your desktop or compositor
to a `quickhop open ID` command for a global shortcut to any result. The window's slices,
machines and acceptance tests are in [plan.md](plan.md).
