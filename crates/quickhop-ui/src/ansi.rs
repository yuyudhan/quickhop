//! Colour in a script's full output (SCR-07): ANSI SGR sequences turned into plain text plus
//! coloured byte ranges, which the window draws. Only the colours are kept; every other escape
//! sequence (cursor moves, bold, underline) is dropped from the text.

use std::ops::Range;

/// Text with the ranges drawn in one of the 16 ANSI colours.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Styled {
    /// The text without escape sequences.
    pub text: String,
    /// Byte ranges of [`Self::text`] and their colour, in order, never overlapping.
    pub spans: Vec<(Range<usize>, Colour)>,
}

/// One of the 16 ANSI colours: 0 to 7 normal (black, red, green, yellow, blue, magenta, cyan,
/// white), 8 to 15 bright.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colour(pub u8);

/// Splits `text` into plain text and coloured ranges. Foreground colours 30-37, 90-97 and
/// `38;5;n` for n below 16 are kept; 39 and 0 end them; other parameters, and every escape
/// sequence that is not SGR, are dropped.
pub fn parse(text: &str) -> Styled {
    let mut styled = Styled::default();
    let mut colour: Option<(usize, Colour)> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            styled.text.push(c);
            continue;
        }
        if chars.peek() != Some(&'[') {
            // A two-character escape (ESC c, ESC 7, ...): drop its second character.
            chars.next();
            continue;
        }
        chars.next();
        let mut parameters = String::new();
        let mut last = None;
        for c in chars.by_ref() {
            if ('\u{40}'..='\u{7e}').contains(&c) {
                last = Some(c);
                break;
            }
            parameters.push(c);
        }
        if last != Some('m') {
            continue;
        }
        let next = sgr_colour(&parameters, colour.map(|(_, c)| c));
        if next != colour.map(|(_, c)| c) {
            let at = styled.text.len();
            if let Some((start, old)) = colour.take()
                && start < at
            {
                styled.spans.push((start..at, old));
            }
            colour = next.map(|c| (at, c));
        }
    }
    if let Some((start, old)) = colour
        && start < styled.text.len()
    {
        styled.spans.push((start..styled.text.len(), old));
    }
    styled
}

/// The foreground colour after the SGR parameters `parameters`, starting from `current`.
fn sgr_colour(parameters: &str, current: Option<Colour>) -> Option<Colour> {
    let mut colour = current;
    let mut values = parameters.split(';').map(|p| p.parse::<u16>().unwrap_or(0));
    while let Some(value) = values.next() {
        colour = match value {
            0 | 39 => None,
            30..=37 => Some(Colour(u8::try_from(value - 30).unwrap_or(0))),
            90..=97 => Some(Colour(u8::try_from(value - 90 + 8).unwrap_or(0))),
            38 => match values.next() {
                Some(5) => match values.next() {
                    Some(index @ 0..=15) => Some(Colour(u8::try_from(index).unwrap_or(0))),
                    _ => colour,
                },
                Some(2) => {
                    // A 24-bit colour: its three components are skipped.
                    values.nth(2);
                    colour
                }
                _ => colour,
            },
            // Backgrounds with their extended forms, so their numbers are not read as colours.
            48 => {
                match values.next() {
                    Some(5) => {
                        values.next();
                    }
                    Some(2) => {
                        values.nth(2);
                    }
                    _ => {}
                }
                colour
            }
            _ => colour,
        };
    }
    colour
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Colour = Colour(1);
    const GREEN: Colour = Colour(2);

    #[test]
    fn plain_text_has_no_spans() {
        assert_eq!(
            parse("hello\nworld"),
            Styled {
                text: "hello\nworld".into(),
                spans: Vec::new()
            }
        );
    }

    #[test]
    fn colours_cover_their_text_until_reset() {
        let styled = parse("a \u{1b}[31mred\u{1b}[0m and \u{1b}[32mgreen\u{1b}[39m.");
        assert_eq!(styled.text, "a red and green.");
        assert_eq!(styled.spans, vec![(2..5, RED), (10..15, GREEN)]);
    }

    #[test]
    fn a_colour_change_ends_the_previous_span_and_unclosed_ones_run_to_the_end() {
        let styled = parse("\u{1b}[31mré\u{1b}[1;32mgo");
        assert_eq!(styled.text, "régo");
        assert_eq!(styled.spans, vec![(0..3, RED), (3..5, GREEN)]);
    }

    #[test]
    fn bright_and_indexed_colours_are_kept_and_others_ignored() {
        assert_eq!(parse("\u{1b}[91mx").spans, vec![(0..1, Colour(9))]);
        assert_eq!(parse("\u{1b}[38;5;12mx").spans, vec![(0..1, Colour(12))]);
        // A 256-colour index past 15 and a 24-bit colour keep the current colour.
        assert_eq!(parse("\u{1b}[38;5;200mx").spans, Vec::new());
        assert_eq!(parse("\u{1b}[38;2;1;2;3mx").spans, Vec::new());
        // Background numbers are not read as foreground colours.
        assert_eq!(parse("\u{1b}[48;5;31mx").spans, Vec::new());
        assert_eq!(parse("\u{1b}[41mx").spans, Vec::new());
    }

    #[test]
    fn other_escape_sequences_are_dropped() {
        assert_eq!(
            parse("\u{1b}[2Kclear\u{1b}cline\u{1b}[1mbold").text,
            "clearlinebold"
        );
        // An escape cut off at the end of the output.
        assert_eq!(parse("end\u{1b}[3").text, "end");
    }

    #[test]
    fn empty_coloured_runs_leave_no_span() {
        assert_eq!(parse("\u{1b}[31m\u{1b}[32m\u{1b}[0mtext").spans, Vec::new());
    }
}
