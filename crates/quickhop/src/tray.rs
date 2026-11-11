//! quickhop's tray icon on Linux: a `StatusNotifierItem` over D-Bus, which KDE Plasma, GNOME
//! with the `AppIndicator` extension, waybar, sway's swaybar and most panels show. Its menu
//! opens the settings, reloads the config or quits quickhop, each by asking the daemon over its
//! socket, as `quickhop settings`, `quickhop reload` and `quickhop daemon stop` do. Where no
//! panel shows such items the icon simply never appears.

use std::sync::{Arc, Mutex, PoisonError};

use ksni::blocking::TrayMethods as _;
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem};
use quickhop_core::Flavour;

use crate::daemon::{self, Request};

/// The icon as 32 x 32 ARGB32 pixels in network byte order, drawn from
/// `packaging/linux/quickhop.svg`: shown when the icon theme has no `quickhop` icon (a
/// development build, a binary without its desktop files).
const PIXMAP: &[u8] = include_bytes!("../assets/tray-icon-32.argb");

/// The pixmap's side in pixels.
const PIXMAP_SIDE: i32 = 32;

/// What the tray shows; its menu acts through the daemon's socket.
struct QuickhopTray;

/// Sends `request` to this daemon from a thread of its own (the tray's thread must keep
/// answering D-Bus, and the request thread may be busy); failures go to standard error.
fn ask(request: Request) {
    std::thread::spawn(move || {
        if let Err(error) = daemon::send(&request).and_then(daemon::message) {
            crate::warn(&format!("tray: {error}"));
        }
    });
}

impl ksni::Tray for QuickhopTray {
    // A left click opens the menu: Linux has no launcher window to open yet.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        Flavour::CURRENT.stem().into()
    }

    fn title(&self) -> String {
        Flavour::CURRENT.app_name().into()
    }

    fn icon_name(&self) -> String {
        Flavour::CURRENT.stem().into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![Icon {
            width: PIXMAP_SIDE,
            height: PIXMAP_SIDE,
            data: PIXMAP.to_vec(),
        }]
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let item = |label: &str, request: fn() -> Request| {
            StandardItem {
                label: label.into(),
                activate: Box::new(move |_: &mut Self| ask(request())),
                ..StandardItem::default()
            }
            .into()
        };
        vec![
            item("Settings…", || Request::Settings),
            item("Reload quickhop config", || Request::Reload),
            MenuItem::Separator,
            item("Quit quickhop", || Request::Stop),
        ]
    }
}

/// The tray icon while it is shown; dropping it removes the icon.
pub(crate) struct Tray {
    handle: ksni::blocking::Handle<QuickhopTray>,
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.handle.shutdown().wait();
    }
}

/// Shows the tray icon.
///
/// # Errors
/// When the session has no D-Bus session bus.
pub(crate) fn show() -> Result<Tray, String> {
    QuickhopTray
        .spawn()
        .map(|handle| Tray { handle })
        .map_err(|error| error.to_string())
}

/// The tray icon kept by the daemon, shown or hidden as `menu_bar_icon` says.
#[derive(Default, Clone)]
pub(crate) struct TraySlot(Arc<Mutex<Option<Tray>>>);

impl TraySlot {
    /// Shows the icon when `on` and it is not shown, removes it when not `on`. Returns the
    /// status line: `on`, `off ([…] menu_bar_icon = false)` or `off (why)`.
    pub(crate) fn set(&self, on: bool) -> String {
        let mut tray = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !on {
            *tray = None;
            return "off (menu_bar_icon = false)".into();
        }
        if tray.is_none() {
            match show() {
                Ok(shown) => *tray = Some(shown),
                Err(error) => return format!("off ({error})"),
            }
        }
        "on".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pixmap_is_32_square_argb() {
        assert_eq!(PIXMAP.len(), 32 * 32 * 4);
        // The corners are transparent, the middle of the lens is not.
        assert_eq!(PIXMAP[0], 0);
        let middle = (16 * 32 + 16) * 4;
        assert_eq!(PIXMAP[middle], 0xff);
    }

    #[test]
    fn the_menu_settles_on_settings_reload_and_quit() {
        let labels: Vec<String> = ksni::Tray::menu(&QuickhopTray)
            .into_iter()
            .map(|item| match item {
                MenuItem::Standard(item) => item.label,
                MenuItem::Separator => "-".into(),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(
            labels,
            ["Settings…", "Reload quickhop config", "-", "Quit quickhop"]
        );
    }
}
