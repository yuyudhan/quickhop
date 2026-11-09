//! The launcher's view model: what the window shows and how input changes it, with no toolkit
//! types, so the behaviour is tested headless (ADR 0001).
//!
//! The window feeds input in ([`Model::set_query`], [`Model::select_next`], [`Model::escape`],
//! …), runs the searches the model asks for and hands their results back with
//! [`Model::set_results`], and carries out the [`Effect`]s the model returns. Searches are
//! numbered: results for anything but the newest search of the page on top are dropped, so a
//! slow search can never overwrite the results of a later keystroke.

use quickhop_core::{Action, Item, ItemId, Matcher, Query, Ranked};

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

/// The launcher's state: a navigation stack of pages (the root search at the bottom), the
/// action panel when open, and the last toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    stack: Vec<Page>,
    panel: Option<ActionPanel>,
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
            toast: None,
            generation: 0,
        }
    }

    /// Back to an empty root search, as when the window is shown again: pops every pushed page,
    /// closes the panel and the toast, and asks for the root's search.
    pub fn reset(&mut self) -> Effect {
        self.stack.truncate(1);
        self.panel = None;
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

    /// The search bar's text changed. With the action panel open it filters the actions;
    /// otherwise the page searches again (detail pages have nothing to search).
    pub fn set_query(&mut self, text: impl Into<String>) -> Effect {
        let text = text.into();
        if let Some(panel) = &mut self.panel {
            panel.filter = text;
            panel.refilter();
            return Effect::None;
        }
        if !self.page().is_searchable() || self.page().query == text {
            return Effect::None;
        }
        self.set_page_query(text)
    }

    fn set_page_query(&mut self, text: String) -> Effect {
        self.generation += 1;
        let generation = self.generation;
        let page = self.page_mut();
        page.query = text;
        page.generation = generation;
        if !page.is_searchable() {
            return Effect::None;
        }
        Effect::Search(SearchRequest {
            generation,
            source: page.source(),
            query: Query::new(page.query.clone()),
        })
    }

    /// Results of the search numbered `generation`. Dropped, returning `false`, unless that is
    /// the newest search of the page on top. The first result is selected.
    pub fn set_results(&mut self, generation: u64, results: Vec<Ranked>) -> bool {
        let page = self.page_mut();
        if page.generation != generation || !page.is_searchable() {
            return false;
        }
        page.selected = (!results.is_empty()).then_some(0);
        page.results = results;
        true
    }

    /// Moves the selection down one entry (of the panel when open), wrapping to the first.
    pub fn select_next(&mut self) {
        self.step(true);
    }

    /// Moves the selection up one entry (of the panel when open), wrapping to the last.
    pub fn select_previous(&mut self) {
        self.step(false);
    }

    fn step(&mut self, forward: bool) {
        let (selected, count) = if let Some(panel) = &mut self.panel {
            (&mut panel.selected, panel.entries.len())
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

    /// Selects result `index` of the page on top (a click); out-of-range indices are ignored.
    pub fn select(&mut self, index: usize) {
        if self.panel.is_none() && index < self.page().results.len() {
            self.page_mut().selected = Some(index);
        }
    }

    /// `Enter`: performs the selected panel action, or else the selected item's default action.
    pub fn activate(&mut self) -> Effect {
        let query = Query::new(self.page().query.clone());
        if let Some(panel) = self.panel.take() {
            let Some(selected) = panel.selected else {
                self.panel = Some(panel);
                return Effect::None;
            };
            let action = panel.item.actions[panel.entries[selected].action].clone();
            return Effect::Perform {
                item: Box::new(panel.item),
                action,
                query,
            };
        }
        let Some(item) = self.selected_item() else {
            return Effect::None;
        };
        let Some(action) = item.default_action().cloned() else {
            return Effect::None;
        };
        Effect::Perform {
            item: Box::new(item.clone()),
            action,
            query,
        }
    }

    /// `⌘K` / `Ctrl+K`: opens the action panel for the selected item, or closes it when open.
    /// Returns whether the panel is open afterwards.
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
        true
    }

    /// `Esc`: closes the panel, else pops the page on top, else (at the root) hides the window.
    pub fn escape(&mut self) -> Effect {
        if self.panel.take().is_some() {
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
}

/// The label of an action in the action panel.
pub fn action_title(action: &Action) -> String {
    match action {
        Action::Open { target }
            if target.starts_with("http://") || target.starts_with("https://") =>
        {
            "Open in Browser"
        }
        Action::Open { .. } => "Open",
        Action::Copy { .. } => "Copy to Clipboard",
        Action::Run { .. } => "Run",
        Action::Script { .. } => "Run Script",
    }
    .to_owned()
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
}
