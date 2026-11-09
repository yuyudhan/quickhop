//! Global hotkeys as written in `config.toml` (`hotkey = "Alt+Space"`, LCH-01, LCH-05): parsed
//! once here, so every platform registers the same combination and a typo is a config error.
//!
//! A hotkey is modifiers and one key joined by `+`, in any order and any case: `Alt+Space`,
//! `cmd+shift+k`, `Ctrl+Alt+F13`. `Cmd`, `Command`, `Super` and `Meta` name the same modifier
//! (`⌘` on macOS, the logo key elsewhere); `Alt`, `Option` and `Opt` name another. A key that
//! types a character needs a modifier, or the hotkey would take that character from every
//! app; function keys may stand alone.

use std::fmt;

/// Modifier keys held with the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    /// `⌘` on macOS, the logo key (`Super`) elsewhere.
    pub command: bool,
    /// `Ctrl`.
    pub control: bool,
    /// `Alt` (`⌥ Option` on macOS).
    pub alt: bool,
    /// `Shift`.
    pub shift: bool,
}

impl Modifiers {
    fn is_empty(self) -> bool {
        self == Self::default()
    }
}

/// The key of a hotkey, by its position on a US keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// `A` to `Z`, as the uppercase letter.
    Letter(char),
    /// `0` to `9`, as the digit.
    Digit(char),
    /// `F1` to `F20`.
    Function(u8),
    /// The space bar.
    Space,
    /// `Return` (`Enter`).
    Return,
    /// `Tab`.
    Tab,
    /// `Esc`.
    Escape,
    /// `Backspace` (`Delete` on a Mac keyboard).
    Backspace,
    /// Forward delete (`Del`).
    Delete,
    /// The up arrow.
    Up,
    /// The down arrow.
    Down,
    /// The left arrow.
    Left,
    /// The right arrow.
    Right,
    /// `Home`.
    Home,
    /// `End`.
    End,
    /// `Page Up`.
    PageUp,
    /// `Page Down`.
    PageDown,
    /// `-`.
    Minus,
    /// `=`.
    Equal,
    /// `[`.
    LeftBracket,
    /// `]`.
    RightBracket,
    /// `\`.
    Backslash,
    /// `;`.
    Semicolon,
    /// `'`.
    Quote,
    /// `,`.
    Comma,
    /// `.`.
    Period,
    /// `/`.
    Slash,
    /// `` ` ``.
    Backquote,
}

/// Named keys with their names in `config.toml` (lowercase; the first one is how
/// [`Hotkey`] prints it).
const NAMED: &[(Key, &[&str])] = &[
    (Key::Space, &["space"]),
    (Key::Return, &["return", "enter"]),
    (Key::Tab, &["tab"]),
    (Key::Escape, &["escape", "esc"]),
    (Key::Backspace, &["backspace"]),
    (Key::Delete, &["delete", "del"]),
    (Key::Up, &["up"]),
    (Key::Down, &["down"]),
    (Key::Left, &["left"]),
    (Key::Right, &["right"]),
    (Key::Home, &["home"]),
    (Key::End, &["end"]),
    (Key::PageUp, &["pageup"]),
    (Key::PageDown, &["pagedown"]),
    (Key::Minus, &["-", "minus"]),
    (Key::Equal, &["=", "equal"]),
    (Key::LeftBracket, &["[", "leftbracket"]),
    (Key::RightBracket, &["]", "rightbracket"]),
    (Key::Backslash, &["\\", "backslash"]),
    (Key::Semicolon, &[";", "semicolon"]),
    (Key::Quote, &["'", "quote"]),
    (Key::Comma, &[",", "comma"]),
    (Key::Period, &[".", "period"]),
    (Key::Slash, &["/", "slash"]),
    (Key::Backquote, &["`", "backquote"]),
];

impl Key {
    fn parse(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        let mut chars = lower.chars();
        if let (Some(only), None) = (chars.next(), chars.next()) {
            if only.is_ascii_lowercase() {
                return Some(Self::Letter(only.to_ascii_uppercase()));
            }
            if only.is_ascii_digit() {
                return Some(Self::Digit(only));
            }
        }
        if let Some(digits) = lower.strip_prefix('f')
            && !digits.starts_with('0')
            && let Ok(number @ 1..=20) = digits.parse::<u8>()
        {
            return Some(Self::Function(number));
        }
        NAMED
            .iter()
            .find(|(_, names)| names.contains(&lower.as_str()))
            .map(|(key, _)| *key)
    }

    /// Whether pressing the key alone types or edits text in the focused app.
    fn types(self) -> bool {
        !matches!(self, Self::Function(_))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Letter(c) | Self::Digit(c) => write!(f, "{c}"),
            Self::Function(n) => write!(f, "F{n}"),
            named => {
                let name = NAMED
                    .iter()
                    .find(|(key, _)| key == named)
                    .map_or("?", |(_, names)| names[0]);
                let mut chars = name.chars();
                match chars.next() {
                    Some(first) => write!(f, "{}{}", first.to_ascii_uppercase(), chars.as_str()),
                    None => Ok(()),
                }
            }
        }
    }
}

/// A global hotkey: modifiers and one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hotkey {
    /// The modifiers held.
    pub modifiers: Modifiers,
    /// The key pressed.
    pub key: Key,
}

/// Why a hotkey does not parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyError {
    /// Nothing, or an empty part (`Alt+`, `Alt++Space`).
    #[error("hotkey {0:?} has an empty part")]
    Empty(String),
    /// A part that is neither a modifier nor a key.
    #[error("hotkey {hotkey:?}: {part:?} is not a key or modifier")]
    Unknown {
        /// The whole hotkey.
        hotkey: String,
        /// The part not understood.
        part: String,
    },
    /// Two keys, a modifier twice, or modifiers only.
    #[error("hotkey {0:?} must be modifiers and exactly one key")]
    NotOneKey(String),
    /// A key that types text, without a modifier.
    #[error("hotkey {0:?} needs a modifier: alone, the key would stop typing in every app")]
    NeedsModifier(String),
}

impl Hotkey {
    /// Parses `text` as written in `config.toml`.
    ///
    /// # Errors
    /// [`HotkeyError`] when it is not modifiers and one key, or a typing key stands alone.
    pub fn parse(text: &str) -> Result<Self, HotkeyError> {
        let trimmed = text.trim();
        // `+` itself is no key here, so splitting on it is unambiguous.
        let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
        if parts.iter().any(|part| part.is_empty()) {
            return Err(HotkeyError::Empty(text.to_owned()));
        }
        let mut modifiers = Modifiers::default();
        let mut key = None;
        for part in parts {
            let held = match part.to_ascii_lowercase().as_str() {
                "cmd" | "command" | "super" | "meta" => &mut modifiers.command,
                "ctrl" | "control" => &mut modifiers.control,
                "alt" | "option" | "opt" => &mut modifiers.alt,
                "shift" => &mut modifiers.shift,
                _ => {
                    let parsed = Key::parse(part).ok_or_else(|| HotkeyError::Unknown {
                        hotkey: text.to_owned(),
                        part: part.to_owned(),
                    })?;
                    if key.replace(parsed).is_some() {
                        return Err(HotkeyError::NotOneKey(text.to_owned()));
                    }
                    continue;
                }
            };
            if std::mem::replace(held, true) {
                return Err(HotkeyError::NotOneKey(text.to_owned()));
            }
        }
        let key = key.ok_or_else(|| HotkeyError::NotOneKey(text.to_owned()))?;
        if modifiers.is_empty() && key.types() {
            return Err(HotkeyError::NeedsModifier(text.to_owned()));
        }
        Ok(Self { modifiers, key })
    }
}

impl fmt::Display for Hotkey {
    /// The canonical spelling: `Cmd`, `Ctrl`, `Alt`, `Shift` in that order, then the key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Modifiers {
            command,
            control,
            alt,
            shift,
        } = self.modifiers;
        for (held, name) in [
            (command, "Cmd"),
            (control, "Ctrl"),
            (alt, "Alt"),
            (shift, "Shift"),
        ] {
            if held {
                write!(f, "{name}+")?;
            }
        }
        write!(f, "{}", self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hotkey(text: &str) -> Hotkey {
        Hotkey::parse(text).unwrap_or_else(|error| panic!("{text}: {error}"))
    }

    #[test]
    fn modifiers_and_keys_parse_in_any_order_and_case() {
        let alt_space = Hotkey {
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::default()
            },
            key: Key::Space,
        };
        assert_eq!(hotkey("Alt+Space"), alt_space);
        assert_eq!(hotkey(" space + OPTION "), alt_space);
        assert_eq!(hotkey("opt+SPACE"), alt_space);

        let k = hotkey("shift+K+cmd");
        assert_eq!(k.key, Key::Letter('K'));
        assert!(k.modifiers.command && k.modifiers.shift);
        assert!(!k.modifiers.control && !k.modifiers.alt);
        assert_eq!(hotkey("Super+k"), hotkey("Command+K"));
        assert_eq!(hotkey("Meta+k"), hotkey("Cmd+k"));
        assert_eq!(hotkey("Control+5").key, Key::Digit('5'));
        assert_eq!(hotkey("Ctrl+Alt+F13").key, Key::Function(13));
        assert_eq!(hotkey("ctrl+/").key, Key::Slash);
        assert_eq!(hotkey("ctrl+esc"), hotkey("Ctrl+Escape"));
        assert_eq!(hotkey("alt+enter"), hotkey("Alt+Return"));
    }

    #[test]
    fn function_keys_may_stand_alone_typing_keys_may_not() {
        assert_eq!(hotkey("F5").modifiers, Modifiers::default());
        assert_eq!(hotkey("f20").key, Key::Function(20));
        for typing in ["Space", "k", "5", "Return", "Tab", "/", "Up", "Backspace"] {
            assert_eq!(
                Hotkey::parse(typing),
                Err(HotkeyError::NeedsModifier(typing.to_owned()))
            );
        }
    }

    #[test]
    fn malformed_hotkeys_are_refused_with_the_reason() {
        for empty in ["", " ", "Alt+", "+Space", "Alt++Space"] {
            assert_eq!(
                Hotkey::parse(empty),
                Err(HotkeyError::Empty(empty.to_owned())),
                "{empty:?}"
            );
        }
        for not_one in [
            "Alt",
            "Cmd+Shift",
            "Alt+A+B",
            "Alt+Option+Space",
            "Cmd+Super+K",
        ] {
            assert_eq!(
                Hotkey::parse(not_one),
                Err(HotkeyError::NotOneKey(not_one.to_owned())),
                "{not_one:?}"
            );
        }
        for (unknown, part) in [
            ("Hyper+Space", "Hyper"),
            ("Alt+F0", "F0"),
            ("Alt+F21", "F21"),
            ("Alt+F05", "F05"),
            ("Alt+é", "é"),
            ("Ctrl+Spacebar", "Spacebar"),
        ] {
            assert_eq!(
                Hotkey::parse(unknown),
                Err(HotkeyError::Unknown {
                    hotkey: unknown.to_owned(),
                    part: part.to_owned(),
                }),
                "{unknown:?}"
            );
        }
    }

    #[test]
    fn printing_gives_the_canonical_spelling_that_parses_back() {
        assert_eq!(hotkey("space+alt").to_string(), "Alt+Space");
        assert_eq!(
            hotkey("shift+alt+ctrl+cmd+k").to_string(),
            "Cmd+Ctrl+Alt+Shift+K"
        );
        assert_eq!(hotkey("ctrl+pageup").to_string(), "Ctrl+Pageup");
        assert_eq!(hotkey("f7").to_string(), "F7");
        let every_key = (b'a'..=b'z')
            .map(|c| char::from(c).to_string())
            .chain((0..=9).map(|d| d.to_string()))
            .chain((1..=20).map(|n| format!("F{n}")))
            .chain(NAMED.iter().map(|(_, names)| names[0].to_owned()));
        for key in every_key {
            let parsed = hotkey(&format!("Ctrl+{key}"));
            assert_eq!(hotkey(&parsed.to_string()), parsed, "{key}");
        }
    }
}
