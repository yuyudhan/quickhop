//! quickhop's icon in the menu bar (`NSStatusItem`), as Raycast has one: a monochrome template
//! image macOS tints for a light or dark menu bar, and a menu whose entries call back into
//! quickhop. The icon is `assets/menu-bar-icon.png`, drawn from
//! `packaging/macos/menu-bar-icon.svg`.

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{AnyThread as _, DefinedClass as _, MainThreadMarker, MainThreadOnly};
use objc2::{define_class, msg_send, sel};
use objc2_app_kit::{
    NSEventModifierFlags, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSSize, NSString};

/// The icon: 36 x 36 pixels, black on transparent, shown at 18 x 18 points.
const ICON: &[u8] = include_bytes!("../assets/menu-bar-icon.png");

/// The icon's size in the menu bar, in points.
const ICON_POINTS: f64 = 18.0;

/// Why the menu-bar icon could not be shown.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StatusItemError {
    /// Called on another thread than the main one.
    #[error("the menu-bar icon is made on the main thread")]
    NotMainThread,
    /// The menu bar gave the item no button to draw in.
    #[error("the menu bar has no room for the icon")]
    NoButton,
    /// The icon's PNG could not be decoded.
    #[error("cannot decode the menu-bar icon")]
    Icon,
}

/// One line of the menu.
pub enum Entry {
    /// A command: its title, the key shown beside it with `⌘` (`","`, `"q"`, or `""` for
    /// none), and what choosing it does (called on the main thread).
    Command {
        /// The title, e.g. `Settings…`.
        title: String,
        /// The key shown with `⌘`.
        key: String,
        /// Runs when the entry is chosen.
        action: Box<dyn Fn()>,
    },
    /// A separator line.
    Separator,
}

/// The callbacks of the menu's commands, indexed by each item's tag.
struct Actions {
    actions: Vec<Box<dyn Fn()>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `MenuTarget` implements no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Actions]
    struct MenuTarget;

    impl MenuTarget {
        /// The action of every menu item: runs the callback the item's tag names.
        #[unsafe(method(choose:))]
        fn choose(&self, sender: &NSMenuItem) {
            if let Ok(index) = usize::try_from(sender.tag())
                && let Some(action) = self.ivars().actions.get(index)
            {
                action();
            }
        }
    }

    unsafe impl NSObjectProtocol for MenuTarget {}
);

impl MenuTarget {
    fn new(actions: Vec<Box<dyn Fn()>>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Actions { actions });
        // SAFETY: `init` is NSObject's designated initialiser, called once on the freshly
        // allocated object whose ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

/// The icon in the menu bar; dropping it removes the icon.
pub struct StatusItem {
    item: Retained<NSStatusItem>,
    /// Kept alive: menu items hold their target weakly.
    _target: Retained<MenuTarget>,
}

impl std::fmt::Debug for StatusItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusItem")
            .field("visible", &self.item.isVisible())
            .finish_non_exhaustive()
    }
}

/// Puts quickhop's icon in the menu bar with `entries` as its menu and `tooltip` (the app's
/// name) as its help tag and accessibility description.
///
/// # Errors
/// [`StatusItemError`] off the main thread, or when the icon cannot be drawn.
pub fn install(tooltip: &str, entries: Vec<Entry>) -> Result<StatusItem, StatusItemError> {
    let mtm = MainThreadMarker::new().ok_or(StatusItemError::NotMainThread)?;
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(ICON))
        .ok_or(StatusItemError::Icon)?;
    image.setSize(NSSize::new(ICON_POINTS, ICON_POINTS));
    image.setTemplate(true);
    let name = NSString::from_str(tooltip);
    image.setAccessibilityDescription(Some(&name));

    let menu = NSMenu::new(mtm);
    let mut actions = Vec::new();
    let mut items = Vec::new();
    for entry in entries {
        match entry {
            Entry::Separator => items.push(NSMenuItem::separatorItem(mtm)),
            Entry::Command { title, key, action } => {
                // SAFETY: `choose:` is a valid selector, implemented by the `MenuTarget` set as
                // the item's target below.
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(&title),
                        Some(sel!(choose:)),
                        &NSString::from_str(&key),
                    )
                };
                item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
                item.setTag(isize::try_from(actions.len()).unwrap_or(isize::MAX));
                actions.push(action);
                items.push(item);
            }
        }
    }
    let target = MenuTarget::new(actions, mtm);
    for item in &items {
        if item.action().is_some() {
            // SAFETY: `target` responds to `choose:` and outlives the menu: `StatusItem`
            // keeps it for as long as the item, and so the menu, exists.
            unsafe { item.setTarget(Some(&target)) };
        }
        menu.addItem(item);
    }

    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let Some(button) = item.button(mtm) else {
        NSStatusBar::systemStatusBar().removeStatusItem(&item);
        return Err(StatusItemError::NoButton);
    };
    button.setImage(Some(&image));
    button.setToolTip(Some(&name));
    item.setMenu(Some(&menu));
    Ok(StatusItem {
        item,
        _target: target,
    })
}

impl StatusItem {
    /// Shows or hides the icon, keeping its place in the menu bar.
    pub fn set_visible(&self, visible: bool) {
        self.item.setVisible(visible);
    }
}

impl Drop for StatusItem {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}
