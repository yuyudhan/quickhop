//! A window of quickhop that stays on screen while quickhop is hidden: the HUD a silent script
//! or a copy leaves after the launcher hid (SCR-07). Hiding the app is what gives the focus back
//! to the app the user came from, and `AppKit` hides every window of a hidden app unless the
//! window says it cannot hide.

use objc2::MainThreadMarker;
use objc2_app_kit::NSView;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Why a window could not be shown over the hidden app.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PanelError {
    /// Called on another thread than the main one.
    #[error("windows are shown on the main thread")]
    NotMainThread,
    /// The window has no `AppKit` view (it was closed, or is no `AppKit` window).
    #[error("the window has no AppKit view")]
    NoView,
}

/// Makes `window` stay on screen when quickhop is hidden and orders it to the front, without
/// making quickhop the active app (the focus stays where it is).
///
/// # Errors
/// [`PanelError`] off the main thread, or when `window` has no `AppKit` view.
pub fn show_while_hidden(window: &impl HasWindowHandle) -> Result<(), PanelError> {
    MainThreadMarker::new().ok_or(PanelError::NotMainThread)?;
    let handle = window.window_handle().map_err(|_| PanelError::NoView)?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err(PanelError::NoView);
    };
    // SAFETY: an AppKit window handle points to the window's `NSView`, alive while `handle`
    // borrows `window`; this runs on the main thread (checked above), where AppKit views live.
    let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    let ns_window = view.window().ok_or(PanelError::NoView)?;
    ns_window.setCanHide(false);
    ns_window.orderFrontRegardless();
    Ok(())
}
