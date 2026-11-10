//! The launcher window on macOS: one gpui window over the [`Model`], created hidden when the
//! app starts and shown and hidden from then on (ADR 0001: reopening doubles the show time).
//! Beside it, on demand, the onboarding window that asks for the permission pasting needs.
//!
//! [`run`] takes over the main thread; everything else talks to the window through a
//! [`Handle`] from other threads. Searches and actions go to a [`Backend`] on gpui's
//! background threads, so the window never waits for a provider.

use std::cell::{Cell, OnceCell};
use std::collections::HashMap;
use std::io::Write as _;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, FontWeight, HighlightStyle, Image,
    ImageFormat, KeyDownEvent, QuitMode, Render, SharedString, StyledText, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, img,
    prelude::*, px, rgb, rgba, size, uniform_list,
};
use quickhop_core::{Action, Flavour, Item, ItemId, Query, Ranked};

use crate::model::{
    Effect, Model, Outcome, PageKind, SearchRequest, Snapshot, ToastStyle, highlight_ranges,
    keeps_window,
};
use crate::onboarding_window::{self, OnboardingView};

const ROW_HEIGHT: f32 = 44.0;
const WINDOW_WIDTH: f32 = 750.0;
const WINDOW_HEIGHT: f32 = 474.0;
const HUD_WIDTH: f32 = 360.0;
const HUD_HEIGHT: f32 = 44.0;
/// Distance from the HUD to the bottom of the screen.
const HUD_BOTTOM: f32 = 140.0;
/// How long a HUD stays on screen.
const HUD_SECONDS: u64 = 2;

/// The size, in pixels, icons are asked for: the 24-point icon slot on a 2× display, with room.
pub const ICON_PIXELS: u32 = 64;

/// Icons kept at most; past it the cache starts again, since items come and go with searches.
const MAX_ICONS: usize = 1_000;

/// What the window needs from the rest of quickhop. Called on background threads.
pub trait Backend: Send + Sync + 'static {
    /// The results for `query`, best first: the root search when `source` is `None`, else the
    /// items of the command that pushed the page.
    fn search(&self, source: Option<&ItemId>, query: &Query) -> Vec<Ranked>;

    /// Carries out `action` of `item` with `args` for a script's arguments, chosen while
    /// `query` was typed. Returns what came of it for the user.
    ///
    /// # Errors
    /// A message for the user when the action failed.
    fn perform(
        &self,
        item: &Item,
        action: &Action,
        args: &[String],
        query: &Query,
    ) -> Result<Outcome, String>;

    /// The icon of `item` as PNG data at least [`ICON_PIXELS`] square, when the system has
    /// one for it (an application, a file); `None` leaves the row's text icon, if any.
    fn icon(&self, item: &Item) -> Option<Vec<u8>>;

    /// Whether quickhop may paste into the app in front: whether the user allowed it under
    /// Privacy & Security › Accessibility. The onboarding window asks again whenever quickhop
    /// becomes the active app.
    fn may_paste(&self) -> bool;

    /// Leads the user to allow pasting: the system's prompt, which also puts quickhop in the
    /// Accessibility list, then that pane of System Settings.
    ///
    /// # Errors
    /// Why System Settings did not open.
    fn allow_paste(&self) -> Result<(), String>;

    /// The global hotkey that shows the launcher, as bound; `None` when none is.
    fn hotkey(&self) -> Option<String>;
}

/// A result's icon in the window's cache.
enum Icon {
    /// Asked of the backend, not answered yet.
    Loading,
    /// Drawn in the row.
    Ready(Arc<Image>),
    /// The backend has none.
    Missing,
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
    /// Search the page shown again: its items changed (an inline script's new subtitle).
    Refresh,
    /// Open the onboarding window, or bring it to the front and ask again whether quickhop
    /// may paste.
    Onboarding,
    /// quickhop.app was opened (Finder, Spotlight, `open`): show the launcher, or the
    /// onboarding window while quickhop may not paste yet.
    AppOpened,
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
    let application = gpui_platform::application().with_quit_mode(QuitMode::Explicit);
    let windows = Rc::new(OnceCell::<Opened>::new());
    let reopened = Rc::clone(&windows);
    // quickhop.app opened again while it runs (Finder, Spotlight, `open`).
    application.on_reopen(move |cx| {
        if let Some(opened) = reopened.get() {
            opened.carry_out(Command::AppOpened, cx);
        }
    });
    application.run(move |cx: &mut App| {
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
        let _ = windows.set(launcher.clone());
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

/// The onboarding window while it is open: shared by the commands and the launcher window,
/// whose activation also makes it ask again.
type OnboardingSlot = Rc<Cell<Option<WindowHandle<OnboardingView>>>>;

/// The windows and the views in them.
#[derive(Clone)]
struct Opened {
    window: WindowHandle<Launcher>,
    launcher: Entity<Launcher>,
    backend: Arc<dyn Backend>,
    onboarding: OnboardingSlot,
}

impl Opened {
    fn carry_out(&self, command: Command, cx: &mut App) -> Snapshot {
        let visible = self.launcher.read(cx).visible;
        let show = match command {
            Command::Show => true,
            Command::Toggle => !visible,
            Command::Hide | Command::Quit => false,
            Command::Snapshot => return self.snapshot(cx),
            Command::Refresh => {
                self.launcher.update(cx, |launcher, cx| {
                    let effect = launcher.model.refresh();
                    launcher.apply(effect, cx);
                });
                return self.snapshot(cx);
            }
            Command::AppOpened if self.backend.may_paste() => true,
            Command::Onboarding | Command::AppOpened => {
                self.show_onboarding(cx);
                return self.snapshot(cx);
            }
        };
        let _ = self.window.update(cx, |launcher, window, cx| {
            if show {
                launcher.show(window, cx);
            } else {
                launcher.hide(cx);
            }
        });
        self.snapshot(cx)
    }

    /// What the windows show.
    fn snapshot(&self, cx: &App) -> Snapshot {
        let mut snapshot = self.launcher.read(cx).snapshot();
        snapshot.onboarding = self
            .onboarding
            .get()
            .and_then(|handle| handle.read(cx).ok())
            .map(OnboardingView::status);
        snapshot
    }

    /// Brings the onboarding window to the front, opening it unless it is open.
    fn show_onboarding(&self, cx: &mut App) {
        if let Some(handle) = self.onboarding.get()
            && handle.update(cx, OnboardingView::bring_to_front).is_ok()
        {
            return;
        }
        match onboarding_window::open(Arc::clone(&self.backend), cx) {
            Ok(handle) => self.onboarding.set(Some(handle)),
            Err(error) => {
                self.onboarding.set(None);
                let _ = writeln!(
                    std::io::stderr(),
                    "quickhop: cannot open the onboarding window: {error}"
                );
            }
        }
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
        app_id: Some(Flavour::CURRENT.stem().into()),
        ..WindowOptions::default()
    };
    let mut launcher = None;
    let onboarding = OnboardingSlot::default();
    let window = cx.open_window(options, |window, cx| {
        let entity = cx.new(|cx| Launcher::new(Arc::clone(&backend), window, cx));
        // The launcher in front (the hotkey) means quickhop became the active app: the
        // onboarding window, when open, asks again whether quickhop may paste.
        let slot = Rc::clone(&onboarding);
        entity.update(cx, |_, cx| {
            cx.observe_window_activation(window, move |_, window, cx| {
                if window.is_window_active()
                    && let Some(handle) = slot.get()
                {
                    let _ = handle.update(cx, |view, _, cx| view.recheck(cx));
                }
            })
            .detach();
        });
        launcher = Some(entity.clone());
        entity
    })?;
    let launcher = launcher.ok_or("the window has no view")?;
    Ok(Opened {
        window,
        launcher,
        backend,
        onboarding,
    })
}

/// The launcher view: the search bar, the results, the action panel and a toast.
struct Launcher {
    model: Model,
    backend: Arc<dyn Backend>,
    focus: FocusHandle,
    visible: bool,
    icons: HashMap<ItemId, Icon>,
    /// The HUD on screen, if any, and its message.
    hud: Option<(WindowHandle<Hud>, String)>,
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
            icons: HashMap::new(),
            hud: None,
        }
    }

    fn snapshot(&self) -> Snapshot {
        let mut snapshot = self.model.snapshot(self.visible);
        snapshot.icons = self
            .model
            .page()
            .results()
            .iter()
            .map(|ranked| matches!(self.icons.get(&ranked.item.id), Some(Icon::Ready(_))))
            .collect();
        snapshot.hud = self.hud.as_ref().map(|(_, text)| text.clone());
        snapshot
    }

    /// Shows `text` in a HUD, a small window low in the middle of the screen, for
    /// [`HUD_SECONDS`], over whatever app is in front (quickhop stays hidden and inactive).
    fn show_hud(&mut self, text: String, cx: &mut Context<Self>) {
        if let Some((old, _)) = self.hud.take() {
            let _ = old.update(cx, |_, window, _| window.remove_window());
        }
        let width = px(HUD_WIDTH);
        let height = px(HUD_HEIGHT);
        let bounds = match cx.primary_display() {
            Some(display) => {
                let screen = display.bounds();
                Bounds::new(
                    gpui::point(
                        screen.origin.x + (screen.size.width - width) / 2.0,
                        screen.origin.y + screen.size.height - height - px(HUD_BOTTOM),
                    ),
                    size(width, height),
                )
            }
            None => Bounds::centered(None, size(width, height), cx),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: false,
            show: false,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Blurred,
            app_id: Some(Flavour::CURRENT.stem().into()),
            ..WindowOptions::default()
        };
        let message = SharedString::from(text.clone());
        let opened = cx.open_window(options, |window, cx| {
            if let Err(error) = quickhop_platform_macos::panel::show_while_hidden(window) {
                let _ = writeln!(std::io::stderr(), "quickhop: cannot show the HUD: {error}");
            }
            cx.new(|_| Hud { text: message })
        });
        let handle = match opened {
            Ok(handle) => handle,
            Err(error) => {
                let _ = writeln!(std::io::stderr(), "quickhop: cannot open the HUD: {error}");
                return;
            }
        };
        self.hud = Some((handle, text));
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(HUD_SECONDS))
                .await;
            let _ = this.update(cx, |launcher, cx| {
                if launcher.hud.as_ref().is_some_and(|(h, _)| *h == handle) {
                    launcher.hud = None;
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                }
            });
        })
        .detach();
    }

    /// Asks the backend for the icons of `items`, whose icons are not asked for yet; each row
    /// redraws once its icon arrives.
    fn load_icons(&mut self, items: Vec<Item>, cx: &mut Context<Self>) {
        if self.icons.len() + items.len() > MAX_ICONS {
            self.icons.clear();
        }
        for item in items {
            self.icons.insert(item.id.clone(), Icon::Loading);
            let backend = Arc::clone(&self.backend);
            let task = cx.background_executor().spawn(async move {
                let png = backend.icon(&item);
                (item.id, png)
            });
            cx.spawn(async move |this, cx| {
                let (id, png) = task.await;
                let icon = png.map_or(Icon::Missing, |bytes| {
                    Icon::Ready(Arc::new(Image::from_bytes(ImageFormat::Png, bytes)))
                });
                let _ = this.update(cx, |launcher, cx| {
                    launcher.icons.insert(id, icon);
                    cx.notify();
                });
            })
            .detach();
        }
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
                args,
                query,
            } => self.perform(*item, action, args, query, cx),
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

    /// Carries out the action: with the window shown when it has something to show there (a
    /// progress toast says it runs), else after hiding the window. What comes of it is shown
    /// through the model, or as a HUD.
    fn perform(
        &mut self,
        item: Item,
        action: Action,
        args: Vec<String>,
        query: Query,
        cx: &mut Context<Self>,
    ) {
        if keeps_window(&action) {
            self.model.running(&item);
        } else {
            self.hide(cx);
        }
        let id = item.id.clone();
        let backend = Arc::clone(&self.backend);
        let task = cx
            .background_executor()
            .spawn(async move { backend.perform(&item, &action, &args, &query) });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            let _ = this.update(cx, |launcher, cx| {
                match outcome {
                    Ok(Outcome::Hud(text)) => {
                        launcher.model.performed(&id, Outcome::Done);
                        launcher.show_hud(text, cx);
                    }
                    Ok(outcome) => {
                        let effect = launcher.model.performed(&id, outcome);
                        launcher.apply(effect, cx);
                    }
                    Err(message) => launcher.model.failed(message),
                }
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
            "tab" => {
                if !self.model.step_field(!modifiers.shift) {
                    return;
                }
                Effect::None
            }
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

    fn render_row(
        index: usize,
        ranked: &Ranked,
        selected: bool,
        icon: Option<Arc<Image>>,
    ) -> impl IntoElement + use<> {
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
        let slot = div().size_6().flex().items_center().justify_center();
        let slot = match (icon, text_icon(&ranked.item)) {
            (Some(image), _) => slot.child(img(image).size_6()),
            (None, Some(glyph)) => slot.child(glyph),
            (None, None) => slot,
        };
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
            .child(slot)
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

    /// The search bar: the query (the panel's filter while it is open, the page's title on a
    /// detail page, or as the placeholder of a list), then a script's argument fields while it
    /// asks for them.
    fn render_bar(&self) -> impl IntoElement + use<> {
        let page = self.model.page();
        let (text, placeholder): (SharedString, bool) = if let Some(panel) = self.model.panel() {
            if panel.filter().is_empty() {
                ("Search for actions...".into(), true)
            } else {
                (panel.filter().to_owned().into(), false)
            }
        } else if matches!(page.kind, PageKind::Detail { .. }) {
            (page.title.clone().into(), false)
        } else if page.query().is_empty() && matches!(page.kind, PageKind::List { .. }) {
            (format!("{}...", page.title).into(), true)
        } else if page.query().is_empty() {
            ("Search for apps and commands...".into(), true)
        } else {
            (page.query().to_owned().into(), false)
        };
        let fields = self.model.form().into_iter().flat_map(|form| {
            let focused = form.focused();
            form.arguments()
                .iter()
                .enumerate()
                .map(|(index, argument)| {
                    let shown = form.shown(index);
                    let empty = shown.is_empty();
                    div()
                        .px_2()
                        .py_1()
                        .min_w(px(90.0))
                        .rounded_md()
                        .text_base()
                        .bg(rgba(0xffff_ff14))
                        .border_1()
                        .border_color(if index == focused {
                            rgba(0x88c0_d0ff)
                        } else {
                            rgba(0xffff_ff1a)
                        })
                        .when(empty, |field| field.text_color(rgb(0x008f_96a3)))
                        .child(if empty {
                            argument.placeholder.clone()
                        } else {
                            shown
                        })
                })
                .collect::<Vec<_>>()
        });
        div()
            .h(px(56.0))
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .text_xl()
            .border_b_1()
            .border_color(rgba(0xffff_ff1a))
            .child(
                div()
                    .when(placeholder, |field| field.text_color(rgb(0x008f_96a3)))
                    .child(text),
            )
            .children(fields)
    }

    /// A detail page's text, its ANSI colours drawn.
    fn render_detail(text: &str) -> impl IntoElement + use<> {
        let styled = crate::ansi::parse(text);
        let highlights = styled
            .spans
            .into_iter()
            .map(|(range, colour)| {
                (
                    range,
                    HighlightStyle {
                        color: Some(rgb(ANSI_COLOURS[usize::from(colour.0)]).into()),
                        ..HighlightStyle::default()
                    },
                )
            })
            .collect::<Vec<_>>();
        div()
            .id("detail")
            .flex_1()
            .overflow_y_scroll()
            .px_4()
            .py_3()
            .text_base()
            .text_color(rgb(0x00d8_dee9))
            .child(StyledText::new(SharedString::from(styled.text)).with_highlights(highlights))
    }

    fn render_results(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let count = self.model.page().results().len();
        uniform_list(
            "results",
            count,
            cx.processor(|launcher, range: std::ops::Range<usize>, _window, cx| {
                let missing: Vec<Item> = launcher.model.page().results()[range.clone()]
                    .iter()
                    .filter(|ranked| !launcher.icons.contains_key(&ranked.item.id))
                    .map(|ranked| ranked.item.clone())
                    .collect();
                launcher.load_icons(missing, cx);
                let page = launcher.model.page();
                let selected = page.selected();
                page.results()[range.clone()]
                    .iter()
                    .zip(range)
                    .map(|(ranked, index)| {
                        let icon = match launcher.icons.get(&ranked.item.id) {
                            Some(Icon::Ready(image)) => Some(Arc::clone(image)),
                            _ => None,
                        };
                        Self::render_row(index, ranked, selected == Some(index), icon)
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .py_2()
        .flex_1()
    }
}

impl Render for Launcher {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.model.page().kind {
            PageKind::Detail { text } => Self::render_detail(text).into_any_element(),
            PageKind::Root | PageKind::List { .. } => self.render_results(cx).into_any_element(),
        };
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
            .child(self.render_bar())
            .child(body)
            .children(self.render_toast())
            .children(self.render_panel())
    }
}

/// A short message on its own, low in the middle of the screen: see [`Launcher::show_hud`].
struct Hud {
    text: SharedString,
}

impl Render for Hud {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .px_4()
            .rounded_xl()
            .bg(rgba(0x2e34_40d0))
            .text_color(rgb(0x00ec_eff4))
            .child(self.text.clone())
    }
}

/// The 16 ANSI colours as drawn on the window's dark background: normal, then bright.
const ANSI_COLOURS: [u32; 16] = [
    0x004c_566a,
    0x00ff_5555,
    0x00a3_be8c,
    0x00eb_cb8b,
    0x0081_a1c1,
    0x00b4_8ead,
    0x0088_c0d0,
    0x00e5_e9f0,
    0x0061_6e88,
    0x00ff_7a7a,
    0x00b9_d4a5,
    0x00f0_d8a8,
    0x009a_b5d1,
    0x00c8_a6c3,
    0x008f_bcbb,
    0x00ec_eff4,
];

/// The item's icon when it is text to draw as is, such as a script command's `👋`: a few
/// characters that are not a file path or a URL (those are images, loaded by the backend).
fn text_icon(item: &Item) -> Option<SharedString> {
    const MAX_CHARS: usize = 4;
    let icon = item.icon.as_deref()?.trim();
    let is_text =
        !icon.is_empty() && icon.chars().count() <= MAX_CHARS && !icon.contains(['/', '.', ':']);
    is_text.then(|| SharedString::from(icon.to_owned()))
}
