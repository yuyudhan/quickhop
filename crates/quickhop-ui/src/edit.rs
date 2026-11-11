//! Deleting text in the search bar the way native text fields do: `Backspace` removes a
//! character, `Option+Backspace` (macOS) or `Ctrl+Backspace` (Linux) the word before the caret,
//! and `Cmd+Backspace` (macOS) everything before the caret on its line.
//!
//! Toolkit-free so the key conventions and word boundaries are tested on every OS; the window
//! maps its key events to [`Modifiers`] and applies the [`Deletion`] to the bar's text.

/// Whose text-editing key conventions to follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keymap {
    /// macOS: `Option` deletes words, `Cmd` deletes to the start of the line.
    Mac,
    /// Linux desktops (GTK, Qt): `Ctrl` deletes words.
    Linux,
}

impl Keymap {
    /// The conventions of the OS this is built for.
    pub const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Mac
    } else {
        Self::Linux
    };
}

/// The modifier keys held with a key press.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// `Option` on macOS, `Alt` elsewhere.
    pub alt: bool,
    /// `Control`.
    pub control: bool,
    /// `Cmd` on macOS, the Super/Windows key elsewhere.
    pub platform: bool,
}

/// How much text a deleting key removes before the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deletion {
    /// The character before the caret.
    Char,
    /// The word before the caret, with the spaces and punctuation between it and the caret.
    Word,
    /// Everything between the start of the caret's line and the caret.
    Line,
}

/// What `Backspace` held with `modifiers` deletes under `keymap`, or `None` when that
/// combination is not a text-editing key there. `Shift` never changes it.
#[must_use]
pub const fn backspace(keymap: Keymap, modifiers: Modifiers) -> Option<Deletion> {
    let Modifiers {
        alt,
        control,
        platform,
    } = modifiers;
    match keymap {
        Keymap::Mac => match (platform, alt, control) {
            (true, _, false) => Some(Deletion::Line),
            (false, true, false) => Some(Deletion::Word),
            // `Ctrl+Delete` deletes a character in Cocoa text fields.
            (false, false, _) => Some(Deletion::Char),
            _ => None,
        },
        Keymap::Linux => match (control, alt, platform) {
            (true, false, false) => Some(Deletion::Word),
            (false, false, false) => Some(Deletion::Char),
            _ => None,
        },
    }
}

/// Removes what `deletion` covers before byte offset `caret` in `text` and returns the new
/// caret. A caret past the end counts as the end, one inside a character as that character's
/// start.
pub fn delete_before(text: &mut String, caret: usize, deletion: Deletion) -> usize {
    let caret = char_boundary_at_or_before(text, caret);
    let before = &text[..caret];
    let start = match deletion {
        Deletion::Char => before
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index),
        Deletion::Word => word_start(before),
        Deletion::Line => before.rfind('\n').map_or(0, |newline| newline + 1),
    };
    text.replace_range(start..caret, "");
    start
}

/// The largest character boundary of `text` that is not after `offset`.
fn char_boundary_at_or_before(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Where the word ending `before` starts, as `Option+Backspace` sees it: back over the
/// whitespace and punctuation next to the caret, then over the word itself. An apostrophe
/// between letters (`don't`) belongs to the word.
fn word_start(before: &str) -> usize {
    let mut chars = before.char_indices().rev().peekable();
    while chars.next_if(|&(_, c)| !is_word_char(c)).is_some() {}
    let mut start = chars.peek().map_or(0, |&(index, c)| index + c.len_utf8());
    let mut after_word_char = false;
    while let Some((index, c)) = chars.next() {
        let in_word = is_word_char(c)
            || (after_word_char
                && is_apostrophe(c)
                && chars
                    .peek()
                    .is_some_and(|&(_, previous)| is_word_char(previous)));
        if !in_word {
            break;
        }
        after_word_char = is_word_char(c);
        start = index;
    }
    start
}

/// Letters, digits and `_` make up words; everything else separates them.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}')
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        alt: false,
        control: false,
        platform: false,
    };
    const ALT: Modifiers = Modifiers { alt: true, ..NONE };
    const CONTROL: Modifiers = Modifiers {
        control: true,
        ..NONE
    };
    const PLATFORM: Modifiers = Modifiers {
        platform: true,
        ..NONE
    };

    /// `text` after `deletion` with the caret at its end.
    fn delete_at_end(text: &str, deletion: Deletion) -> String {
        let mut text = text.to_owned();
        let caret = text.len();
        delete_before(&mut text, caret, deletion);
        text
    }

    /// `text` after `deletion` with the caret where `|` is, and the new caret marked the same
    /// way.
    fn delete_at_bar(marked: &str, deletion: Deletion) -> String {
        let caret = marked.find('|').unwrap_or(marked.len());
        let mut text = marked.replacen('|', "", 1);
        let caret = delete_before(&mut text, caret, deletion);
        text.insert(caret, '|');
        text
    }

    #[test]
    fn mac_option_backspace_deletes_a_word_and_cmd_backspace_the_line() {
        assert_eq!(backspace(Keymap::Mac, NONE), Some(Deletion::Char));
        assert_eq!(backspace(Keymap::Mac, ALT), Some(Deletion::Word));
        assert_eq!(backspace(Keymap::Mac, PLATFORM), Some(Deletion::Line));
        assert_eq!(
            backspace(
                Keymap::Mac,
                Modifiers {
                    alt: true,
                    ..PLATFORM
                }
            ),
            Some(Deletion::Line)
        );
        assert_eq!(backspace(Keymap::Mac, CONTROL), Some(Deletion::Char));
        assert_eq!(
            backspace(
                Keymap::Mac,
                Modifiers {
                    alt: true,
                    ..CONTROL
                }
            ),
            None
        );
    }

    #[test]
    fn linux_ctrl_backspace_deletes_a_word() {
        assert_eq!(backspace(Keymap::Linux, NONE), Some(Deletion::Char));
        assert_eq!(backspace(Keymap::Linux, CONTROL), Some(Deletion::Word));
        assert_eq!(backspace(Keymap::Linux, ALT), None);
        assert_eq!(backspace(Keymap::Linux, PLATFORM), None);
    }

    #[test]
    fn the_brief_example_types_hello_world_foo() {
        let text = "hello world foo";
        let word = delete_at_end(text, backspace(Keymap::Mac, ALT).unwrap_or(Deletion::Char));
        assert_eq!(word, "hello world ");
        let line = delete_at_end(
            &word,
            backspace(Keymap::Mac, PLATFORM).unwrap_or(Deletion::Char),
        );
        assert_eq!(line, "");
        let linux = delete_at_end(
            text,
            backspace(Keymap::Linux, CONTROL).unwrap_or(Deletion::Char),
        );
        assert_eq!(linux, "hello world ");
    }

    #[test]
    fn backspace_deletes_one_character() {
        assert_eq!(delete_at_end("hello", Deletion::Char), "hell");
        assert_eq!(delete_at_end("jalapeño", Deletion::Char), "jalapeñ");
        assert_eq!(delete_at_end("hi 👋", Deletion::Char), "hi ");
        assert_eq!(delete_at_bar("hello wor|ld", Deletion::Char), "hello wo|ld");
    }

    #[test]
    fn an_empty_field_stays_empty() {
        for deletion in [Deletion::Char, Deletion::Word, Deletion::Line] {
            assert_eq!(delete_at_end("", deletion), "");
            assert_eq!(delete_at_bar("|hello", deletion), "|hello");
        }
    }

    #[test]
    fn word_deletion_takes_trailing_spaces_with_the_word() {
        assert_eq!(delete_at_end("hello world   ", Deletion::Word), "hello ");
        assert_eq!(delete_at_end("hello", Deletion::Word), "");
        assert_eq!(delete_at_end("   ", Deletion::Word), "");
        assert_eq!(delete_at_end("a\tb ", Deletion::Word), "a\t");
    }

    #[test]
    fn word_deletion_with_the_caret_mid_word_keeps_the_rest() {
        assert_eq!(delete_at_bar("hello wo|rld", Deletion::Word), "hello |rld");
        assert_eq!(delete_at_bar("hello |world", Deletion::Word), "|world");
        assert_eq!(delete_at_bar("he|llo", Deletion::Line), "|llo");
    }

    #[test]
    fn punctuation_ends_words() {
        assert_eq!(delete_at_end("foo.bar", Deletion::Word), "foo.");
        assert_eq!(delete_at_end("foo-bar", Deletion::Word), "foo-");
        assert_eq!(delete_at_end("path/to/", Deletion::Word), "path/");
        assert_eq!(delete_at_end("hello, world!", Deletion::Word), "hello, ");
        assert_eq!(delete_at_end("...", Deletion::Word), "");
        assert_eq!(delete_at_end("snake_case", Deletion::Word), "");
        assert_eq!(delete_at_end("say don't", Deletion::Word), "say ");
        assert_eq!(delete_at_end("it's'", Deletion::Word), "");
        assert_eq!(delete_at_end("quote 'x", Deletion::Word), "quote '");
        assert_eq!(delete_at_end("naïve café", Deletion::Word), "naïve ");
    }

    #[test]
    fn line_deletion_stops_at_a_newline() {
        assert_eq!(delete_at_end("hello world foo", Deletion::Line), "");
        assert_eq!(delete_at_end("one\ntwo three", Deletion::Line), "one\n");
    }

    #[test]
    fn a_caret_off_a_boundary_is_clamped() {
        let mut text = String::from("jalapeño");
        assert_eq!(delete_before(&mut text, 100, Deletion::Char), 8);
        assert_eq!(text, "jalapeñ");
        let mut text = String::from("é");
        assert_eq!(delete_before(&mut text, 1, Deletion::Char), 0);
        assert_eq!(text, "é");
    }
}
