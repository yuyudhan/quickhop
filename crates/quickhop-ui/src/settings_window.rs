//! The settings window (CFG-01), as Raycast has one: an ordinary titled window over the
//! [`SettingsModel`], opened by quickhop Settings in the search, the menu-bar icon or
//! `quickhop settings`. Every change goes to the [`Backend`], which writes `config.toml` (or
//! the login item) and reloads quickhop, on a background thread; the window shows what it read
//! back. Coming back to the front reads the file again, so edits made in an editor show.
//!
//! Keys: `↑`/`↓` (and `Tab`) select a row, `Return` or `Space` flips a switch, cycles the theme,
//! starts the hotkey recorder or opens the file, `←`/`→` step numbers, themes and switches,
//! `Esc` stops the recorder or closes the window, as `⌘W` does.

use std::sync::Arc;

use gpui::{
    App, AppContext as _, Bounds, ClickEvent, Context, FocusHandle, FontWeight, KeyDownEvent,
    Render, ScrollHandle, SharedString, TitlebarOptions, Window, WindowAppearance, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, div, prelude::*, px, rgb, rgba, size,
};

use crate::settings::{Change, Held, Row, SettingsModel, Value};
use crate::theme::{self, Palette};
use crate::window::Backend;

/// The window's width.
const WIDTH: f32 = 560.0;
/// The window's height; the rows scroll inside it.
const HEIGHT: f32 = 660.0;
/// The window's title in the title bar.
const TITLE: &str = "quickhop Settings";

/// Whether gpui's `appearance` is a dark one.
pub(crate) fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

/// The settings window's view.
pub(crate) struct SettingsView {
    model: SettingsModel,
    backend: Arc<dyn Backend>,
    focus: FocusHandle,
    scroll: ScrollHandle,
}

/// Opens the settings window in front of every other app, focused.
///
/// # Errors
/// When the window cannot be opened.
pub(crate) fn open(
    backend: Arc<dyn Backend>,
    cx: &mut App,
) -> Result<WindowHandle<SettingsView>, String> {
    let model = SettingsModel::new(backend.settings());
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
        app_id: Some(quickhop_core::Flavour::CURRENT.stem().into()),
        ..WindowOptions::default()
    };
    let handle = cx
        .open_window(options, |window, cx| {
            cx.new(|cx| SettingsView::new(model, backend, window, cx))
        })
        .map_err(|error| error.to_string())?;
    // quickhop is an agent app: its windows come to the front only when it is the active app.
    cx.activate(true);
    Ok(handle)
}

impl SettingsView {
    fn new(
        model: SettingsModel,
        backend: Arc<dyn Backend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Back in front (from an editor, say): read the file again.
        cx.observe_window_activation(window, |view, window, cx| {
            if window.is_window_active() {
                view.reread(cx);
            }
        })
        .detach();
        cx.observe_window_appearance(window, |_, _, cx| cx.notify())
            .detach();
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            model,
            backend,
            focus,
            scroll: ScrollHandle::new(),
        }
    }

    /// Brings the window to the front, focused, with the values read again.
    pub(crate) fn bring_to_front(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.activate(true);
        window.activate_window();
        self.focus.focus(window, cx);
        self.reread(cx);
    }

    /// Reads the settings again, off the main thread.
    fn reread(&mut self, cx: &mut Context<Self>) {
        let backend = Arc::clone(&self.backend);
        let task = cx
            .background_executor()
            .spawn(async move { backend.settings() });
        cx.spawn(async move |this, cx| {
            let values = task.await;
            let _ = this.update(cx, |view, cx| {
                view.model.refreshed(values);
                cx.notify();
            });
        })
        .detach();
    }

    /// Hands `change` to the backend, off the main thread, and shows what came of it.
    fn change(&mut self, change: Option<Change>, cx: &mut Context<Self>) {
        let Some(change) = change else {
            return;
        };
        let backend = Arc::clone(&self.backend);
        let asked = change.clone();
        let task = cx
            .background_executor()
            .spawn(async move { backend.change_setting(&asked) });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            let _ = this.update(cx, |view, cx| {
                view.model.applied(&change, outcome);
                cx.notify();
            });
        })
        .detach();
    }

    fn click(&mut self, index: usize, cx: &mut Context<Self>) {
        let change = self.model.click(index);
        self.change(change, cx);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if self.model.is_recording() {
            let held = Held {
                command: modifiers.platform,
                control: modifiers.control,
                alt: modifiers.alt,
                shift: modifiers.shift,
            };
            let change = self.model.record(&keystroke.key, held);
            self.change(change, cx);
            cx.notify();
            return;
        }
        let change = match keystroke.key.as_str() {
            "w" if modifiers.platform => {
                window.remove_window();
                return;
            }
            "escape" => {
                window.remove_window();
                return;
            }
            _ if modifiers.platform || modifiers.control || modifiers.alt => return,
            "down" => {
                self.model.select_next();
                None
            }
            "tab" if !modifiers.shift => {
                self.model.select_next();
                None
            }
            "up" | "tab" => {
                self.model.select_previous();
                None
            }
            "enter" | "space" => self.model.activate(),
            "left" => self.model.step(false),
            "right" => self.model.step(true),
            _ => return,
        };
        self.scroll.scroll_to_item(self.model.selected());
        self.change(change, cx);
        cx.notify();
    }

    /// A switch: a pill, filled when on, with its knob on that side.
    fn switch(on: bool, palette: &Palette) -> impl IntoElement + use<> {
        div()
            .w(px(36.0))
            .h(px(20.0))
            .p(px(2.0))
            .flex()
            .rounded_full()
            .when(on, |pill| pill.justify_end().bg(rgb(palette.accent)))
            .when(!on, |pill| pill.justify_start().bg(rgba(palette.field)))
            .child(div().size(px(16.0)).rounded_full().bg(rgb(0x00ff_ffff)))
    }

    /// The control on the right of a row.
    fn control(value: Value, selected: bool, palette: &Palette) -> gpui::AnyElement {
        let boxed = |text: String, focused: bool| {
            div()
                .px_2()
                .py(px(2.0))
                .rounded_md()
                .bg(rgba(palette.field))
                .border_1()
                .border_color(if focused {
                    rgb(palette.accent)
                } else {
                    rgba(palette.border)
                })
                .child(text)
        };
        match value {
            Value::Switch(on) => Self::switch(on, palette).into_any_element(),
            Value::Choice(text) => boxed(format!("‹ {text} ›"), selected).into_any_element(),
            Value::Number(count) => boxed(format!("− {count} +"), selected).into_any_element(),
            Value::Shortcut { text, recording } => boxed(text, recording).into_any_element(),
            Value::Button(text) => div()
                .px_3()
                .py(px(2.0))
                .rounded_md()
                .bg(rgb(palette.accent))
                .text_color(rgb(palette.on_accent))
                .child(text)
                .into_any_element(),
            Value::Unavailable(why) => div()
                .max_w(px(220.0))
                .text_xs()
                .text_color(rgb(palette.muted))
                .child(why)
                .into_any_element(),
        }
    }

    fn render_row(
        &self,
        index: usize,
        row: Row,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let selected = self.model.selected() == index;
        let heading = SettingsModel::heading(row).map(|heading| {
            div()
                .px_3()
                .pt_4()
                .pb_1()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(palette.muted))
                .child(heading)
        });
        let line = div()
            .id(("settings-row", index))
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_4()
            .rounded_md()
            .cursor_pointer()
            .when(selected, |line| line.bg(rgba(palette.selection)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.click(index, cx)))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(palette.text))
                            .child(SettingsModel::label(row)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(palette.muted))
                            .child(SharedString::from(self.model.hint(row))),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_sm()
                    .text_color(rgb(palette.text))
                    .child(Self::control(self.model.value(row), selected, palette)),
            );
        div().children(heading).child(line)
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self
            .model
            .values()
            .map(|values| values.config.theme)
            .unwrap_or_default();
        let palette = theme::palette(theme, is_dark(window.appearance()));
        let rows: Vec<_> = self
            .model
            .rows()
            .into_iter()
            .enumerate()
            .map(|(index, row)| self.render_row(index, row, palette, cx))
            .collect();
        let message = self.model.message().map(|message| {
            div()
                .flex_none()
                .px_4()
                .py_2()
                .border_t_1()
                .border_color(rgba(palette.border))
                .text_xs()
                .text_color(rgb(if message.failure {
                    palette.failure
                } else {
                    palette.success
                }))
                .child(SharedString::from(message.text.clone()))
        });
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(palette.window))
            .text_color(rgb(palette.text))
            .child(
                div()
                    .id("settings-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px_2()
                    .pb_3()
                    .children(rows),
            )
            .children(message)
    }
}
