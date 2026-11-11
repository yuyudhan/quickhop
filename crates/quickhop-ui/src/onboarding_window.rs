//! The onboarding window (PLT-02, plan slice C2): an ordinary titled window over the
//! [`Onboarding`] model that says whether quickhop may paste into the app in front (the
//! Accessibility permission), with the one button that leads to the setting. It asks the
//! system again whenever quickhop becomes the active app, which is how the user comes back
//! from System Settings: this window or the launcher coming to the front. After the button
//! led there it also asks every second, and comes back to the front by itself once the user
//! has turned quickhop on.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, AppContext as _, Bounds, ClickEvent, Context, FocusHandle, Font, FontWeight, KeyDownEvent,
    Pixels, Render, SharedString, Task, TextRun, TitlebarOptions, Window, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, WindowTextSystem, div, font, prelude::*, px, relative,
    rgb, size,
};

use crate::onboarding::{Onboarding, Press, Program, Status};
use crate::settings_window::is_dark;
use crate::theme;
use crate::window::Backend;

/// How often the window asks the system while the user is in System Settings.
const WATCH_EVERY: Duration = Duration::from_secs(1);

/// The window's width; its height follows the text it shows ([`height`]).
pub const WIDTH: f32 = 480.0;
/// The space around the content.
const PADDING: f32 = 24.0;
/// The space between the title, the status, the text and the button.
const GAP: f32 = 12.0;
/// The title's text size.
const TITLE_SIZE: f32 = 20.0;
/// The size of every other text.
const TEXT_SIZE: f32 = 14.0;
/// The line height, as a multiple of the text size (gpui's default).
const LINE_HEIGHT: f32 = std::f32::consts::GOLDEN_RATIO;
/// The button's padding above and below its label.
const BUTTON_PADDING_Y: f32 = 4.0;
/// Rounding of the layout to whole pixels.
const SLACK: f32 = 2.0;
/// The width the texts wrap at.
const WRAP_WIDTH: f32 = WIDTH - 2.0 * PADDING;

/// The system font the window draws with (gpui's default family).
fn text_font() -> Font {
    font(".SystemUIFont")
}

/// How many lines `text` takes in `font` at `font_size`, wrapped at [`WRAP_WIDTH`].
fn lines(text_system: &WindowTextSystem, text: &str, font: Font, font_size: f32) -> usize {
    let run = TextRun {
        len: text.len(),
        font,
        ..TextRun::default()
    };
    let wrapped = text_system.shape_text(
        SharedString::from(text.to_owned()),
        px(font_size),
        &[run],
        Some(px(WRAP_WIDTH)),
        None,
    );
    match wrapped {
        Ok(wrapped) => wrapped
            .iter()
            .map(|line| line.wrap_boundaries().len() + 1)
            .sum(),
        // No shaping: a generous estimate, fewer characters per line than any size fits.
        Err(_) => text
            .lines()
            .map(|line| {
                line.chars()
                    .count()
                    .max(1)
                    .div_ceil(FALLBACK_CHARS_PER_LINE)
            })
            .sum::<usize>()
            .max(1),
    }
}

/// Characters per line when the text cannot be shaped: fewer than fit at the title's size.
const FALLBACK_CHARS_PER_LINE: usize = 30;

/// The height of the window's content showing `model`: padding, the title, the status line,
/// the text as `text_system` wraps it, and the button, so the button is always whole.
pub fn height(text_system: &WindowTextSystem, model: &Onboarding) -> Pixels {
    let title = Font {
        weight: FontWeight::BOLD,
        ..text_font()
    };
    let line = |size: f32, count: usize| {
        // Line counts are small; beyond u16 the window is off any screen anyway.
        let count = f32::from(u16::try_from(count).unwrap_or(u16::MAX));
        size * LINE_HEIGHT * count
    };
    let title_lines = lines(text_system, model.title(), title, TITLE_SIZE);
    let message_lines = lines(text_system, &model.message(), text_font(), TEXT_SIZE);
    px(2.0 * PADDING
        + line(TITLE_SIZE, title_lines)
        + line(TEXT_SIZE, 1)
        + line(TEXT_SIZE, message_lines)
        + line(TEXT_SIZE, 1)
        + 2.0 * BUTTON_PADDING_Y
        // Between the title, status, text, spacer and button.
        + 4.0 * GAP
        + SLACK)
}

/// The window's title in the title bar.
const TITLE: &str = "Welcome to quickhop";

/// The onboarding window's view.
pub(crate) struct OnboardingView {
    model: Onboarding,
    backend: Arc<dyn Backend>,
    focus: FocusHandle,
    /// Asking the system every [`WATCH_EVERY`] while the model is watching; dropping it stops.
    watch: Option<Task<()>>,
    /// The content height the window was last given, so a render resizes it only when the
    /// text needs another.
    fitted: Pixels,
}

/// The program macOS checks the permission of: this process's executable, symlinks resolved.
fn own_program() -> Program {
    match std::env::current_exe() {
        Ok(exe) => Program::of(&std::fs::canonicalize(&exe).unwrap_or(exe)),
        Err(_) => Program::of(Path::new("quickhop")),
    }
}

/// Opens the onboarding window in front of every other app, focused.
///
/// # Errors
/// When the window cannot be opened.
pub(crate) fn open(
    backend: Arc<dyn Backend>,
    cx: &mut App,
) -> Result<WindowHandle<OnboardingView>, String> {
    let model = Onboarding::new(backend.may_paste(), backend.hotkey(), own_program());
    let fitted = height(&WindowTextSystem::new(Arc::clone(cx.text_system())), &model);
    let bounds = Bounds::centered(None, size(px(WIDTH), fitted), cx);
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
        app_id: Some(quickhop_core::Flavour::CURRENT.stem().into()),
        ..WindowOptions::default()
    };
    let handle = cx
        .open_window(options, |window, cx| {
            cx.new(|cx| OnboardingView::new(model, fitted, backend, window, cx))
        })
        .map_err(|error| error.to_string())?;
    // quickhop is an agent app: its windows come to the front only when it is the active app.
    cx.activate(true);
    Ok(handle)
}

impl OnboardingView {
    fn new(
        model: Onboarding,
        fitted: Pixels,
        backend: Arc<dyn Backend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
            model,
            backend,
            focus,
            watch: None,
            fitted,
        }
    }

    /// Gives the window the height its text needs now, after this frame.
    fn fit(&mut self, window: &Window, cx: &mut Context<Self>) {
        let needed = height(window.text_system(), &self.model);
        if (needed - self.fitted).abs() < px(0.5) {
            return;
        }
        self.fitted = needed;
        cx.defer_in(window, move |_, window, _| {
            window.resize(size(px(WIDTH), needed));
        });
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
                cx.spawn_in(window, async move |this, cx| {
                    let outcome = task.await;
                    let _ = this.update_in(cx, |view, window, cx| {
                        view.model.asked(outcome);
                        view.watch(window, cx);
                        cx.notify();
                    });
                })
                .detach();
            }
            Press::Close => window.remove_window(),
        }
    }

    /// While the model is watching, asks the system every [`WATCH_EVERY`]; once it allows
    /// pasting, brings the window to the front, so the user sees it is done. A new ask
    /// replaces (and so stops) the watch of the one before.
    fn watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.model.watching() {
            self.watch = None;
            return;
        }
        self.watch = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH_EVERY).await;
                let watching = this.update_in(cx, |view, window, cx| {
                    view.recheck(cx);
                    if view.model.granted() {
                        view.bring_to_front(window, cx);
                    }
                    view.model.watching()
                });
                if !matches!(watching, Ok(true)) {
                    break;
                }
            }
        }));
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.fit(window, cx);
        let palette = theme::palette(self.backend.theme(), is_dark(window.appearance()));
        let dot = if self.model.granted() {
            rgb(palette.success)
        } else {
            rgb(palette.warning)
        };
        // The sizes are the ones [`height`] measures with; the button row never shrinks, so
        // it stays whole even before a resize lands.
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .p(px(PADDING))
            .line_height(relative(LINE_HEIGHT))
            .bg(rgb(palette.window))
            .text_color(rgb(palette.strong))
            .child(
                div()
                    .flex_none()
                    .text_size(px(TITLE_SIZE))
                    .font_weight(FontWeight::BOLD)
                    .child(self.model.title()),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size_2().rounded_full().bg(dot))
                    .child(
                        div()
                            .text_size(px(TEXT_SIZE))
                            .child(self.model.status_line()),
                    ),
            )
            .child(
                div()
                    .flex_shrink(1.0)
                    .min_h_0()
                    .overflow_hidden()
                    .text_size(px(TEXT_SIZE))
                    .text_color(rgb(palette.text))
                    .child(self.model.message()),
            )
            .child(div().flex_1())
            .child(
                div().flex_none().flex().justify_end().child(
                    div()
                        .id("onboarding-button")
                        .px_4()
                        .py(px(BUTTON_PADDING_Y))
                        .rounded_md()
                        .bg(rgb(palette.accent))
                        .text_color(rgb(palette.on_accent))
                        .text_size(px(TEXT_SIZE))
                        .cursor_pointer()
                        .child(self.model.button())
                        .on_click(cx.listener(Self::on_button)),
                ),
            )
    }
}
