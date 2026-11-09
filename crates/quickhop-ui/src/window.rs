//! The launcher window on macOS: one gpui window over the [`Model`], created hidden when the
//! app starts and shown and hidden from then on (ADR 0001: reopening doubles the show time).
//!
//! [`run`] takes over the main thread; everything else talks to the window through a
//! [`Handle`] from other threads. Searches and actions go to a [`Backend`] on gpui's
//! background threads, so the window never waits for a provider.

use std::io::Write as _;
use std::sync::Arc;
use std::sync::mpsc;

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, FontWeight, HighlightStyle,
    KeyDownEvent, QuitMode, Render, SharedString, StyledText, Window, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowKind, WindowOptions, div, prelude::*, px, rgb, rgba, size,
    uniform_list,
};
use quickhop_core::{Action, Item, ItemId, Query, Ranked};

use crate::model::{Effect, Model, SearchRequest, Snapshot, Toast, ToastStyle, highlight_ranges};

const ROW_HEIGHT: f32 = 44.0;
const WINDOW_WIDTH: f32 = 750.0;
const WINDOW_HEIGHT: f32 = 474.0;

/// What the window needs from the rest of quickhop. Called on background threads.
pub trait Backend: Send + Sync + 'static {
    /// The results for `query`, best first: the root search when `source` is `None`, else the
    /// items of the command that pushed the page.
    fn search(&self, source: Option<&ItemId>, query: &Query) -> Vec<Ranked>;

    /// Carries out `action` of `item`, chosen while `query` was typed. Returns what to tell
    /// the user, if anything.
    ///
    /// # Errors
    /// A message for the user when the action failed.
    fn perform(&self, item: &Item, action: &Action, query: &Query) -> Result<String, String>;
}

/// What a [`Handle`] asks of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Show the window with an empty root search.
    Show,
    /// Hide it.
    Hide,
    /// Show it when hidden, else hide it.
    Toggle,
    /// Change nothing; report what it shows.
    Snapshot,
    /// End the app: [`run`] returns.
    Quit,
}

/// The app ended before the command was carried out.
#[derive(Debug, thiserror::Error)]
#[error("the launcher window is gone")]
pub struct Gone;

/// What a [`Handle`] sends to the app's main thread.
enum Message {
    /// A command, and where its snapshot goes.
    Command(Command, mpsc::Sender<Snapshot>),
    /// Work that must run on the main thread (registering a hotkey).
    Job(Box<dyn FnOnce() + Send>),
}

/// Sends [`Command`]s to the window from any thread.
#[derive(Clone)]
pub struct Handle {
    messages: async_channel::Sender<Message>,
}

impl Handle {
    /// Carries out `command` and returns what the window shows afterwards. Never call it on
    /// the main thread: it waits for that thread.
    ///
    /// # Errors
    /// [`Gone`] when the app has ended.
    pub fn send(&self, command: Command) -> Result<Snapshot, Gone> {
        let (reply, answer) = mpsc::channel();
        self.messages
            .send_blocking(Message::Command(command, reply))
            .map_err(|_| Gone)?;
        answer.recv().map_err(|_| Gone)
    }

    /// Runs `job` on the app's main thread, after the commands sent before it, without
    /// waiting for it.
    ///
    /// # Errors
    /// [`Gone`] when the app has ended.
    pub fn on_main_thread(&self, job: impl FnOnce() + Send + 'static) -> Result<(), Gone> {
        self.messages
            .send_blocking(Message::Job(Box::new(job)))
            .map_err(|_| Gone)
    }
}

/// Runs the launcher app on this thread, which must be the main thread, with its window hidden,
/// until a [`Command::Quit`]. `started` gets the window's [`Handle`] on a thread of its own as
/// soon as the app runs.
pub fn run(backend: Arc<dyn Backend>, started: impl FnOnce(Handle) + Send + 'static) {
    let (messages, incoming) = async_channel::unbounded::<Message>();
    gpui_platform::application()
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            // An agent app: no Dock icon, no menu bar.
            cx.set_activation_policy(gpui::ActivationPolicy::Accessory);
            let launcher = match open(backend, cx) {
                Ok(launcher) => launcher,
                Err(error) => {
                    let _ = writeln!(
                        std::io::stderr(),
                        "quickhop: cannot open the launcher window: {error}"
                    );
                    cx.quit();
                    return;
                }
            };
            cx.spawn(async move |cx| {
                while let Ok(message) = incoming.recv().await {
                    let (command, reply) = match message {
                        Message::Command(command, reply) => (command, reply),
                        Message::Job(job) => {
                            job();
                            continue;
                        }
                    };
                    let quit = command == Command::Quit;
                    let snapshot = cx.update(|cx| launcher.carry_out(command, cx));
                    let _ = reply.send(snapshot);
                    if quit {
                        cx.update(|cx| cx.quit());
                        break;
                    }
                }
            })
            .detach();
            let handle = Handle { messages };
            std::thread::spawn(move || started(handle));
        });
}

/// The window and the view in it.
struct Opened {
    window: WindowHandle<Launcher>,
    launcher: Entity<Launcher>,
}

impl Opened {
    fn carry_out(&self, command: Command, cx: &mut App) -> Snapshot {
        let visible = self.launcher.read(cx).visible;
        let show = match command {
            Command::Show => true,
            Command::Toggle => !visible,
            Command::Hide | Command::Quit => false,
            Command::Snapshot => return self.launcher.read(cx).snapshot(),
        };
        let _ = self.window.update(cx, |launcher, window, cx| {
            if show {
                launcher.show(window, cx);
            } else {
                launcher.hide(cx);
            }
        });
        self.launcher.read(cx).snapshot()
    }
}

fn open(backend: Arc<dyn Backend>, cx: &mut App) -> Result<Opened, Box<dyn std::error::Error>> {
    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        focus: true,
        show: false,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Blurred,
        app_id: Some("quickhop".into()),
        ..WindowOptions::default()
    };
    let mut launcher = None;
    let window = cx.open_window(options, |window, cx| {
        let entity = cx.new(|cx| Launcher::new(backend, window, cx));
        launcher = Some(entity.clone());
        entity
    })?;
    let launcher = launcher.ok_or("the window has no view")?;
    Ok(Opened { window, launcher })
}

/// The launcher view: the search bar, the results, the action panel and a toast.
struct Launcher {
    model: Model,
    backend: Arc<dyn Backend>,
    focus: FocusHandle,
    visible: bool,
}

impl Launcher {
    fn new(backend: Arc<dyn Backend>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Hidden as soon as quickhop is no longer the app in front.
        cx.observe_window_activation(window, |launcher, window, cx| {
            if launcher.visible && !window.is_window_active() {
                launcher.hide(cx);
            }
        })
        .detach();
        Self {
            model: Model::new(),
            backend,
            focus: cx.focus_handle(),
            visible: false,
        }
    }

    fn snapshot(&self) -> Snapshot {
        self.model.snapshot(self.visible)
    }

    fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let effect = self.model.reset();
        self.visible = true;
        cx.activate(true);
        window.activate_window();
        self.focus.focus(window, cx);
        self.apply(effect, cx);
    }

    fn hide(&mut self, cx: &mut Context<Self>) {
        if self.visible {
            self.visible = false;
            cx.hide();
        }
    }

    /// Carries out what the model asked for after an input.
    fn apply(&mut self, effect: Effect, cx: &mut Context<Self>) {
        match effect {
            Effect::None => {}
            Effect::Hide => self.hide(cx),
            Effect::Search(request) => self.search(request, cx),
            Effect::Perform {
                item,
                action,
                query,
            } => self.perform(*item, action, query, cx),
        }
        cx.notify();
    }

    fn search(&self, request: SearchRequest, cx: &mut Context<Self>) {
        let backend = Arc::clone(&self.backend);
        let SearchRequest {
            generation,
            source,
            query,
        } = request;
        let task = cx
            .background_executor()
            .spawn(async move { backend.search(source.as_ref(), &query) });
        cx.spawn(async move |this, cx| {
            let results = task.await;
            let _ = this.update(cx, |launcher, cx| {
                if launcher.model.set_results(generation, results) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Hides the window, then carries out the action; a failure shows again with a toast.
    fn perform(&mut self, item: Item, action: Action, query: Query, cx: &mut Context<Self>) {
        self.hide(cx);
        let backend = Arc::clone(&self.backend);
        let task = cx
            .background_executor()
            .spawn(async move { backend.perform(&item, &action, &query) });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            let _ = this.update(cx, |launcher, cx| {
                let toast = match outcome {
                    Ok(message) if message.is_empty() => return,
                    Ok(message) => Toast {
                        style: ToastStyle::Success,
                        title: message,
                    },
                    Err(message) => Toast {
                        style: ToastStyle::Failure,
                        title: message,
                    },
                };
                launcher.model.show_toast(toast);
                cx.notify();
            });
        })
        .detach();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let effect = match keystroke.key.as_str() {
            "k" if modifiers.platform || modifiers.control => {
                self.model.toggle_actions();
                Effect::None
            }
            _ if modifiers.platform || modifiers.control => return,
            "escape" => self.model.escape(),
            "enter" => self.model.activate(),
            "down" => {
                self.model.select_next();
                Effect::None
            }
            "up" => {
                self.model.select_previous();
                Effect::None
            }
            "backspace" => {
                let mut text = self.model.bar_text().to_owned();
                text.pop();
                self.model.set_query(text)
            }
            _ => match &keystroke.key_char {
                Some(typed) => {
                    let text = format!("{}{typed}", self.model.bar_text());
                    self.model.set_query(text)
                }
                None => return,
            },
        };
        self.apply(effect, cx);
    }

    fn render_row(index: usize, ranked: &Ranked, selected: bool) -> impl IntoElement + use<> {
        let title = ranked.item.title.clone();
        let highlights = highlight_ranges(&title, &ranked.positions)
            .into_iter()
            .map(|range| {
                (
                    range,
                    HighlightStyle {
                        color: Some(rgb(0x00ff_ffff).into()),
                        font_weight: Some(FontWeight::BOLD),
                        ..HighlightStyle::default()
                    },
                )
            })
            .collect::<Vec<_>>();
        let subtitle = ranked.item.subtitle.clone().unwrap_or_default();
        div()
            .id(index)
            .h(px(ROW_HEIGHT))
            .px_3()
            .mx_2()
            .flex()
            .items_center()
            .gap_3()
            .rounded_md()
            .when(selected, |row| row.bg(rgba(0xffff_ff1f)))
            .child(div().size_6().rounded_sm().bg(rgb(0x005e_81ac)))
            .child(
                div()
                    .flex_1()
                    .text_color(rgb(0x00d8_dee9))
                    .child(StyledText::new(SharedString::from(title)).with_highlights(highlights)),
            )
            .child(div().text_sm().text_color(rgb(0x008f_96a3)).child(subtitle))
    }

    fn render_panel(&self) -> Option<impl IntoElement> {
        let panel = self.model.panel()?;
        let selected = panel.selected();
        let rows = panel.entries().iter().enumerate().map(|(index, entry)| {
            div()
                .px_3()
                .py_1()
                .rounded_md()
                .text_color(rgb(0x00d8_dee9))
                .when(selected == Some(index), |row| row.bg(rgba(0xffff_ff1f)))
                .child(entry.title.clone())
        });
        Some(
            div()
                .absolute()
                .right_3()
                .bottom_3()
                .w(px(300.0))
                .p_2()
                .rounded_lg()
                .bg(rgba(0x1e22_2af0))
                .border_1()
                .border_color(rgba(0xffff_ff1a))
                .child(
                    div()
                        .px_3()
                        .pb_1()
                        .text_sm()
                        .text_color(rgb(0x008f_96a3))
                        .child(panel.item().title.clone()),
                )
                .children(rows),
        )
    }

    fn render_toast(&self) -> Option<impl IntoElement> {
        let toast = self.model.toast()?;
        let color = match toast.style {
            ToastStyle::Success => rgb(0x00a3_be8c),
            ToastStyle::Failure => rgb(0x00bf_616a),
            ToastStyle::Progress => rgb(0x0088_c0d0),
        };
        Some(
            div()
                .h(px(32.0))
                .px_4()
                .flex()
                .items_center()
                .border_t_1()
                .border_color(rgba(0xffff_ff1a))
                .text_sm()
                .text_color(color)
                .child(toast.title.clone()),
        )
    }
}

impl Render for Launcher {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = self.model.bar_text();
        let placeholder = text.is_empty();
        let field: SharedString = if placeholder {
            if self.model.panel().is_some() {
                "Search for actions...".into()
            } else {
                "Search for apps and commands...".into()
            }
        } else {
            text.to_owned().into()
        };
        let count = self.model.page().results().len();
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .rounded_xl()
            .overflow_hidden()
            .bg(rgba(0x2e34_40b0))
            .text_color(rgb(0x00ec_eff4))
            .child(
                div()
                    .h(px(56.0))
                    .px_4()
                    .flex()
                    .items_center()
                    .text_xl()
                    .border_b_1()
                    .border_color(rgba(0xffff_ff1a))
                    .when(placeholder, |field| field.text_color(rgb(0x008f_96a3)))
                    .child(field),
            )
            .child(
                uniform_list(
                    "results",
                    count,
                    cx.processor(|launcher, range: std::ops::Range<usize>, _window, _cx| {
                        let page = launcher.model.page();
                        let selected = page.selected();
                        page.results()[range.clone()]
                            .iter()
                            .zip(range)
                            .map(|(ranked, index)| {
                                Self::render_row(index, ranked, selected == Some(index))
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .py_2()
                .flex_1(),
            )
            .children(self.render_toast())
            .children(self.render_panel())
    }
}
