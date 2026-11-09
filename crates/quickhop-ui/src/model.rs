//! The launcher's view model: what the window shows and how input changes it, with no toolkit
//! types, so the behaviour is tested headless (ADR 0001).
//!
//! The window feeds input in ([`Model::set_query`], [`Model::select_next`], [`Model::escape`],
//! …), runs the searches the model asks for and hands their results back with
//! [`Model::set_results`], and carries out the [`Effect`]s the model returns. Searches are
//! numbered: results for anything but the newest search of the page on top are dropped, so a
//! slow search can never overwrite the results of a later keystroke.

use quickhop_core::{
    Action, Item, ItemId, Matcher, Query, Ranked, ScriptArgument, ScriptArgumentKind, ScriptMode,
};

/// A search the window must run; its results come back through [`Model::set_results`] with the
/// same `generation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    /// Number of this search; newer searches have larger numbers.
    pub generation: u64,
    /// The page the search is for: `None` for the root search, else the command that pushed it.
    pub source: Option<ItemId>,
    /// What to search for.
    pub query: Query,
}

/// What the window must do after the model handled an input, besides redrawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Nothing.
    None,
    /// Hide the window.
    Hide,
    /// Run this search and hand its results to [`Model::set_results`].
    Search(SearchRequest),
    /// Carry out `action` of `item`, chosen while `query` was typed (for frecency).
    Perform {
        /// The chosen item.
        item: Box<Item>,
        /// The chosen action, one of `item.actions`.
        action: Action,
        /// Values for a script's arguments, in order; empty for every other action.
        args: Vec<String>,
        /// The query typed when it was chosen.
        query: Query,
    },
}

/// What a page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageKind {
    /// The root search over every provider.
    Root,
    /// A searchable list pushed by a command (clipboard history, a script's items, …).
    List {
        /// The command that pushed it; searches of this page name it as their source.
        command: ItemId,
    },
    /// A read-only text view (a script's full output).
    Detail {
        /// The text shown.
        text: String,
    },
}

/// One level of the navigation stack: a view with its own query, results and selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// Shown in the search bar's place for pushed pages; empty for the root.
    pub title: String,
    /// What the page shows.
    pub kind: PageKind,
    query: String,
    generation: u64,
    results: Vec<Ranked>,
    selected: Option<usize>,
    /// The item to keep selected when the newest search's results come ([`Model::refresh`]).
    keep: Option<ItemId>,
}

impl Page {
    /// A page that has not searched yet.
    pub fn new(title: impl Into<String>, kind: PageKind) -> Self {
        Self {
            title: title.into(),
            kind,
            query: String::new(),
            generation: 0,
            results: Vec::new(),
            selected: None,
            keep: None,
        }
    }

    /// The text typed on this page.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The results of the page's newest search, best first.
    pub fn results(&self) -> &[Ranked] {
        &self.results
    }

    /// Index of the selected result; `None` exactly when there are no results.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    fn is_searchable(&self) -> bool {
        !matches!(self.kind, PageKind::Detail { .. })
    }

    fn source(&self) -> Option<ItemId> {
        match &self.kind {
            PageKind::List { command } => Some(command.clone()),
            PageKind::Root | PageKind::Detail { .. } => None,
        }
    }
}

/// One entry of the action panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelEntry {
    /// The action's label.
    pub title: String,
    /// Index of the action in the item's [`Item::actions`].
    pub action: usize,
    /// Matched character indices of `title`, for highlighting.
    pub positions: Vec<usize>,
}

/// The action panel (`⌘K` / `Ctrl+K`): every action of one item, filtered by what is typed
/// while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionPanel {
    item: Item,
    filter: String,
    entries: Vec<PanelEntry>,
    selected: Option<usize>,
}

impl ActionPanel {
    fn new(item: Item) -> Self {
        let mut panel = Self {
            item,
            filter: String::new(),
            entries: Vec::new(),
            selected: None,
        };
        panel.refilter();
        panel
    }

    /// The item whose actions are listed.
    pub fn item(&self) -> &Item {
        &self.item
    }

    /// The text typed while the panel is open.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// The actions matching the filter: in the item's order when the filter is empty, else best
    /// match first.
    pub fn entries(&self) -> &[PanelEntry] {
        &self.entries
    }

    /// Index into [`Self::entries`] of the selected entry; `None` exactly when none match.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    fn refilter(&mut self) {
        let mut matcher = Matcher::new(&self.filter);
        let mut scored: Vec<(u32, PanelEntry)> = self
            .item
            .actions
            .iter()
            .enumerate()
            .filter_map(|(index, action)| {
                let title = action_title(action);
                let found = matcher.score(&title)?;
                Some((
                    found.score,
                    PanelEntry {
                        title,
                        action: index,
                        positions: found.positions,
                    },
                ))
            })
            .collect();
        // Stable: equal scores (every score, for an empty filter) keep the item's order.
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        self.entries = scored.into_iter().map(|(_, entry)| entry).collect();
        self.selected = (!self.entries.is_empty()).then_some(0);
    }
}

/// How a toast looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastStyle {
    /// Something finished.
    Success,
    /// Something failed.
    Failure,
    /// Something is still running.
    Progress,
}

/// A short message at the bottom of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    /// How it looks.
    pub style: ToastStyle,
    /// The message.
    pub title: String,
}

/// What came of an action, for the window to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to show.
    Done,
    /// A short message in the window, which stays shown (a compact script's last line).
    Toast(String),
    /// A text read on a page of its own (a full-output script's output, ANSI colour kept).
    Detail {
        /// The page's title.
        title: String,
        /// The text.
        text: String,
    },
    /// A short message shown on its own after the window hid (a silent script, a copy).
    Hud(String),
    /// The item's new subtitle (an inline script run by hand).
    Subtitle(String),
}

/// Whether the window stays shown while `action` runs, to show what comes of it: scripts in
/// `fullOutput`, `compact` and `inline` mode. Every other action hides the window first, so it
/// reaches the app the user came from.
pub fn keeps_window(action: &Action) -> bool {
    matches!(
        action,
        Action::Script {
            mode: ScriptMode::FullOutput | ScriptMode::Compact | ScriptMode::Inline,
            ..
        }
    )
}

/// The launcher's state: a navigation stack of pages (the root search at the bottom), the
/// action panel when open, a script's argument fields while it asks for them, and the last
/// toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    stack: Vec<Page>,
    panel: Option<ActionPanel>,
    form: Option<ArgumentForm>,
    toast: Option<Toast>,
    generation: u64,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    /// A root page with an empty query that has not searched yet; call [`Self::reset`] to get
    /// its first search.
    pub fn new() -> Self {
        Self {
            stack: vec![Page::new("", PageKind::Root)],
            panel: None,
            form: None,
            toast: None,
            generation: 0,
        }
    }

    /// Back to an empty root search, as when the window is shown again: pops every pushed page,
    /// closes the panel, the argument fields and the toast, and asks for the root's search.
    pub fn reset(&mut self) -> Effect {
        self.stack.truncate(1);
        self.panel = None;
        self.form = None;
        self.toast = None;
        self.set_page_query(String::new())
    }

    /// The page on top of the stack: the one shown.
    pub fn page(&self) -> &Page {
        // The stack always holds the root page.
        &self.stack[self.stack.len() - 1]
    }

    fn page_mut(&mut self) -> &mut Page {
        let top = self.stack.len() - 1;
        &mut self.stack[top]
    }

    /// Every page, root first.
    pub fn stack(&self) -> &[Page] {
        &self.stack
    }

    /// The action panel, when open.
    pub fn panel(&self) -> Option<&ActionPanel> {
        self.panel.as_ref()
    }

    /// The last toast, until it is dismissed or the model is reset.
    pub fn toast(&self) -> Option<&Toast> {
        self.toast.as_ref()
    }

    /// The selected result of the page on top, if any.
    pub fn selected_item(&self) -> Option<&Item> {
        let page = self.page();
        page.selected.map(|index| &page.results[index].item)
    }

    /// The argument fields, while a script asks for its arguments.
    pub fn form(&self) -> Option<&ArgumentForm> {
        self.form.as_ref()
    }

    /// The search bar's text changed. With the action panel open it filters the actions; with
    /// argument fields open it is the focused field's new text; otherwise the page searches
    /// again (detail pages have nothing to search). Typing a space after the alias of a script
    /// that asks for arguments opens its fields instead.
    pub fn set_query(&mut self, text: impl Into<String>) -> Effect {
        let text = text.into();
        if let Some(panel) = &mut self.panel {
            panel.filter = text;
            panel.refilter();
            return Effect::None;
        }
        if let Some(form) = &mut self.form {
            form.values[form.focused] = text;
            return Effect::None;
        }
        if !self.page().is_searchable() || self.page().query == text {
            return Effect::None;
        }
        if let Some(form) = self.alias_form(&text) {
            self.form = Some(form);
            return Effect::None;
        }
        self.set_page_query(text)
    }

    /// The argument fields to open when `text` is the page's query plus a space and the query
    /// is an alias of the selected item, a script that asks for arguments.
    fn alias_form(&self, text: &str) -> Option<ArgumentForm> {
        let alias = text.strip_suffix(' ')?;
        if alias != self.page().query || alias.trim().is_empty() {
            return None;
        }
        let item = self.selected_item()?;
        if !item
            .aliases
            .iter()
            .any(|a| a.eq_ignore_ascii_case(alias.trim()))
        {
            return None;
        }
        ArgumentForm::new(item.clone(), item.default_action()?.clone())
    }

    fn set_page_query(&mut self, text: String) -> Effect {
        self.generation += 1;
        let generation = self.generation;
        let page = self.page_mut();
        page.query = text;
        page.generation = generation;
        page.keep = None;
        if !page.is_searchable() {
            return Effect::None;
        }
        Effect::Search(SearchRequest {
            generation,
            source: page.source(),
            query: Query::new(page.query.clone()),
        })
    }

    /// Searches the page on top again with its query (its items changed, such as an inline
    /// script's subtitle), keeping the selected item selected when it is still there.
    pub fn refresh(&mut self) -> Effect {
        let keep = self.selected_item().map(|item| item.id.clone());
        let effect = self.set_page_query(self.page().query.clone());
        self.page_mut().keep = keep;
        effect
    }

    /// Results of the search numbered `generation`. Dropped, returning `false`, unless that is
    /// the newest search of the page on top. The first result is selected, unless the search
    /// is a [`Self::refresh`] and the item selected before is among them.
    pub fn set_results(&mut self, generation: u64, results: Vec<Ranked>) -> bool {
        let page = self.page_mut();
        if page.generation != generation || !page.is_searchable() {
            return false;
        }
        let kept = page
            .keep
            .take()
            .and_then(|id| results.iter().position(|ranked| ranked.item.id == id));
        page.selected = kept.or((!results.is_empty()).then_some(0));
        page.results = results;
        true
    }

    /// Moves the selection down one entry (of the panel when open), wrapping to the first.
    /// With argument fields open it picks the focused dropdown's next choice instead.
    pub fn select_next(&mut self) {
        self.step(true);
    }

    /// Moves the selection up one entry (of the panel when open), wrapping to the last. With
    /// argument fields open it picks the focused dropdown's previous choice instead.
    pub fn select_previous(&mut self) {
        self.step(false);
    }

    fn step(&mut self, forward: bool) {
        let (selected, count) = if let Some(panel) = &mut self.panel {
            (&mut panel.selected, panel.entries.len())
        } else if let Some(form) = &mut self.form {
            form.step_choice(forward);
            return;
        } else {
            let page = self.page_mut();
            (&mut page.selected, page.results.len())
        };
        if let Some(index) = selected {
            *index = if forward {
                (*index + 1) % count
            } else {
                index.checked_sub(1).unwrap_or(count - 1)
            };
        }
    }

    /// `Tab` (`forward`) or `Shift+Tab`: focuses the next or previous argument field, wrapping.
    /// Returns whether argument fields are open.
    pub fn step_field(&mut self, forward: bool) -> bool {
        match &mut self.form {
            Some(form) if self.panel.is_none() => {
                form.step_field(forward);
                true
            }
            _ => false,
        }
    }

    /// Selects result `index` of the page on top (a click); out-of-range indices are ignored.
    pub fn select(&mut self, index: usize) {
        if self.panel.is_none() && self.form.is_none() && index < self.page().results.len() {
            self.page_mut().selected = Some(index);
        }
    }

    /// `Enter`: performs the selected panel action, or else the selected item's default action.
    /// A script that asks for arguments opens its fields first; with them open, `Enter` runs it
    /// once every required field is filled, else focuses the first one missing.
    pub fn activate(&mut self) -> Effect {
        let query = Query::new(self.page().query.clone());
        if let Some(panel) = self.panel.take() {
            let Some(selected) = panel.selected else {
                self.panel = Some(panel);
                return Effect::None;
            };
            let action = panel.item.actions[panel.entries[selected].action].clone();
            return self.perform_or_ask(panel.item, action, query);
        }
        if let Some(mut form) = self.form.take() {
            return match form.complete() {
                Ok(args) => Effect::Perform {
                    item: Box::new(form.item),
                    action: form.action,
                    args,
                    query,
                },
                Err(missing) => {
                    form.focused = missing;
                    self.form = Some(form);
                    Effect::None
                }
            };
        }
        let Some(item) = self.selected_item() else {
            return Effect::None;
        };
        let Some(action) = item.default_action().cloned() else {
            return Effect::None;
        };
        let item = item.clone();
        self.perform_or_ask(item, action, query)
    }

    /// Performs `action`: a view opens as a page of its own, a script asking for arguments
    /// opens its argument fields, anything else is for the window to carry out.
    fn perform_or_ask(&mut self, item: Item, action: Action, query: Query) -> Effect {
        if let Action::View { .. } = action {
            return self.push(Page::new(
                item.title.clone(),
                PageKind::List { command: item.id },
            ));
        }
        if script_arguments(&action).is_empty() {
            return Effect::Perform {
                item: Box::new(item),
                action,
                args: Vec::new(),
                query,
            };
        }
        self.form = ArgumentForm::new(item, action);
        Effect::None
    }

    /// `⌘K` / `Ctrl+K`: opens the action panel for the selected item, or closes it when open.
    /// Opening it closes the argument fields. Returns whether the panel is open afterwards.
    pub fn toggle_actions(&mut self) -> bool {
        if self.panel.take().is_some() {
            return false;
        }
        let Some(item) = self.selected_item() else {
            return false;
        };
        if item.actions.is_empty() {
            return false;
        }
        self.panel = Some(ActionPanel::new(item.clone()));
        self.form = None;
        true
    }

    /// `Esc`: closes the panel, else the argument fields, else pops the page on top, else (at
    /// the root) hides the window.
    pub fn escape(&mut self) -> Effect {
        if self.panel.take().is_some() || self.form.take().is_some() {
            return Effect::None;
        }
        if self.pop() {
            return Effect::None;
        }
        Effect::Hide
    }

    /// Pushes `page` (a command opened a view) and asks for its first search when it has one.
    pub fn push(&mut self, page: Page) -> Effect {
        self.panel = None;
        self.form = None;
        self.stack.push(page);
        let query = self.page().query.clone();
        self.set_page_query(query)
    }

    /// Pops the page on top; the root page is never popped. Returns whether a page was popped.
    pub fn pop(&mut self) -> bool {
        if self.stack.len() == 1 {
            return false;
        }
        self.panel = None;
        self.form = None;
        self.stack.pop();
        true
    }

    /// Shows `toast`, replacing the previous one.
    pub fn show_toast(&mut self, toast: Toast) {
        self.toast = Some(toast);
    }

    /// Removes the toast.
    pub fn dismiss_toast(&mut self) {
        self.toast = None;
    }

    /// `item` started running while the window stays shown: a progress toast says so.
    pub fn running(&mut self, item: &Item) {
        self.show_toast(Toast {
            style: ToastStyle::Progress,
            title: format!("Running {}…", item.title),
        });
    }

    /// Shows what came of an action of the item `id`: a toast, a detail page pushed, or a new
    /// subtitle on the rows showing the item. A progress toast left from
    /// [`Self::running`] goes. Returns what the window must do (a pushed page never searches).
    pub fn performed(&mut self, id: &ItemId, outcome: Outcome) -> Effect {
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| toast.style == ToastStyle::Progress)
        {
            self.toast = None;
        }
        match outcome {
            Outcome::Done | Outcome::Hud(_) => Effect::None,
            Outcome::Toast(message) => {
                if !message.is_empty() {
                    self.show_toast(Toast {
                        style: ToastStyle::Success,
                        title: message,
                    });
                }
                Effect::None
            }
            Outcome::Detail { title, text } => {
                self.push(Page::new(title, PageKind::Detail { text }))
            }
            Outcome::Subtitle(subtitle) => {
                for page in &mut self.stack {
                    for ranked in page.results.iter_mut().filter(|r| &r.item.id == id) {
                        ranked.item.subtitle = Some(subtitle.clone());
                    }
                }
                Effect::None
            }
        }
    }

    /// An action failed with `message`: a failure toast shows it.
    pub fn failed(&mut self, message: String) {
        self.show_toast(Toast {
            style: ToastStyle::Failure,
            title: message,
        });
    }
}

/// The label of an action in the action panel.
pub fn action_title(action: &Action) -> String {
    match action {
        Action::Open { target }
            if target.starts_with("http://") || target.starts_with("https://") =>
        {
            "Open in Browser"
        }
        Action::Open { .. } | Action::View { .. } => "Open",
        Action::Copy { .. } => "Copy to Clipboard",
        Action::Paste { .. } => "Paste",
        Action::Run { program, .. } if program == "qlmanage" => "Quick Look",
        Action::Run { .. } => "Run",
        Action::Script { .. } => "Run Script",
        Action::Snippet { .. } => "Copy Snippet",
        Action::Reveal { .. } => "Show in File Manager",
        Action::Trash { .. } => "Move to Trash",
    }
    .to_owned()
}

/// What the window shows, as `quickhop daemon --test-socket` reports it to the UI tests.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Snapshot {
    /// Whether the window is on screen.
    pub visible: bool,
    /// The search bar's text: the action panel's filter while it is open.
    pub query: String,
    /// Ids of the results of the page on top, best first.
    pub results: Vec<String>,
    /// Titles of those results.
    pub titles: Vec<String>,
    /// Subtitles of those results, empty for a result without one.
    pub subtitles: Vec<String>,
    /// Index of the selected result.
    pub selected: Option<usize>,
    /// Titles of the pages, root first (the root's is empty).
    pub stack: Vec<String>,
    /// Labels of the action panel's entries, when it is open.
    pub panel: Option<Vec<String>>,
    /// The toast's message, when one shows.
    pub toast: Option<String>,
    /// For each result, whether the window draws its icon: filled in by the window, which
    /// loads the icons; empty from the model alone.
    pub icons: Vec<bool>,
    /// The argument fields' text as shown (passwords masked), while a script asks for them.
    pub arguments: Option<Vec<String>>,
    /// Index of the argument field typing goes to, while a script asks for arguments.
    pub argument: Option<usize>,
    /// The text of the detail page on top (a full-output script's output), colour removed.
    pub detail: Option<String>,
    /// The HUD's message while one shows: filled in by the window, which shows it.
    pub hud: Option<String>,
    /// What the onboarding window says while it is open: filled in by the window.
    pub onboarding: Option<crate::onboarding::Status>,
}

impl Model {
    /// The snapshot of what the window shows; `visible` says whether it is on screen.
    pub fn snapshot(&self, visible: bool) -> Snapshot {
        let page = self.page();
        Snapshot {
            visible,
            query: self
                .panel
                .as_ref()
                .map_or_else(|| page.query(), ActionPanel::filter)
                .to_owned(),
            results: page
                .results
                .iter()
                .map(|ranked| ranked.item.id.to_string())
                .collect(),
            titles: page
                .results
                .iter()
                .map(|ranked| ranked.item.title.clone())
                .collect(),
            subtitles: page
                .results
                .iter()
                .map(|ranked| ranked.item.subtitle.clone().unwrap_or_default())
                .collect(),
            selected: page.selected,
            stack: self.stack.iter().map(|page| page.title.clone()).collect(),
            panel: self.panel.as_ref().map(|panel| {
                panel
                    .entries
                    .iter()
                    .map(|entry| entry.title.clone())
                    .collect()
            }),
            toast: self.toast.as_ref().map(|toast| toast.title.clone()),
            icons: Vec::new(),
            arguments: self
                .form
                .as_ref()
                .map(|form| (0..form.values.len()).map(|i| form.shown(i)).collect()),
            argument: self.form.as_ref().map(|form| form.focused),
            detail: match &page.kind {
                PageKind::Detail { text } => Some(crate::ansi::parse(text).text),
                PageKind::Root | PageKind::List { .. } => None,
            },
            hud: None,
            onboarding: None,
        }
    }

    /// What typing edits: the action panel's filter while it is open, else the focused argument
    /// field while a script asks for arguments, else the query of the page on top.
    pub fn bar_text(&self) -> &str {
        if let Some(panel) = &self.panel {
            return panel.filter();
        }
        match &self.form {
            Some(form) => &form.values[form.focused],
            None => self.page().query(),
        }
    }
}

/// The argument fields of a script command (SCR-02, LCH-12), shown after the search bar's
/// text; what is typed goes to the focused field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgumentForm {
    item: Item,
    action: Action,
    values: Vec<String>,
    focused: usize,
}

impl ArgumentForm {
    /// The form for `action` of `item`; `None` unless it is a script asking for arguments.
    fn new(item: Item, action: Action) -> Option<Self> {
        let count = script_arguments(&action).len();
        (count > 0).then(|| Self {
            item,
            action,
            values: vec![String::new(); count],
            focused: 0,
        })
    }

    /// The script command asking.
    pub fn item(&self) -> &Item {
        &self.item
    }

    /// What each field asks for.
    pub fn arguments(&self) -> &[ScriptArgument] {
        script_arguments(&self.action)
    }

    /// The field typing goes to.
    pub fn focused(&self) -> usize {
        self.focused
    }

    /// The text of field `index` as shown: a password's characters are masked.
    pub fn shown(&self, index: usize) -> String {
        let value = &self.values[index];
        match self.arguments()[index].kind {
            ScriptArgumentKind::Password => "•".repeat(value.chars().count()),
            ScriptArgumentKind::Text | ScriptArgumentKind::Dropdown => value.clone(),
        }
    }

    /// The values to run the script with, dropdowns resolved to the choice they name; or the
    /// index of the first field that is required and empty, or names no choice.
    fn complete(&self) -> Result<Vec<String>, usize> {
        self.arguments()
            .iter()
            .zip(&self.values)
            .enumerate()
            .map(|(index, (argument, value))| {
                if value.is_empty() {
                    return if argument.optional {
                        Ok(String::new())
                    } else {
                        Err(index)
                    };
                }
                match argument.kind {
                    ScriptArgumentKind::Dropdown => resolve_choice(&argument.choices, value)
                        .map(str::to_owned)
                        .ok_or(index),
                    ScriptArgumentKind::Text | ScriptArgumentKind::Password => Ok(value.clone()),
                }
            })
            .collect()
    }

    /// Focuses the next field (`forward`) or the previous one, wrapping.
    fn step_field(&mut self, forward: bool) {
        let count = self.values.len();
        self.focused = if forward {
            (self.focused + 1) % count
        } else {
            self.focused.checked_sub(1).unwrap_or(count - 1)
        };
    }

    /// Sets a focused dropdown to its next choice (`forward`) or previous one, wrapping, from
    /// the choice it names now. Returns `false` when the focused field is no dropdown.
    fn step_choice(&mut self, forward: bool) -> bool {
        let argument = &self.arguments()[self.focused];
        if argument.kind != ScriptArgumentKind::Dropdown || argument.choices.is_empty() {
            return false;
        }
        let choices = &argument.choices;
        let value = &self.values[self.focused];
        let current =
            resolve_choice(choices, value).and_then(|c| choices.iter().position(|x| x == c));
        let next = match (current, forward) {
            (None, true) => 0,
            (None, false) => choices.len() - 1,
            (Some(index), true) => (index + 1) % choices.len(),
            (Some(index), false) => index.checked_sub(1).unwrap_or(choices.len() - 1),
        };
        self.values[self.focused] = choices[next].clone();
        true
    }
}

fn script_arguments(action: &Action) -> &[ScriptArgument] {
    match action {
        Action::Script { arguments, .. } => arguments,
        _ => &[],
    }
}

/// The choice `typed` names: the one equal to it ignoring case, else the only one it starts
/// (ignoring case).
pub fn resolve_choice<'a>(choices: &'a [String], typed: &str) -> Option<&'a str> {
    let typed = typed.to_lowercase();
    if let Some(exact) = choices.iter().find(|c| c.to_lowercase() == typed) {
        return Some(exact);
    }
    let mut starting = choices
        .iter()
        .filter(|c| c.to_lowercase().starts_with(&typed));
    match (starting.next(), starting.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// Byte ranges of the characters at `positions` (character indices, as the fuzzy matcher
/// reports them) in `text`, adjacent ones merged: what a result row highlights.
pub fn highlight_ranges(text: &str, positions: &[usize]) -> Vec<std::ops::Range<usize>> {
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
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

#[cfg(test)]
mod tests {
    use super::*;
    use quickhop_core::Tier;

    fn item(key: &str, actions: Vec<Action>) -> Item {
        let mut item = Item::new(
            ItemId::new("test", key),
            key,
            Action::Open {
                target: format!("/Applications/{key}.app"),
            },
        );
        item.actions = actions;
        item
    }

    fn ranked(items: Vec<Item>) -> Vec<Ranked> {
        items
            .into_iter()
            .map(|item| Ranked {
                item,
                tier: Tier::Matched,
                score: 0,
                positions: Vec::new(),
            })
            .collect()
    }

    fn open(target: &str) -> Action {
        Action::Open {
            target: target.into(),
        }
    }

    fn copy(text: &str) -> Action {
        Action::Copy { text: text.into() }
    }

    fn three() -> Vec<Ranked> {
        ranked(vec![
            item(
                "Firefox",
                vec![open("/Applications/Firefox.app"), copy("Firefox")],
            ),
            item(
                "Finder",
                vec![open("/System/Library/CoreServices/Finder.app")],
            ),
            item("Fig", vec![open("https://fig.io"), copy("https://fig.io")]),
        ])
    }

    #[test]
    fn the_snapshot_reports_results_selection_panel_and_toast() {
        let mut model = Model::new();
        let Effect::Search(request) = model.set_query("fi") else {
            panic!("typing searches");
        };
        model.set_results(request.generation, three());
        model.select_next();
        let snapshot = model.snapshot(true);
        assert_eq!(
            snapshot,
            Snapshot {
                visible: true,
                query: "fi".into(),
                results: vec![
                    "test:Firefox".into(),
                    "test:Finder".into(),
                    "test:Fig".into()
                ],
                titles: vec!["Firefox".into(), "Finder".into(), "Fig".into()],
                subtitles: vec![String::new(); 3],
                selected: Some(1),
                stack: vec![String::new()],
                panel: None,
                toast: None,
                icons: Vec::new(),
                arguments: None,
                argument: None,
                detail: None,
                hud: None,
                onboarding: None,
            }
        );
        model.select_next();
        assert!(model.toggle_actions());
        model.set_query("cop");
        model.show_toast(Toast {
            style: ToastStyle::Success,
            title: "Copied".into(),
        });
        let snapshot = model.snapshot(false);
        // While the panel is open the bar shows its filter; the page keeps its query.
        assert_eq!(snapshot.query, "cop");
        assert_eq!(model.page().query(), "fi");
        assert_eq!(snapshot.panel, Some(vec!["Copy to Clipboard".into()]));
        assert_eq!(snapshot.toast.as_deref(), Some("Copied"));
        assert!(!snapshot.visible);
    }

    #[test]
    fn highlight_ranges_merge_adjacent_characters_as_byte_ranges() {
        assert_eq!(highlight_ranges("Firefox", &[0, 1, 2, 4]), vec![0..3, 4..5]);
        assert_eq!(highlight_ranges("Café au", &[3, 5]), vec![3..5, 6..7]);
        assert_eq!(
            highlight_ranges("Fig", &[]),
            Vec::<std::ops::Range<usize>>::new()
        );
    }

    /// A model whose root shows `results` for the query `text`.
    fn with_results(text: &str, results: Vec<Ranked>) -> Model {
        let mut model = Model::new();
        let Effect::Search(request) = model.set_query(text) else {
            unreachable!("a root query change always searches");
        };
        assert!(model.set_results(request.generation, results));
        model
    }

    fn generation(effect: &Effect) -> u64 {
        match effect {
            Effect::Search(request) => request.generation,
            other => unreachable!("expected a search, got {other:?}"),
        }
    }

    #[test]
    fn typing_asks_for_a_search_of_the_text() {
        let mut model = Model::new();
        let effect = model.set_query("fire");
        assert_eq!(
            effect,
            Effect::Search(SearchRequest {
                generation: 1,
                source: None,
                query: Query::new("fire"),
            })
        );
        assert_eq!(model.page().query(), "fire");
        assert_eq!(
            model.set_query("fire"),
            Effect::None,
            "unchanged text does not search"
        );
    }

    #[test]
    fn results_of_an_older_search_are_dropped() {
        let mut model = Model::new();
        let old = generation(&model.set_query("f"));
        let new = generation(&model.set_query("fi"));
        assert!(
            !model.set_results(old, three()),
            "the slow first search lost"
        );
        assert_eq!(model.page().results(), []);
        assert!(model.set_results(new, three()));
        assert_eq!(model.page().results().len(), 3);
        assert_eq!(model.page().selected(), Some(0));
    }

    #[test]
    fn selection_wraps_both_ways() {
        let mut model = with_results("f", three());
        model.select_previous();
        assert_eq!(
            model.page().selected(),
            Some(2),
            "up from the first wraps to the last"
        );
        model.select_next();
        assert_eq!(
            model.page().selected(),
            Some(0),
            "down from the last wraps to the first"
        );
        model.select_next();
        model.select_next();
        assert_eq!(model.selected_item().map(|i| i.title.as_str()), Some("Fig"));
    }

    #[test]
    fn selection_is_none_without_results_and_moving_it_is_harmless() {
        let mut model = with_results("zzz", Vec::new());
        model.select_next();
        model.select_previous();
        assert_eq!(model.page().selected(), None);
        assert_eq!(model.activate(), Effect::None);
        assert!(!model.toggle_actions());
    }

    #[test]
    fn new_results_select_the_first() {
        let mut model = with_results("f", three());
        model.select_next();
        let next = generation(&model.set_query("fi"));
        model.set_results(next, three());
        assert_eq!(model.page().selected(), Some(0));
    }

    #[test]
    fn clicking_selects_in_range_only() {
        let mut model = with_results("f", three());
        model.select(2);
        assert_eq!(model.page().selected(), Some(2));
        model.select(3);
        assert_eq!(model.page().selected(), Some(2));
    }

    #[test]
    fn enter_performs_the_default_action_with_the_query() {
        let mut model = with_results("fire", three());
        assert_eq!(
            model.activate(),
            Effect::Perform {
                item: Box::new(three()[0].item.clone()),
                action: open("/Applications/Firefox.app"),
                args: Vec::new(),
                query: Query::new("fire"),
            }
        );
    }

    #[test]
    fn action_panel_lists_every_action_of_the_selected_item_in_order() {
        let mut model = with_results("f", three());
        model.select(2);
        assert!(model.toggle_actions());
        let panel = model.panel().map(ActionPanel::entries).unwrap_or_default();
        let titles: Vec<&str> = panel.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Open in Browser", "Copy to Clipboard"]);
        assert!(!model.toggle_actions(), "⌘K again closes it");
        assert!(model.panel().is_none());
    }

    #[test]
    fn typing_with_the_panel_open_filters_actions_not_results() {
        let mut model = with_results("f", three());
        model.toggle_actions();
        assert_eq!(model.set_query("cop"), Effect::None);
        assert_eq!(model.page().query(), "f", "the page query is untouched");
        let panel = model.panel().map(ActionPanel::entries).unwrap_or_default();
        assert_eq!(panel.len(), 1);
        assert_eq!(panel[0].title, "Copy to Clipboard");
        assert_eq!(panel[0].action, 1);
        assert_eq!(panel[0].positions, vec![0, 1, 2]);
        model.set_query("nothing like it");
        assert_eq!(model.panel().and_then(ActionPanel::selected), None);
        assert_eq!(
            model.activate(),
            Effect::None,
            "no match: Enter does nothing"
        );
        assert!(model.panel().is_some(), "and the panel stays open");
    }

    #[test]
    fn panel_selection_wraps_and_enter_performs_it() {
        let mut model = with_results("fire", three());
        model.toggle_actions();
        model.select_previous();
        assert_eq!(model.panel().and_then(ActionPanel::selected), Some(1));
        assert_eq!(
            model.page().selected(),
            Some(0),
            "the list selection does not move"
        );
        let effect = model.activate();
        assert_eq!(
            effect,
            Effect::Perform {
                item: Box::new(three()[0].item.clone()),
                action: copy("Firefox"),
                args: Vec::new(),
                query: Query::new("fire"),
            }
        );
        assert!(model.panel().is_none(), "performing closes the panel");
    }

    #[test]
    fn escape_closes_the_panel_then_pops_then_hides() {
        let mut model = with_results("f", three());
        let pushed = generation(&model.push(Page::new(
            "Clipboard History",
            PageKind::List {
                command: ItemId::new("clipboard", "history"),
            },
        )));
        model.set_results(pushed, three());
        assert!(model.toggle_actions());
        assert_eq!(model.escape(), Effect::None);
        assert_eq!(
            model.stack().len(),
            2,
            "the first Esc only closed the panel"
        );
        assert_eq!(model.escape(), Effect::None);
        assert_eq!(model.stack().len(), 1);
        assert_eq!(model.page().query(), "f", "the root kept its query");
        assert_eq!(model.escape(), Effect::Hide);
        assert_eq!(model.stack().len(), 1, "the root is never popped");
    }

    #[test]
    fn pushed_list_pages_search_their_own_source() {
        let mut model = with_results("f", three());
        let command = ItemId::new("clipboard", "history");
        let effect = model.push(Page::new(
            "Clipboard History",
            PageKind::List {
                command: command.clone(),
            },
        ));
        let Effect::Search(request) = effect else {
            unreachable!("a pushed list searches at once, got {effect:?}");
        };
        assert_eq!(request.source, Some(command));
        assert_eq!(request.query, Query::new(""));
        assert_eq!(model.page().results(), []);
        assert!(model.set_results(request.generation, three()));
        assert_eq!(model.page().results().len(), 3);
    }

    #[test]
    fn return_on_a_view_command_opens_its_list_and_searches_it() {
        let history = ItemId::new("views", "clipboard");
        let command = Item::new(
            history.clone(),
            "Clipboard History",
            Action::View {
                view: quickhop_core::View::Clipboard,
            },
        );
        let mut model = with_results(
            "clip",
            vec![Ranked {
                item: command,
                tier: Tier::Matched,
                score: 0,
                positions: Vec::new(),
            }],
        );
        let effect = model.activate();
        let Effect::Search(request) = effect else {
            panic!("a view searches its list at once, got {effect:?}");
        };
        assert_eq!(request.source, Some(history));
        assert_eq!(request.query, Query::new(""), "the list starts unfiltered");
        assert_eq!(model.stack().len(), 2);
        assert_eq!(model.page().title, "Clipboard History");
        assert!(model.set_results(request.generation, three()));
        // Typing filters the list, not the root.
        let Effect::Search(typed) = model.set_query("fi") else {
            panic!("typing in a list searches it");
        };
        assert_eq!(typed.source, request.source);
        assert_eq!(model.escape(), Effect::None);
        assert_eq!(
            model.page().query(),
            "clip",
            "Esc goes back to the root's query"
        );
    }

    #[test]
    fn results_for_a_popped_page_are_dropped() {
        let mut model = with_results("f", three());
        let pushed = generation(&model.push(Page::new(
            "History",
            PageKind::List {
                command: ItemId::new("clipboard", "history"),
            },
        )));
        model.pop();
        assert!(!model.set_results(pushed, Vec::new()));
        assert_eq!(model.page().results().len(), 3);
    }

    #[test]
    fn detail_pages_ignore_typing() {
        let mut model = Model::new();
        let effect = model.push(Page::new(
            "hello",
            PageKind::Detail {
                text: "Hello, world".into(),
            },
        ));
        assert_eq!(effect, Effect::None);
        assert_eq!(model.set_query("x"), Effect::None);
        assert_eq!(model.page().query(), "");
    }

    #[test]
    fn reset_returns_to_an_empty_root() {
        let mut model = with_results("f", three());
        model.push(Page::new(
            "hello",
            PageKind::Detail {
                text: "Hello".into(),
            },
        ));
        model.show_toast(Toast {
            style: ToastStyle::Success,
            title: "Copied".into(),
        });
        let effect = model.reset();
        assert_eq!(model.stack().len(), 1);
        assert_eq!(model.page().query(), "");
        assert!(model.toast().is_none());
        let Effect::Search(request) = effect else {
            unreachable!("reset searches the empty root, got {effect:?}");
        };
        assert_eq!(request.query, Query::new(""));
        assert!(model.set_results(request.generation, three()));
    }

    #[test]
    fn a_toast_replaces_the_previous_one() {
        let mut model = Model::new();
        model.show_toast(Toast {
            style: ToastStyle::Progress,
            title: "Running".into(),
        });
        model.show_toast(Toast {
            style: ToastStyle::Failure,
            title: "exit status 1".into(),
        });
        assert_eq!(model.toast().map(|t| t.style), Some(ToastStyle::Failure));
        model.dismiss_toast();
        assert!(model.toast().is_none());
    }

    #[test]
    fn action_titles() {
        assert_eq!(
            action_title(&open("https://example.com")),
            "Open in Browser"
        );
        assert_eq!(action_title(&open("/Applications/Safari.app")), "Open");
        assert_eq!(action_title(&copy("x")), "Copy to Clipboard");
    }

    fn argument(kind: ScriptArgumentKind, placeholder: &str, optional: bool) -> ScriptArgument {
        ScriptArgument {
            kind,
            placeholder: placeholder.into(),
            optional,
            percent_encoded: false,
            choices: if kind == ScriptArgumentKind::Dropdown {
                vec!["rust".into(), "python".into(), "pyodide".into()]
            } else {
                Vec::new()
            },
        }
    }

    fn script(mode: ScriptMode, arguments: Vec<ScriptArgument>) -> Action {
        Action::Script {
            path: "/scripts/search.sh".into(),
            mode,
            cwd: None,
            arguments,
            confirm: false,
        }
    }

    /// `search.sh`: a required query, an optional site dropdown, an optional password.
    fn search_script() -> Item {
        let mut item = item(
            "search.sh",
            vec![script(
                ScriptMode::Compact,
                vec![
                    argument(ScriptArgumentKind::Text, "Query", false),
                    argument(ScriptArgumentKind::Dropdown, "Site", true),
                    argument(ScriptArgumentKind::Password, "Token", true),
                ],
            )],
        );
        item.aliases = vec!["sd".into()];
        item
    }

    fn type_text(model: &mut Model, text: &str) {
        for character in text.chars() {
            let typed = format!("{}{character}", model.bar_text());
            model.set_query(typed);
        }
    }

    #[test]
    fn enter_on_a_script_with_arguments_asks_for_them_then_runs_it_with_them() {
        let mut model = with_results("search", ranked(vec![search_script()]));
        assert_eq!(model.activate(), Effect::None, "the fields open first");
        assert_eq!(model.form().map(ArgumentForm::focused), Some(0));
        type_text(&mut model, "rust");
        assert_eq!(model.page().query(), "search", "typing goes to the field");
        assert!(model.step_field(true));
        type_text(&mut model, "py");
        // "py" starts two choices: no value, so Enter focuses the dropdown.
        assert_eq!(model.activate(), Effect::None);
        assert_eq!(model.form().map(ArgumentForm::focused), Some(1));
        type_text(&mut model, "t");
        assert!(model.step_field(true));
        type_text(&mut model, "s3cret");
        let snapshot = model.snapshot(true);
        assert_eq!(snapshot.query, "search");
        assert_eq!(
            snapshot.arguments,
            Some(vec!["rust".into(), "pyt".into(), "••••••".into()])
        );
        assert_eq!(snapshot.argument, Some(2));
        assert_eq!(
            model.activate(),
            Effect::Perform {
                item: Box::new(search_script()),
                action: search_script().actions[0].clone(),
                args: vec!["rust".into(), "python".into(), "s3cret".into()],
                query: Query::new("search"),
            }
        );
        assert!(model.form().is_none(), "running closes the fields");
    }

    #[test]
    fn a_required_field_left_empty_is_focused_and_optional_ones_run_empty() {
        let mut model = with_results("search", ranked(vec![search_script()]));
        model.activate();
        assert!(model.step_field(false), "Shift+Tab wraps to the last field");
        assert_eq!(model.form().map(ArgumentForm::focused), Some(2));
        assert_eq!(model.activate(), Effect::None);
        assert_eq!(model.form().map(ArgumentForm::focused), Some(0));
        type_text(&mut model, "docs");
        let Effect::Perform { args, .. } = model.activate() else {
            panic!("every required field is filled");
        };
        assert_eq!(args, vec!["docs".to_owned(), String::new(), String::new()]);
    }

    #[test]
    fn a_space_after_the_alias_opens_the_fields() {
        let mut model = with_results("sd", ranked(vec![search_script()]));
        assert_eq!(model.set_query("sd "), Effect::None);
        assert_eq!(model.page().query(), "sd");
        assert_eq!(model.form().map(ArgumentForm::focused), Some(0));
        type_text(&mut model, "x");
        assert_eq!(
            model.snapshot(true).arguments.map(|a| a[0].clone()),
            Some("x".into())
        );
        // Anything but the alias plus a space searches as usual.
        let mut model = with_results("se", ranked(vec![search_script()]));
        assert!(matches!(model.set_query("se "), Effect::Search(_)));
        assert!(model.form().is_none());
    }

    #[test]
    fn arrows_pick_dropdown_choices_and_escape_closes_the_fields_first() {
        let mut model = with_results("search", ranked(vec![search_script()]));
        model.activate();
        model.step_field(true);
        model.select_next();
        assert_eq!(model.bar_text(), "rust");
        model.select_next();
        model.select_next();
        model.select_next();
        assert_eq!(model.bar_text(), "rust", "wraps after the last choice");
        model.select_previous();
        assert_eq!(model.bar_text(), "pyodide");
        // On a text field the arrows change nothing.
        model.step_field(false);
        model.select_next();
        assert_eq!(model.bar_text(), "");
        assert_eq!(model.page().selected(), Some(0));
        assert_eq!(model.escape(), Effect::None);
        assert!(model.form().is_none());
        assert_eq!(model.escape(), Effect::Hide);
    }

    #[test]
    fn choices_resolve_exactly_or_by_their_only_prefix_ignoring_case() {
        let choices: Vec<String> = vec!["rust".into(), "python".into(), "pyodide".into()];
        assert_eq!(resolve_choice(&choices, "Rust"), Some("rust"));
        assert_eq!(resolve_choice(&choices, "pyt"), Some("python"));
        assert_eq!(resolve_choice(&choices, "py"), None);
        assert_eq!(resolve_choice(&choices, "go"), None);
        let overlapping: Vec<String> = vec!["py".into(), "python".into()];
        assert_eq!(resolve_choice(&overlapping, "py"), Some("py"));
    }

    #[test]
    fn outcomes_show_as_toast_detail_page_or_subtitle_and_end_the_progress_toast() {
        let inline = item("inline.sh", vec![script(ScriptMode::Inline, Vec::new())]);
        let mut model = with_results("i", ranked(vec![inline.clone()]));
        model.running(&inline);
        assert_eq!(model.toast().map(|t| t.style), Some(ToastStyle::Progress));
        assert_eq!(
            model.performed(&inline.id, Outcome::Subtitle("all good".into())),
            Effect::None
        );
        assert!(model.toast().is_none());
        assert_eq!(
            model.page().results()[0].item.subtitle.as_deref(),
            Some("all good")
        );
        model.performed(
            &inline.id,
            Outcome::Toast("searching rust on python".into()),
        );
        assert_eq!(
            model.snapshot(true).toast.as_deref(),
            Some("searching rust on python")
        );
        model.failed("boom".into());
        model.performed(&inline.id, Outcome::Done);
        assert_eq!(
            model.toast().map(|t| t.style),
            Some(ToastStyle::Failure),
            "only a progress toast goes"
        );
        let effect = model.performed(
            &inline.id,
            Outcome::Detail {
                title: "Hello World".into(),
                text: "\u{1b}[31mHello\u{1b}[0m\nWorld\n".into(),
            },
        );
        assert_eq!(effect, Effect::None, "a detail page never searches");
        let snapshot = model.snapshot(true);
        assert_eq!(snapshot.stack, vec![String::new(), "Hello World".into()]);
        assert_eq!(snapshot.detail.as_deref(), Some("Hello\nWorld\n"));
        assert_eq!(model.escape(), Effect::None);
        assert_eq!(model.snapshot(true).detail, None);
    }

    #[test]
    fn a_refresh_searches_again_and_keeps_the_selected_item() {
        let mut model = with_results("f", three());
        model.select_next();
        let effect = model.refresh();
        let Effect::Search(request) = &effect else {
            panic!("a refresh searches");
        };
        assert_eq!(request.query, Query::new("f"));
        // Finder, selected, moves from the second row to the last.
        let mut reordered = three();
        reordered.swap(1, 2);
        assert!(model.set_results(generation(&effect), reordered));
        assert_eq!(model.page().selected(), Some(2));
        assert_eq!(
            model.selected_item().map(|item| item.id.to_string()),
            Some("test:Finder".into())
        );
        // A search typed after the refresh selects the first result again.
        model.refresh();
        let typed = model.set_query("fi");
        assert!(model.set_results(generation(&typed), three()));
        assert_eq!(model.page().selected(), Some(0));
    }

    #[test]
    fn only_scripts_that_show_output_keep_the_window() {
        assert!(keeps_window(&script(ScriptMode::FullOutput, Vec::new())));
        assert!(keeps_window(&script(ScriptMode::Compact, Vec::new())));
        assert!(keeps_window(&script(ScriptMode::Inline, Vec::new())));
        assert!(!keeps_window(&script(ScriptMode::Silent, Vec::new())));
        assert!(!keeps_window(&open("/Applications/Safari.app")));
        assert!(!keeps_window(&copy("x")));
    }
}
