//! The view model of the settings window (CFG-01), toolkit-free so every OS tests it: the rows
//! it lists (the hotkey, launch at login, the menu-bar icon, the theme, results per search, the
//! providers, clipboard history and hidden files, then the config file for everything else),
//! the keyboard and mouse input on them, and the hotkey recorder. Each input that changes a
//! setting yields a [`Change`] for the backend, which writes `config.toml` and reloads quickhop;
//! [`SettingsModel::applied`] takes what it read back.

use quickhop_core::config::MAX_RESULTS_LIMIT;
use quickhop_core::settings::{PROVIDERS, Setting, provider};
use quickhop_core::{Config, Hotkey, Theme};

/// What the settings show, as the backend read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Values {
    /// The configuration in `config.toml`.
    pub config: Config,
    /// Whether quickhop starts at login, or why that cannot be told or changed here.
    pub launch_at_login: Result<bool, String>,
    /// The config file, for the user to edit what the window has no row for.
    pub config_path: String,
}

/// What the user asked to change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A key in `config.toml`.
    Setting(Setting),
    /// Start quickhop at login, or stop doing so.
    LaunchAtLogin(bool),
    /// Open `config.toml` in the default text editor.
    OpenConfigFile,
}

/// One row of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// `hotkey`, with the recorder.
    Hotkey,
    /// Launch at login.
    LaunchAtLogin,
    /// `menu_bar_icon`.
    MenuBarIcon,
    /// `theme`.
    Theme,
    /// `max_results`.
    MaxResults,
    /// The `[providers]` switch at this index of [`PROVIDERS`].
    Provider(usize),
    /// `clipboard.enabled`.
    ClipboardHistory,
    /// `files.hidden`.
    HiddenFiles,
    /// The button that opens `config.toml`.
    OpenConfigFile,
}

/// How a row's value is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A switch, on or off.
    Switch(bool),
    /// One of several choices, changed with the arrows: the theme.
    Choice(String),
    /// A number changed with the arrows.
    Number(usize),
    /// The hotkey, or the recorder's prompt while it listens.
    Shortcut {
        /// The hotkey as written, or what to press.
        text: String,
        /// Whether the recorder listens.
        recording: bool,
    },
    /// A button.
    Button(String),
    /// Nothing can be changed here: why.
    Unavailable(String),
}

/// A line under the rows: what came of the last change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The text.
    pub text: String,
    /// Whether it reports a failure.
    pub failure: bool,
}

/// The modifiers held with a key the recorder hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Held {
    /// `⌘` / Super.
    pub command: bool,
    /// Ctrl.
    pub control: bool,
    /// Alt / Option.
    pub alt: bool,
    /// Shift.
    pub shift: bool,
}

impl Held {
    fn any(self) -> bool {
        self.command || self.control || self.alt || self.shift
    }
}

/// The hotkey for `key` (the toolkit's key name, e.g. `k`, `space`, `f13`) pressed with
/// `held`, as [`Hotkey::parse`] reads it.
///
/// # Errors
/// What to press instead, for the user.
pub fn hotkey_from(key: &str, held: Held) -> Result<Hotkey, String> {
    let mut text = String::new();
    for (on, name) in [
        (held.command, "Cmd+"),
        (held.control, "Ctrl+"),
        (held.alt, "Alt+"),
        (held.shift, "Shift+"),
    ] {
        if on {
            text.push_str(name);
        }
    }
    text.push_str(key);
    Hotkey::parse(&text).map_err(|error| match error {
        quickhop_core::HotkeyError::NeedsModifier(_) => {
            "Hold Cmd, Ctrl, Alt or Shift with that key, or use a function key".to_owned()
        }
        _ => format!("{key:?} cannot be a hotkey: press modifiers and one key"),
    })
}

/// The settings window's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsModel {
    values: Result<Values, String>,
    selected: usize,
    recording: bool,
    message: Option<Message>,
}

impl SettingsModel {
    /// The model over `values` as the backend read them, or why it could not (an invalid
    /// `config.toml`: only the button that opens it is left).
    pub fn new(values: Result<Values, String>) -> Self {
        let message = values.as_ref().err().map(|error| Message {
            text: format!("{error}. Fix the file, then save it."),
            failure: true,
        });
        Self {
            values,
            selected: 0,
            recording: false,
            message,
        }
    }

    /// The values shown, when the config file could be read.
    pub fn values(&self) -> Option<&Values> {
        self.values.as_ref().ok()
    }

    /// The rows, top to bottom.
    pub fn rows(&self) -> Vec<Row> {
        if self.values.is_err() {
            return vec![Row::OpenConfigFile];
        }
        let mut rows = vec![
            Row::Hotkey,
            Row::LaunchAtLogin,
            Row::MenuBarIcon,
            Row::Theme,
            Row::MaxResults,
        ];
        rows.extend((0..PROVIDERS.len()).map(Row::Provider));
        rows.extend([Row::ClipboardHistory, Row::HiddenFiles, Row::OpenConfigFile]);
        rows
    }

    /// The heading above `row` when it starts a group.
    pub fn heading(row: Row) -> Option<&'static str> {
        match row {
            Row::Hotkey => Some("General"),
            Row::Provider(0) => Some("Search"),
            Row::ClipboardHistory => Some("Clipboard and files"),
            Row::OpenConfigFile => Some("Everything else"),
            _ => None,
        }
    }

    /// The row's label.
    pub fn label(row: Row) -> &'static str {
        match row {
            Row::Hotkey => "quickhop hotkey",
            Row::LaunchAtLogin => "Launch at login",
            Row::MenuBarIcon => "Show in menu bar",
            Row::Theme => "Theme",
            Row::MaxResults => "Results per search",
            Row::Provider(index) => PROVIDERS.get(index).map_or("", |(_, label)| label),
            Row::ClipboardHistory => "Clipboard history",
            Row::HiddenFiles => "Index hidden files",
            Row::OpenConfigFile => "Config file",
        }
    }

    /// The line under the row's label.
    pub fn hint(&self, row: Row) -> String {
        match row {
            Row::Hotkey => "Shows and hides quickhop. Click, then press the new keys".into(),
            Row::LaunchAtLogin => "Start quickhop when you log in".into(),
            Row::MenuBarIcon => "quickhop's icon with Open, Settings and Quit".into(),
            Row::Theme => "System follows light and dark mode".into(),
            Row::MaxResults => format!("1 to {MAX_RESULTS_LIMIT}; ← and → change it"),
            Row::Provider(index) => PROVIDERS
                .get(index)
                .map(|(key, _)| format!("providers.{key}"))
                .unwrap_or_default(),
            Row::ClipboardHistory => "Record copied text".into(),
            Row::HiddenFiles => "Files and folders whose names start with a dot".into(),
            Row::OpenConfigFile => self.values.as_ref().map_or_else(
                |_| "config.toml".to_owned(),
                |values| {
                    format!(
                        "{}: quicklinks, aliases, folders and the rest; saved changes apply",
                        values.config_path
                    )
                },
            ),
        }
    }

    /// The row's value.
    pub fn value(&self, row: Row) -> Value {
        let Ok(values) = &self.values else {
            return Value::Button("Open in Editor".into());
        };
        let config = &values.config;
        match row {
            Row::Hotkey if self.recording => Value::Shortcut {
                text: "Press the new hotkey… (Esc cancels)".into(),
                recording: true,
            },
            Row::Hotkey => Value::Shortcut {
                text: config.hotkey.clone(),
                recording: false,
            },
            Row::LaunchAtLogin => match &values.launch_at_login {
                Ok(on) => Value::Switch(*on),
                Err(why) => Value::Unavailable(why.clone()),
            },
            Row::MenuBarIcon => Value::Switch(config.menu_bar_icon),
            Row::Theme => Value::Choice(theme_label(config.theme).into()),
            Row::MaxResults => Value::Number(config.max_results),
            Row::Provider(index) => Value::Switch(
                PROVIDERS
                    .get(index)
                    .and_then(|(key, _)| provider(config, key))
                    .unwrap_or(false),
            ),
            Row::ClipboardHistory => Value::Switch(config.clipboard.enabled),
            Row::HiddenFiles => Value::Switch(config.files.hidden),
            Row::OpenConfigFile => Value::Button("Open in Editor".into()),
        }
    }

    /// The selected row's index in [`SettingsModel::rows`].
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Whether the hotkey recorder listens.
    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// What came of the last change, if anything.
    pub fn message(&self) -> Option<&Message> {
        self.message.as_ref()
    }

    /// Selects the next row, staying on the last.
    pub fn select_next(&mut self) {
        self.recording = false;
        self.selected = (self.selected + 1).min(self.rows().len().saturating_sub(1));
    }

    /// Selects the previous row, staying on the first.
    pub fn select_previous(&mut self) {
        self.recording = false;
        self.selected = self.selected.saturating_sub(1);
    }

    /// A click on the row at `index`: selects it and acts as `Return` on it.
    pub fn click(&mut self, index: usize) -> Option<Change> {
        if index >= self.rows().len() {
            return None;
        }
        if self.selected != index {
            self.recording = false;
        }
        self.selected = index;
        self.activate()
    }

    /// `Return` or `Space` on the selected row: flips a switch, moves to the next theme,
    /// starts (or stops) the hotkey recorder, opens the config file.
    pub fn activate(&mut self) -> Option<Change> {
        let row = *self.rows().get(self.selected)?;
        if row == Row::Hotkey {
            self.recording = !self.recording;
            return None;
        }
        self.recording = false;
        match (row, self.value(row)) {
            (Row::OpenConfigFile, _) => Some(Change::OpenConfigFile),
            (Row::Theme, _) => self.theme_step(true),
            (row, Value::Switch(on)) => Self::switch(row, !on),
            _ => None,
        }
    }

    /// `→` (`forward`) or `←` on the selected row: a number up or down, the next or previous
    /// theme, a switch on or off.
    pub fn step(&mut self, forward: bool) -> Option<Change> {
        let row = *self.rows().get(self.selected)?;
        self.recording = false;
        match (row, self.value(row)) {
            (Row::MaxResults, Value::Number(count)) => {
                let next = if forward {
                    (count + 1).min(MAX_RESULTS_LIMIT)
                } else {
                    count.saturating_sub(1).max(1)
                };
                (next != count).then_some(Change::Setting(Setting::MaxResults(next)))
            }
            (Row::Theme, _) => self.theme_step(forward),
            (row, Value::Switch(on)) if on != forward => Self::switch(row, forward),
            _ => None,
        }
    }

    fn theme_step(&self, forward: bool) -> Option<Change> {
        let current = self.values().map(|values| values.config.theme)?;
        let all = Theme::ALL;
        let at = all.iter().position(|theme| *theme == current).unwrap_or(0);
        let next = if forward {
            (at + 1) % all.len()
        } else {
            (at + all.len() - 1) % all.len()
        };
        Some(Change::Setting(Setting::Theme(all[next])))
    }

    fn switch(row: Row, on: bool) -> Option<Change> {
        Some(match row {
            Row::LaunchAtLogin => Change::LaunchAtLogin(on),
            Row::MenuBarIcon => Change::Setting(Setting::MenuBarIcon(on)),
            Row::Provider(index) => {
                Change::Setting(Setting::Provider(PROVIDERS.get(index)?.0.into(), on))
            }
            Row::ClipboardHistory => Change::Setting(Setting::ClipboardHistory(on)),
            Row::HiddenFiles => Change::Setting(Setting::HiddenFiles(on)),
            _ => return None,
        })
    }

    /// A key pressed while the recorder listens: `Esc` alone stops it, a valid hotkey ends it
    /// with the change, anything else says what to press. `None` when it did not listen or
    /// nothing changes yet.
    pub fn record(&mut self, key: &str, held: Held) -> Option<Change> {
        if !self.recording {
            return None;
        }
        if key == "escape" && !held.any() {
            self.recording = false;
            return None;
        }
        match hotkey_from(key, held) {
            Ok(hotkey) => {
                self.recording = false;
                let unchanged = self
                    .values()
                    .is_some_and(|values| Setting::Hotkey(hotkey).is_set_in(&values.config));
                (!unchanged).then_some(Change::Setting(Setting::Hotkey(hotkey)))
            }
            Err(why) => {
                self.message = Some(Message {
                    text: why,
                    failure: true,
                });
                None
            }
        }
    }

    /// What the backend answered to `change`: the values read back, or why it failed (the
    /// shown values stay as they were).
    pub fn applied(&mut self, change: &Change, outcome: Result<Values, String>) {
        self.message = Some(match outcome {
            Ok(values) => {
                let text = match change {
                    Change::OpenConfigFile => format!("Opened {}", values.config_path),
                    Change::LaunchAtLogin(true) => "quickhop starts at login".into(),
                    Change::LaunchAtLogin(false) => "quickhop no longer starts at login".into(),
                    Change::Setting(setting) => format!("Saved {}", setting.key()),
                };
                self.values = Ok(values);
                Message {
                    text,
                    failure: false,
                }
            }
            Err(text) => Message {
                text,
                failure: true,
            },
        });
        self.selected = self.selected.min(self.rows().len().saturating_sub(1));
    }

    /// The values read again (the window came back to the front): changes made in the file
    /// meanwhile show.
    pub fn refreshed(&mut self, values: Result<Values, String>) {
        let had_error = self.values.is_err();
        self.values = values;
        if let Err(error) = &self.values {
            self.message = Some(Message {
                text: format!("{error}. Fix the file, then save it."),
                failure: true,
            });
        } else if had_error {
            self.message = None;
        }
        self.selected = self.selected.min(self.rows().len().saturating_sub(1));
    }
}

/// How the settings name `theme`.
pub fn theme_label(theme: Theme) -> &'static str {
    match theme {
        Theme::System => "System",
        Theme::Dark => "Dark",
        Theme::Light => "Light",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> Values {
        Values {
            config: Config::default(),
            launch_at_login: Ok(false),
            config_path: "/home/u/.config/quickhop/config.toml".into(),
        }
    }

    fn model() -> SettingsModel {
        SettingsModel::new(Ok(values()))
    }

    fn select(model: &mut SettingsModel, row: Row) {
        let index = model.rows().iter().position(|r| *r == row).unwrap();
        while model.selected() < index {
            model.select_next();
        }
    }

    #[test]
    fn the_rows_cover_the_raycast_basics_then_the_file() {
        let rows = model().rows();
        assert_eq!(
            rows[..5],
            [
                Row::Hotkey,
                Row::LaunchAtLogin,
                Row::MenuBarIcon,
                Row::Theme,
                Row::MaxResults
            ]
        );
        assert_eq!(rows.last(), Some(&Row::OpenConfigFile));
        assert_eq!(rows.len(), 5 + PROVIDERS.len() + 3);
        assert_eq!(SettingsModel::label(Row::Provider(1)), "Calculator");
    }

    #[test]
    fn an_invalid_file_leaves_only_the_editor_button() {
        let mut model = SettingsModel::new(Err("invalid config: max_results".into()));
        assert_eq!(model.rows(), vec![Row::OpenConfigFile]);
        assert!(model.message().is_some_and(|m| m.failure));
        assert_eq!(model.activate(), Some(Change::OpenConfigFile));
        model.refreshed(Ok(values()));
        assert_eq!(model.message(), None, "fixing the file clears the error");
        assert_eq!(model.rows().len(), 5 + PROVIDERS.len() + 3);
    }

    #[test]
    fn return_flips_switches_and_cycles_the_theme() {
        let mut model = model();
        select(&mut model, Row::MenuBarIcon);
        assert_eq!(
            model.activate(),
            Some(Change::Setting(Setting::MenuBarIcon(false)))
        );
        let mut model = SettingsModel::new(Ok(values()));
        select(&mut model, Row::LaunchAtLogin);
        assert_eq!(model.activate(), Some(Change::LaunchAtLogin(true)));
        select(&mut model, Row::Theme);
        assert_eq!(
            model.activate(),
            Some(Change::Setting(Setting::Theme(Theme::Dark)))
        );
        select(&mut model, Row::Provider(2));
        assert_eq!(
            model.activate(),
            Some(Change::Setting(Setting::Provider("emoji".into(), false)))
        );
    }

    #[test]
    fn arrows_step_numbers_themes_and_switches() {
        let mut model = model();
        select(&mut model, Row::MaxResults);
        assert_eq!(
            model.step(true),
            Some(Change::Setting(Setting::MaxResults(9)))
        );
        assert_eq!(
            model.step(false),
            Some(Change::Setting(Setting::MaxResults(7)))
        );
        let mut low = values();
        low.config.max_results = 1;
        let mut model = SettingsModel::new(Ok(low));
        select(&mut model, Row::MaxResults);
        assert_eq!(model.step(false), None, "never below 1");
        model.select_previous();
        assert_eq!(
            model.step(false),
            Some(Change::Setting(Setting::Theme(Theme::Light))),
            "← from System wraps to Light"
        );
        select(&mut model, Row::ClipboardHistory);
        assert_eq!(model.step(true), None, "already on");
        assert_eq!(
            model.step(false),
            Some(Change::Setting(Setting::ClipboardHistory(false)))
        );
    }

    #[test]
    fn the_recorder_turns_keys_into_a_hotkey() {
        let mut model = model();
        assert_eq!(model.record("k", Held::default()), None, "not listening");
        assert_eq!(model.click(0), None);
        assert!(model.is_recording());
        assert!(matches!(
            model.value(Row::Hotkey),
            Value::Shortcut {
                recording: true,
                ..
            }
        ));
        assert_eq!(model.record("k", Held::default()), None);
        assert!(model.is_recording(), "a bare letter is refused");
        assert!(model.message().is_some_and(|m| m.failure));
        let held = Held {
            control: true,
            shift: true,
            ..Held::default()
        };
        assert_eq!(
            model.record("k", held),
            Some(Change::Setting(Setting::Hotkey(
                Hotkey::parse("Ctrl+Shift+K").unwrap()
            )))
        );
        assert!(!model.is_recording());
    }

    #[test]
    fn the_recorder_stops_on_escape_and_ignores_the_current_hotkey() {
        let mut model = model();
        model.activate();
        assert_eq!(model.record("escape", Held::default()), None);
        assert!(!model.is_recording());
        model.activate();
        let current = Hotkey::parse(&Config::default().hotkey).unwrap();
        let held = Held {
            command: current.modifiers.command,
            control: current.modifiers.control,
            alt: current.modifiers.alt,
            shift: current.modifiers.shift,
        };
        assert_eq!(model.record("space", held), None, "the same hotkey");
        assert!(!model.is_recording());
        assert_eq!(
            hotkey_from("f13", Held::default()).map(|h| h.to_string()),
            Ok("F13".into())
        );
        assert!(hotkey_from("capslock", Held::default()).is_err());
    }

    #[test]
    fn applied_changes_show_and_failures_keep_the_values() {
        let mut model = model();
        let change = Change::Setting(Setting::MenuBarIcon(false));
        let mut saved = values();
        saved.config.menu_bar_icon = false;
        model.applied(&change, Ok(saved));
        assert_eq!(model.value(Row::MenuBarIcon), Value::Switch(false));
        assert_eq!(
            model.message().map(|m| m.text.as_str()),
            Some("Saved menu_bar_icon")
        );
        model.applied(&Change::LaunchAtLogin(true), Err("launchctl failed".into()));
        assert_eq!(model.value(Row::LaunchAtLogin), Value::Switch(false));
        assert!(model.message().is_some_and(|m| m.failure));
    }

    #[test]
    fn launch_at_login_that_cannot_be_told_is_not_a_switch() {
        let mut unknown = values();
        unknown.launch_at_login = Err("not supported on windows yet".into());
        let mut model = SettingsModel::new(Ok(unknown));
        select(&mut model, Row::LaunchAtLogin);
        assert!(matches!(
            model.value(Row::LaunchAtLogin),
            Value::Unavailable(_)
        ));
        assert_eq!(model.activate(), None);
    }
}
