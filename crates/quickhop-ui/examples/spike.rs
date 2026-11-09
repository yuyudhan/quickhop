//! Toolkit spike for plan slice U0 (`docs/adr/0001-ui-toolkit.md`): a gpui launcher window,
//! a search field over 5,000 fake items ranked by `quickhop-core`, driven over a Unix socket so
//! that show latency and per-keystroke frame cost can be measured from outside the process.
//!
//! ```text
//! cargo run --release -p quickhop-ui --example spike -- serve /tmp/spike.sock [options]
//! cargo run --release -p quickhop-ui --example spike -- measure /tmp/spike.sock [runs]
//! cargo run --release -p quickhop-ui --example spike -- send /tmp/spike.sock <command>
//! ```
//!
//! `serve` options: `--items <n>` (5000), `--background opaque|transparent|blurred` (blurred),
//! `--strategy hide|reopen` (hide: the window stays alive and the app is hidden; reopen: the
//! window is closed on hide and opened again on show), `--throttle on|off` (off: gpui's 30 fps
//! limit for inactive windows is lifted).
//!
//! The socket takes one command per line and answers each with one line:
//!
//! - `show`, `hide`, `toggle`: a show is answered once the first frame after it has been drawn
//!   and presented while the window is the key window: `ok frame_us=… draw_us=…`.
//! - `type <text>`: types each character as one keystroke, answered after the frame showing the
//!   last one: `ok search_us=… draw_us=… frame_us=… results=…`. `back` deletes one character,
//!   `clear` empties the field.
//! - `stats`: the same timings, summarised over every keystroke that arrived as a real key event
//!   (for example from `scripts/spike-keys.swift`); `stats reset` clears them.
//! - `background opaque|transparent|blurred`, `pid`, `quit`.
//!
//! Timings: `frame_us` runs from receiving the command (or the key event) to the end of the
//! frame that shows its result, including the wait for the display's next frame; `draw_us` is
//! that frame's layout, paint and presentation, from the start of rendering to the end of the
//! presentation; `search_us` is ranking every item.

#![forbid(unsafe_code)]

#[cfg(unix)]
mod spike {
    use std::fmt::Write as _;
    use std::io::{BufRead, BufReader, Write};
    use std::ops::Range;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use gpui::{
        App, AppContext as _, Bounds, Context, Entity, FocusHandle, FontWeight, HighlightStyle,
        KeyDownEvent, QuitMode, Render, SharedString, StyledText, Window,
        WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div,
        prelude::*, px, rgb, rgba, size, uniform_list,
    };
    use quickhop_core::{
        Action, Boosts, Cancel, Item, ItemId, Provider, ProviderError, Query, Ranked, Search,
    };

    /// Result for the program's entry points.
    type Fallible<T = ()> = Result<T, Box<dyn std::error::Error>>;

    const DEFAULT_ITEMS: usize = 5_000;
    const ROW_HEIGHT: f32 = 44.0;
    const WINDOW_WIDTH: f32 = 750.0;
    const WINDOW_HEIGHT: f32 = 474.0;
    /// How long a show may wait for the window to become key before it is answered with an
    /// error.
    const SHOW_DEADLINE: Duration = Duration::from_secs(2);

    /// Fake application-like items: deterministic titles, subtitles and keywords.
    struct FakeItems {
        items: Vec<Item>,
    }

    impl FakeItems {
        fn new(count: usize) -> Self {
            const FIRST: [&str; 24] = [
                "Fire", "Safe", "Note", "Mail", "Photo", "Music", "Code", "Text", "Term", "Book",
                "Calc", "Chart", "Clip", "Draw", "Find", "Game", "Home", "Link", "Map", "News",
                "Pass", "Shop", "Time", "Word",
            ];
            const SECOND: [&str; 16] = [
                "fox", "ari", "pad", "box", "board", "studio", "desk", "flow", "kit", "lab", "hub",
                "works", "smith", "view", "craft", "deck",
            ];
            const KINDS: [&str; 8] = [
                "Application",
                "Script Command",
                "Quicklink",
                "System Command",
                "Extension",
                "Snippet",
                "File",
                "Folder",
            ];
            let items = (0..count)
                .map(|n| {
                    let first = FIRST[n % FIRST.len()];
                    let second = SECOND[(n / FIRST.len()) % SECOND.len()];
                    let round = n / (FIRST.len() * SECOND.len());
                    let title = if round == 0 {
                        format!("{first}{second}")
                    } else {
                        format!("{first}{second} {round}")
                    };
                    let kind = KINDS[n % KINDS.len()];
                    Item::new(
                        ItemId::new("fake", n.to_string()),
                        title,
                        Action::Open {
                            target: format!("/Applications/Fake{n}.app"),
                        },
                    )
                    .with_subtitle(kind)
                    .with_keywords([kind.to_lowercase(), format!("{second} {first}")])
                })
                .collect();
            Self { items }
        }
    }

    impl Provider for FakeItems {
        fn name(&self) -> &'static str {
            "fake"
        }

        fn items(&self, _query: &Query, _cancel: &Cancel) -> Result<Vec<Item>, ProviderError> {
            Ok(self.items.clone())
        }
    }

    /// How the window is hidden and shown again.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Strategy {
        /// The window stays open; hiding hides the application.
        Hide,
        /// Hiding closes the window; showing opens a new one.
        Reopen,
    }

    /// Options of `serve`.
    struct ServeOptions {
        socket: String,
        items: usize,
        background: WindowBackgroundAppearance,
        strategy: Strategy,
        /// Whether gpui's default 30 fps limit for inactive windows applies.
        throttle: bool,
    }

    fn parse_background(name: &str) -> Option<WindowBackgroundAppearance> {
        match name {
            "opaque" => Some(WindowBackgroundAppearance::Opaque),
            "transparent" => Some(WindowBackgroundAppearance::Transparent),
            "blurred" => Some(WindowBackgroundAppearance::Blurred),
            _ => None,
        }
    }

    fn parse_serve(args: &[String]) -> Fallible<ServeOptions> {
        let mut options = ServeOptions {
            socket: args.first().ok_or("serve needs a socket path")?.clone(),
            items: DEFAULT_ITEMS,
            background: WindowBackgroundAppearance::Blurred,
            strategy: Strategy::Hide,
            throttle: false,
        };
        let mut rest = args.iter().skip(1);
        while let Some(flag) = rest.next() {
            let value = rest.next().ok_or_else(|| format!("{flag} needs a value"))?;
            match flag.as_str() {
                "--items" => options.items = value.parse()?,
                "--background" => {
                    options.background = parse_background(value)
                        .ok_or_else(|| format!("unknown background {value}"))?;
                }
                "--strategy" => {
                    options.strategy = match value.as_str() {
                        "hide" => Strategy::Hide,
                        "reopen" => Strategy::Reopen,
                        _ => return Err(format!("unknown strategy {value}").into()),
                    };
                }
                "--throttle" => {
                    options.throttle = match value.as_str() {
                        "on" => true,
                        "off" => false,
                        _ => return Err(format!("--throttle takes on or off, not {value}").into()),
                    };
                }
                _ => return Err(format!("unknown option {flag}").into()),
            }
        }
        Ok(options)
    }

    /// One command from the socket and where its answer goes.
    struct Request {
        command: String,
        received: Instant,
        reply: mpsc::Sender<String>,
    }

    /// What a measurement waits for.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Waiting {
        /// The first frame after a show, drawn while the window is key.
        Shown,
        /// The frame showing a keystroke's results.
        Typed,
    }

    /// A measurement waiting for its frame.
    struct Pending {
        waiting: Waiting,
        started: Instant,
        search: Duration,
        /// `None` for real key events, which are only collected for `stats`.
        reply: Option<mpsc::Sender<String>>,
    }

    /// Timings of one keystroke.
    #[derive(Clone, Copy)]
    struct KeySample {
        search: Duration,
        draw: Duration,
        frame: Duration,
    }

    /// One edit of the search field.
    enum Edit {
        Insert(char),
        Backspace,
        Clear,
    }

    /// The launcher view: a search field over the ranked fake items.
    struct Launcher {
        search: Search,
        focus: FocusHandle,
        query: String,
        results: Vec<Ranked>,
        selected: usize,
        pending: Option<Pending>,
        samples: Vec<KeySample>,
    }

    impl Launcher {
        fn new(items: usize, cx: &mut Context<Self>) -> Self {
            let search = Search::new(items).with_provider(Box::new(FakeItems::new(items)));
            let mut launcher = Self {
                search,
                focus: cx.focus_handle(),
                query: String::new(),
                results: Vec::new(),
                selected: 0,
                pending: None,
                samples: Vec::new(),
            };
            launcher.run_search();
            launcher
        }

        fn run_search(&mut self) {
            let results = self.search.run(
                &Query::new(self.query.clone()),
                &Cancel::default(),
                &Boosts::new(),
            );
            self.results = results.ranked;
            self.selected = 0;
        }

        /// Applies one edit, ranks again and waits for the frame that shows the result.
        fn edit(
            &mut self,
            edit: &Edit,
            started: Instant,
            reply: Option<mpsc::Sender<String>>,
            cx: &mut Context<Self>,
        ) {
            match edit {
                Edit::Insert(character) => self.query.push(*character),
                Edit::Backspace => {
                    self.query.pop();
                }
                Edit::Clear => self.query.clear(),
            }
            let ranking = Instant::now();
            self.run_search();
            let search = ranking.elapsed();
            self.wait_for_frame(Waiting::Typed, started, search, reply, cx);
        }

        fn wait_for_frame(
            &mut self,
            waiting: Waiting,
            started: Instant,
            search: Duration,
            reply: Option<mpsc::Sender<String>>,
            cx: &mut Context<Self>,
        ) {
            if let Some(previous) = self.pending.take()
                && let Some(reply) = previous.reply
            {
                // A newer change supersedes it: the frame it waited for never shows alone.
                send(&reply, "error superseded".into());
            }
            self.pending = Some(Pending {
                waiting,
                started,
                search,
                reply,
            });
            cx.notify();
        }

        fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
            let started = Instant::now();
            let keystroke = &event.keystroke;
            if keystroke.modifiers.platform || keystroke.modifiers.control {
                return;
            }
            match keystroke.key.as_str() {
                "backspace" => self.edit(&Edit::Backspace, started, None, cx),
                "escape" => cx.hide(),
                "down" => self.move_selection(1, cx),
                "up" => self.move_selection(-1, cx),
                _ => {
                    if let Some(text) = &keystroke.key_char {
                        for character in text.chars() {
                            self.edit(&Edit::Insert(character), started, None, cx);
                        }
                    }
                }
            }
        }

        fn move_selection(&mut self, step: isize, cx: &mut Context<Self>) {
            if self.results.is_empty() {
                return;
            }
            let count = self.results.len();
            self.selected = self
                .selected
                .checked_add_signed(step)
                .map_or(count - 1, |index| index % count);
            cx.notify();
        }

        /// Called while rendering: answers a pending measurement once this frame counts.
        fn settle_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
            let Some(pending) = self.pending.take() else {
                return;
            };
            if pending.waiting == Waiting::Shown && !window.is_window_active() {
                if pending.started.elapsed() > SHOW_DEADLINE {
                    if let Some(reply) = pending.reply {
                        send(&reply, "error the window did not become key".into());
                    }
                    return;
                }
                // Not key yet: measure the next frame instead.
                self.pending = Some(pending);
                window.request_animation_frame();
                return;
            }
            // This frame renders the change; rendering is where its layout and paint start.
            let render_start = Instant::now();
            let this = cx.entity();
            // Deferred effects run after the frame has been drawn and presented.
            cx.defer(move |cx| {
                let done = Instant::now();
                let sample = KeySample {
                    search: pending.search,
                    draw: done.saturating_duration_since(render_start),
                    frame: done.saturating_duration_since(pending.started),
                };
                let results = this.read(cx).results.len();
                match (pending.waiting, pending.reply) {
                    (Waiting::Shown, Some(reply)) => send(
                        &reply,
                        format!(
                            "ok frame_us={} draw_us={}",
                            sample.frame.as_micros(),
                            sample.draw.as_micros()
                        ),
                    ),
                    (Waiting::Typed, Some(reply)) => send(
                        &reply,
                        format!(
                            "ok search_us={} draw_us={} frame_us={} results={results}",
                            sample.search.as_micros(),
                            sample.draw.as_micros(),
                            sample.frame.as_micros()
                        ),
                    ),
                    (Waiting::Typed, None) => this.update(cx, |launcher, _| {
                        launcher.samples.push(sample);
                    }),
                    (Waiting::Shown, None) => {}
                }
            });
        }

        fn render_row(&self, index: usize, ranked: &Ranked) -> impl IntoElement + use<> {
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
                .when(index == self.selected, |row| row.bg(rgba(0xffff_ff1f)))
                .child(div().size_6().rounded_sm().bg(rgb(0x005e_81ac)))
                .child(
                    div().flex_1().text_color(rgb(0x00d8_dee9)).child(
                        StyledText::new(SharedString::from(title)).with_highlights(highlights),
                    ),
                )
                .child(div().text_sm().text_color(rgb(0x008f_96a3)).child(subtitle))
        }
    }

    impl Render for Launcher {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.settle_pending(window, cx);
            let field: SharedString = if self.query.is_empty() {
                "Search for apps and commands…".into()
            } else {
                self.query.clone().into()
            };
            let count = self.results.len();
            div()
                .track_focus(&self.focus)
                .on_key_down(cx.listener(Self::on_key_down))
                .size_full()
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
                        .when(self.query.is_empty(), |field| {
                            field.text_color(rgb(0x008f_96a3))
                        })
                        .child(field)
                        .child(div().w(px(2.0)).h(px(24.0)).bg(rgb(0x0088_c0d0))),
                )
                .child(
                    uniform_list(
                        "results",
                        count,
                        cx.processor(|launcher, range: Range<usize>, _window, _cx| {
                            launcher.results[range.clone()]
                                .iter()
                                .zip(range)
                                .map(|(ranked, index)| launcher.render_row(index, ranked))
                                .collect::<Vec<_>>()
                        }),
                    )
                    .py_2()
                    .flex_1(),
                )
        }
    }

    /// Byte ranges of the matched characters, merged where they touch.
    fn highlight_ranges(text: &str, positions: &[usize]) -> Vec<Range<usize>> {
        let mut ranges: Vec<Range<usize>> = Vec::new();
        for (index, (start, character)) in text.char_indices().enumerate() {
            if !positions.contains(&index) {
                continue;
            }
            let end = start + character.len_utf8();
            match ranges.last_mut() {
                Some(last) if last.end == start => last.end = end,
                _ => ranges.push(start..end),
            }
        }
        ranges
    }

    fn send(reply: &mpsc::Sender<String>, line: String) {
        // The client may have gone; nothing else waits for this answer.
        let _ = reply.send(line);
    }

    /// The application state outside the window.
    struct Spike {
        options: ServeOptions,
        window: Option<WindowHandle<Launcher>>,
        launcher: Option<Entity<Launcher>>,
        visible: bool,
    }

    impl Spike {
        fn window_options(&self, cx: &App) -> WindowOptions {
            let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: None,
                focus: true,
                show: false,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                window_background: self.options.background,
                app_id: Some("quickhop-spike".into()),
                // The launcher is only on screen to be used: a frame drawn while macOS is still
                // activating it must not wait for the inactive-window frame limit.
                inactive_frame_interval: if self.options.throttle {
                    WindowOptions::default().inactive_frame_interval
                } else {
                    None
                },
                ..WindowOptions::default()
            }
        }

        fn open(&mut self, cx: &mut App) -> Fallible<WindowHandle<Launcher>> {
            if let Some(window) = self.window {
                return Ok(window);
            }
            let items = self.options.items;
            let launcher = self.launcher.clone();
            let window = cx.open_window(self.window_options(cx), move |window, cx| {
                let launcher = launcher.unwrap_or_else(|| cx.new(|cx| Launcher::new(items, cx)));
                launcher.read(cx).focus.clone().focus(window, cx);
                launcher
            })?;
            self.launcher = Some(window.entity(cx)?);
            self.window = Some(window);
            Ok(window)
        }

        fn show(&mut self, request: Request, cx: &mut App) -> Fallible {
            let window = self.open(cx)?;
            self.visible = true;
            cx.activate(true);
            window.update(cx, |launcher, window, cx| {
                window.activate_window();
                launcher.query.clear();
                launcher.run_search();
                launcher.wait_for_frame(
                    Waiting::Shown,
                    request.received,
                    Duration::ZERO,
                    Some(request.reply),
                    cx,
                );
            })?;
            Ok(())
        }

        fn hide(&mut self, cx: &mut App) -> Fallible {
            self.visible = false;
            match self.options.strategy {
                Strategy::Hide => cx.hide(),
                Strategy::Reopen => {
                    if let Some(window) = self.window.take() {
                        window.update(cx, |_, window, _| window.remove_window())?;
                    }
                    cx.hide();
                }
            }
            Ok(())
        }

        fn handle(&mut self, request: Request, cx: &mut App) -> Fallible {
            // Only the line ending is dropped: `type  ` types a space.
            let command = request.command.trim_end_matches(['\r', '\n']).to_owned();
            let (name, argument) = command.split_once(' ').unwrap_or((command.as_str(), ""));
            match name {
                "show" => self.show(request, cx)?,
                "toggle" if !self.visible => self.show(request, cx)?,
                "hide" | "toggle" => {
                    self.hide(cx)?;
                    send(&request.reply, "ok hidden".into());
                }
                "type" | "back" | "clear" => self.edit(name, argument, &request, cx)?,
                "stats" => {
                    let launcher = self.launcher.clone().ok_or("no window yet")?;
                    let line = if argument == "reset" {
                        launcher.update(cx, |launcher, _| launcher.samples.clear());
                        "ok reset".to_owned()
                    } else {
                        summarize(&launcher.read(cx).samples)
                    };
                    send(&request.reply, line);
                }
                "background" => {
                    let background = parse_background(argument)
                        .ok_or_else(|| format!("unknown background {argument}"))?;
                    self.options.background = background;
                    if let Some(window) = self.window {
                        window.update(cx, |_, window, _| {
                            window.set_background_appearance(background);
                            window.refresh();
                        })?;
                    }
                    send(&request.reply, "ok".into());
                }
                "pid" => send(&request.reply, format!("ok {}", std::process::id())),
                "quit" => {
                    send(&request.reply, "ok".into());
                    cx.quit();
                }
                _ => send(&request.reply, format!("error unknown command {name}")),
            }
            Ok(())
        }

        fn edit(
            &mut self,
            name: &str,
            argument: &str,
            request: &Request,
            cx: &mut App,
        ) -> Fallible {
            let window = self.window.ok_or("show the window first")?;
            let edits: Vec<Edit> = match name {
                "type" => argument.chars().map(Edit::Insert).collect(),
                "back" => vec![Edit::Backspace],
                _ => vec![Edit::Clear],
            };
            if edits.is_empty() {
                return Err("type needs text".into());
            }
            let last = edits.len() - 1;
            window.update(cx, |launcher, _, cx| {
                for (index, edit) in edits.iter().enumerate() {
                    let reply = (index == last).then(|| request.reply.clone());
                    launcher.edit(edit, request.received, reply, cx);
                }
            })?;
            Ok(())
        }
    }

    /// Median, 95th percentile and maximum of each timing, in microseconds.
    fn summarize(samples: &[KeySample]) -> String {
        let mut line = format!("ok keys={}", samples.len());
        for (name, pick) in [
            (
                "search",
                (|s: &KeySample| s.search) as fn(&KeySample) -> Duration,
            ),
            ("draw", |s| s.draw),
            ("frame", |s| s.frame),
        ] {
            let mut values: Vec<u128> = samples.iter().map(|s| pick(s).as_micros()).collect();
            values.sort_unstable();
            let _ = write!(
                line,
                " {name}_us=p50:{},p95:{},max:{}",
                percentile(&values, 50),
                percentile(&values, 95),
                values.last().copied().unwrap_or(0)
            );
        }
        line
    }

    /// The `pct`th percentile (nearest rank) of sorted values; 0 when there are none.
    fn percentile(sorted: &[u128], pct: usize) -> u128 {
        if sorted.is_empty() {
            return 0;
        }
        let rank = (sorted.len() * pct).div_ceil(100).max(1);
        sorted[rank - 1]
    }

    /// Accepts connections and forwards each line to the main thread.
    fn listen(listener: &UnixListener, requests: &async_channel::Sender<Request>) {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let requests = requests.clone();
            std::thread::spawn(move || serve_connection(stream, &requests));
        }
    }

    fn serve_connection(stream: UnixStream, requests: &async_channel::Sender<Request>) {
        let Ok(mut writer) = stream.try_clone() else {
            return;
        };
        for line in BufReader::new(stream).lines() {
            let Ok(command) = line else { return };
            let received = Instant::now();
            let (reply, answer) = mpsc::channel();
            let request = Request {
                command,
                received,
                reply,
            };
            if requests.send_blocking(request).is_err() {
                return;
            }
            let line = answer
                .recv()
                .unwrap_or_else(|_| "error no answer".to_owned());
            if writeln!(writer, "{line}").is_err() {
                return;
            }
        }
    }

    pub(crate) fn serve(args: &[String]) -> Fallible {
        let options = parse_serve(args)?;
        // A stale socket from an earlier run would make `bind` fail.
        let _ = std::fs::remove_file(&options.socket);
        let listener = UnixListener::bind(&options.socket)?;
        let (requests, incoming) = async_channel::unbounded::<Request>();
        std::thread::spawn(move || listen(&listener, &requests));
        gpui_platform::application()
            .with_quit_mode(QuitMode::Explicit)
            .run(move |cx: &mut App| {
                cx.set_activation_policy(gpui::ActivationPolicy::Accessory);
                let mut spike = Spike {
                    options,
                    window: None,
                    launcher: None,
                    visible: false,
                };
                cx.spawn(async move |cx| {
                    while let Ok(request) = incoming.recv().await {
                        let reply = request.reply.clone();
                        if let Err(error) = cx.update(|cx| spike.handle(request, cx)) {
                            send(&reply, format!("error {error}"));
                        }
                    }
                })
                .detach();
            });
        Ok(())
    }

    /// Sends one command and waits for its answer.
    fn ask(
        stream: &mut UnixStream,
        reader: &mut BufReader<UnixStream>,
        command: &str,
    ) -> Fallible<String> {
        writeln!(stream, "{command}")?;
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end().to_owned();
        if line.starts_with("ok") {
            Ok(line)
        } else {
            Err(format!("{command}: {line}").into())
        }
    }

    /// The value of `name=<n>` in an answer.
    fn field(line: &str, name: &str) -> Option<u128> {
        line.split_whitespace()
            .find_map(|part| part.strip_prefix(name)?.strip_prefix('=')?.parse().ok())
    }

    fn connect(socket: &str) -> Fallible<(UnixStream, BufReader<UnixStream>)> {
        let stream = UnixStream::connect(socket)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok((stream, reader))
    }

    pub(crate) fn send_command(args: &[String]) -> Fallible {
        let socket = args.first().ok_or("send needs a socket path")?;
        let command = args[1..].join(" ");
        let (mut stream, mut reader) = connect(socket)?;
        let line = ask(&mut stream, &mut reader, &command)?;
        writeln!(std::io::stdout().lock(), "{line}")?;
        Ok(())
    }

    /// Toggles the window `runs` times and types a query one key per frame, then prints the
    /// median, 95th percentile and maximum of each timing.
    pub(crate) fn measure(args: &[String]) -> Fallible {
        let socket = args.first().ok_or("measure needs a socket path")?;
        let runs: usize = args.get(1).map_or(Ok(50), |runs| runs.parse())?;
        let (mut stream, mut reader) = connect(socket)?;
        let mut out = std::io::stdout().lock();

        let mut round_trip = Vec::new();
        let mut in_app = Vec::new();
        let mut show_draw = Vec::new();
        for _ in 0..runs {
            let started = Instant::now();
            let line = ask(&mut stream, &mut reader, "show")?;
            round_trip.push(started.elapsed().as_micros());
            in_app.push(field(&line, "frame_us").unwrap_or(0));
            show_draw.push(field(&line, "draw_us").unwrap_or(0));
            std::thread::sleep(Duration::from_millis(150));
            ask(&mut stream, &mut reader, "hide")?;
            std::thread::sleep(Duration::from_millis(250));
        }
        report(&mut out, "show: client round trip", &mut round_trip)?;
        report(&mut out, "show: command to presented frame", &mut in_app)?;
        report(&mut out, "show: draw of that frame", &mut show_draw)?;

        ask(&mut stream, &mut reader, "show")?;
        let mut search = Vec::new();
        let mut draw = Vec::new();
        let mut frame = Vec::new();
        let mut work = Vec::new();
        let mut results = Vec::new();
        for _ in 0..runs.div_ceil(10).max(1) {
            for text in ["firefox", "safari notes", "term", "code studio", "x"] {
                ask(&mut stream, &mut reader, "clear")?;
                for character in text.chars() {
                    let line = ask(&mut stream, &mut reader, &format!("type {character}"))?;
                    let s = field(&line, "search_us").unwrap_or(0);
                    let d = field(&line, "draw_us").unwrap_or(0);
                    search.push(s);
                    draw.push(d);
                    work.push(s + d);
                    frame.push(field(&line, "frame_us").unwrap_or(0));
                    results.push(field(&line, "results").unwrap_or(0));
                }
            }
        }
        ask(&mut stream, &mut reader, "hide")?;
        report(&mut out, "type: rank 5,000 items", &mut search)?;
        report(&mut out, "type: draw and present", &mut draw)?;
        report(&mut out, "type: rank + draw (frame budget)", &mut work)?;
        report(&mut out, "type: key to presented frame", &mut frame)?;
        report(&mut out, "type: results per keystroke", &mut results)?;
        Ok(())
    }

    fn report(out: &mut impl Write, name: &str, values: &mut [u128]) -> Fallible {
        values.sort_unstable();
        writeln!(
            out,
            "{name}: n={} p50={} p95={} max={} (us)",
            values.len(),
            percentile(values, 50),
            percentile(values, 95),
            values.last().copied().unwrap_or(0)
        )?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn highlight_ranges_merge_adjacent_characters() {
            assert_eq!(highlight_ranges("Firefox", &[0, 1, 2, 4]), vec![0..3, 4..5]);
        }

        #[test]
        fn highlight_ranges_are_byte_ranges() {
            assert_eq!(highlight_ranges("Café au", &[3, 5]), vec![3..5, 6..7]);
        }

        #[test]
        fn percentile_uses_nearest_rank() {
            let values: Vec<u128> = (1..=100).collect();
            assert_eq!(percentile(&values, 50), 50);
            assert_eq!(percentile(&values, 95), 95);
            assert_eq!(percentile(&[7], 95), 7);
            assert_eq!(percentile(&[], 50), 0);
        }

        #[test]
        fn fake_items_are_unique() {
            let items = FakeItems::new(5_000).items;
            let mut titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
            titles.sort_unstable();
            titles.dedup();
            assert_eq!(titles.len(), 5_000);
        }
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    use std::io::Write as _;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("serve") => spike::serve(&args[1..]),
        Some("measure") => spike::measure(&args[1..]),
        Some("send") => spike::send_command(&args[1..]),
        _ => Err("usage: spike serve|measure|send <socket> …".into()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(std::io::stderr().lock(), "spike: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// The spike drives the window over a Unix socket.
#[cfg(not(unix))]
fn main() {}
