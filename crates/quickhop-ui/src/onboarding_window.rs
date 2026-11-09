//! The onboarding window (PLT-02, plan slice C2): an ordinary titled window over the
//! [`Onboarding`] model that says whether quickhop may paste into the app in front (the
//! Accessibility permission), with the one button that leads to the setting. It asks the
//! system again whenever quickhop becomes the active app, which is how the user comes back
//! from System Settings: this window or the launcher coming to the front.

use std::sync::Arc;

use gpui::{
    App, AppContext as _, Bounds, ClickEvent, Context, FocusHandle, FontWeight, KeyDownEvent,
    Render, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowKind, WindowOptions, div,
    prelude::*, px, rgb, size,
};

use crate::onboarding::{Onboarding, Press, Status};
use crate::window::Backend;

const WIDTH: f32 = 480.0;
const HEIGHT: f32 = 250.0;

/// The window's title in the title bar.
const TITLE: &str = "Welcome to quickhop";

/// The onboarding window's view.
pub(crate) struct OnboardingView {
    model: Onboarding,
    backend: Arc<dyn Backend>,
    focus: FocusHandle,
}

/// Opens the onboarding window in front of every other app, focused.
///
/// # Errors
/// When the window cannot be opened.
pub(crate) fn open(
    backend: Arc<dyn Backend>,
    cx: &mut App,
) -> Result<WindowHandle<OnboardingView>, String> {
    let bounds = Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(TITLE.into()),
            ..TitlebarOptions::default()
        }),
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        is_resizable: false,
        is_minimizable: false,
        app_id: Some("quickhop".into()),
        ..WindowOptions::default()
    };
    let handle = cx
        .open_window(options, |window, cx| {
            cx.new(|cx| OnboardingView::new(backend, window, cx))
        })
        .map_err(|error| error.to_string())?;
    // quickhop is an agent app: its windows come to the front only when it is the active app.
    cx.activate(true);
    Ok(handle)
}

impl OnboardingView {
    fn new(backend: Arc<dyn Backend>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Clicking this window, back from System Settings, makes quickhop the active app.
        cx.observe_window_activation(window, |view, window, cx| {
            if window.is_window_active() {
                view.recheck(cx);
            }
        })
        .detach();
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            model: Onboarding::new(backend.may_paste(), backend.hotkey()),
            backend,
            focus,
        }
    }

    /// What the window says, for the UI tests' snapshot.
    pub(crate) fn status(&self) -> Status {
        self.model.status()
    }

    /// Asks the system again whether quickhop may paste, and which hotkey is bound.
    pub(crate) fn recheck(&mut self, cx: &mut Context<Self>) {
        self.model.set_hotkey(self.backend.hotkey());
        self.model.recheck(self.backend.may_paste());
        cx.notify();
    }

    /// Brings the window to the front, focused, and asks again.
    pub(crate) fn bring_to_front(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.activate(true);
        window.activate_window();
        self.focus.focus(window, cx);
        self.recheck(cx);
    }

    /// The button: lead to the setting, or close the window once nothing is missing.
    fn press(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.model.press() {
            Press::Ask => {
                let backend = Arc::clone(&self.backend);
                // The system prompt and `open` take a moment: off the main thread.
                let task = cx
                    .background_executor()
                    .spawn(async move { backend.allow_paste() });
                cx.spawn(async move |this, cx| {
                    let outcome = task.await;
                    let _ = this.update(cx, |view, cx| {
                        view.model.asked(outcome);
                        cx.notify();
                    });
                })
                .detach();
            }
            Press::Close => window.remove_window(),
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "enter" => self.press(window, cx),
            "escape" => window.remove_window(),
            _ => {}
        }
    }

    fn on_button(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.press(window, cx);
    }
}

impl Render for OnboardingView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dot = if self.model.granted() {
            rgb(0x00a3_be8c)
        } else {
            rgb(0x00eb_cb8b)
        };
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_6()
            .bg(rgb(0x002e_3440))
            .text_color(rgb(0x00ec_eff4))
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .child(self.model.title()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size_2().rounded_full().bg(dot))
                    .child(div().text_sm().child(self.model.status_line())),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x00d8_dee9))
                    .child(self.model.message()),
            )
            .child(div().flex_1())
            .child(
                div().flex().justify_end().child(
                    div()
                        .id("onboarding-button")
                        .px_4()
                        .py_1()
                        .rounded_md()
                        .bg(rgb(0x005e_81ac))
                        .text_sm()
                        .cursor_pointer()
                        .child(self.model.button())
                        .on_click(cx.listener(Self::on_button)),
                ),
            )
    }
}
