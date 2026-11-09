//! Raycast dynamic placeholders for snippets and quicklinks: `{clipboard}`,
//! `{snippet name="..."}`, `{cursor}`, `{date}` / `{time}` / `{datetime}` / `{day}` with
//! `format` and `offset`, `{uuid}`, `{selection}`, `{argument name= default= options=}`, and
//! the modifiers `| uppercase | lowercase | trim | percent-encode | json-stringify | raw`.
//!
//! Expansion is pure: the caller supplies the clock, clipboard history, selection, argument
//! values, other snippets and a UUID source in a [`Context`]. Anything that is not a valid
//! placeholder (unknown keyword, attribute or modifier, bad offset, missing snippet or
//! selection) stays in the text exactly as written.

use std::collections::HashMap;
use std::fmt::Write as _;

use jiff::Zoned;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::datefmt;

/// Most distinct `{argument}` inputs a template may ask for; later ones stay verbatim.
pub const MAX_ARGUMENTS: usize = 3;

/// What `percent-encode` leaves alone: JavaScript's `encodeURIComponent` set.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// An input a template asks for: one per distinct `{argument}` (same `name`, same input).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Argument {
    /// `name="..."`: placeholders with the same name share the value.
    pub name: Option<String>,
    /// `default="..."`: makes the argument optional.
    pub default: Option<String>,
    /// `options="a, b"`: the values to choose from (a dropdown).
    pub options: Vec<String>,
}

impl Argument {
    /// Whether a value must be entered.
    pub fn is_required(&self) -> bool {
        self.default.is_none()
    }
}

/// Everything expansion reads from the outside world.
pub struct Context<'a> {
    /// The current time, in the user's time zone.
    pub now: &'a Zoned,
    /// Clipboard history, newest first: `{clipboard offset=1}` is the second entry.
    pub clipboard: &'a [String],
    /// Selected text, when the caller has it; `{selection}` stays verbatim otherwise.
    pub selection: Option<&'a str>,
    /// Values for [`arguments`], in order; missing ones take their default or nothing.
    pub arguments: &'a [String],
    /// The text of another snippet by name, for `{snippet name="..."}`.
    pub snippet: &'a dyn Fn(&str) -> Option<String>,
    /// A fresh UUID per `{uuid}`.
    pub uuid: &'a dyn Fn() -> String,
}

/// The expanded text and where `{cursor}` was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expansion {
    /// The text with every valid placeholder replaced.
    pub text: String,
    /// Character index in `text` of the first `{cursor}`, if any.
    pub cursor: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Placeholder {
    keyword: String,
    attributes: Vec<(String, String)>,
    modifiers: Vec<String>,
}

impl Placeholder {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Whether the keyword, its attributes and modifiers are all known.
    fn is_valid(&self) -> bool {
        let allowed: &[&str] = match self.keyword.as_str() {
            "clipboard" => &["offset"],
            "snippet" => &["name"],
            "date" | "time" | "datetime" | "day" => &["format", "offset"],
            "argument" => &["name", "default", "options"],
            "cursor" | "uuid" | "selection" => &[],
            _ => return false,
        };
        let mut seen = Vec::new();
        let attributes_ok = self.attributes.iter().all(|(key, _)| {
            let fresh = !seen.contains(key);
            seen.push(key.clone());
            fresh && allowed.contains(&key.as_str())
        });
        let modifiers_ok = self.modifiers.iter().all(|m| {
            matches!(
                m.as_str(),
                "uppercase" | "lowercase" | "trim" | "percent-encode" | "json-stringify" | "raw"
            )
        });
        let required_ok = match self.keyword.as_str() {
            "snippet" => self.attribute("name").is_some(),
            "clipboard" => self
                .attribute("offset")
                .is_none_or(|o| o.parse::<usize>().is_ok()),
            _ => true,
        };
        attributes_ok && modifiers_ok && required_ok
    }
}

enum Segment<'t> {
    Text(&'t str),
    Placeholder(Placeholder, &'t str),
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Parses the placeholder starting at the `{` at the beginning of `text`; returns it and
/// its length in bytes, or `None` when `text` does not start with one.
fn parse_one(text: &str) -> Option<(Placeholder, usize)> {
    let mut rest = text.strip_prefix('{')?;
    let skip_ws = |s: &str| s.trim_start_matches([' ', '\t']).len();
    let consumed = |rest: &str| text.len() - rest.len();
    rest = &rest[rest.len() - skip_ws(rest)..];
    let word_len = rest.find(|c: char| !is_word(c)).unwrap_or(rest.len());
    if word_len == 0 {
        return None;
    }
    let keyword = rest[..word_len].to_owned();
    rest = &rest[word_len..];
    let mut attributes = Vec::new();
    let mut modifiers = Vec::new();
    loop {
        let trimmed = &rest[rest.len() - skip_ws(rest)..];
        if let Some(after) = trimmed.strip_prefix('}') {
            let placeholder = Placeholder {
                keyword,
                attributes,
                modifiers,
            };
            return Some((placeholder, consumed(after)));
        }
        if let Some(after) = trimmed.strip_prefix('|') {
            let after = &after[after.len() - skip_ws(after)..];
            let len = after.find(|c: char| !is_word(c)).unwrap_or(after.len());
            if len == 0 {
                return None;
            }
            modifiers.push(after[..len].to_owned());
            rest = &after[len..];
            continue;
        }
        // Attributes need whitespace before them; modifiers come after attributes.
        if trimmed.len() == rest.len() || !modifiers.is_empty() {
            return None;
        }
        let len = trimmed.find(|c: char| !is_word(c)).unwrap_or(trimmed.len());
        if len == 0 {
            return None;
        }
        let key = trimmed[..len].to_owned();
        let after = trimmed[len..].strip_prefix('=')?;
        let (value, after) = if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted.find('"')?;
            (&quoted[..end], &quoted[end + 1..])
        } else {
            let end = after
                .find(|c: char| c.is_whitespace() || matches!(c, '}' | '|' | '"' | '{'))
                .unwrap_or(after.len());
            if end == 0 {
                return None;
            }
            (&after[..end], &after[end..])
        };
        attributes.push((key, value.to_owned()));
        rest = after;
    }
}

fn segments(template: &str) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while let Some(offset) = template[at..].find('{') {
        let open = at + offset;
        match parse_one(&template[open..]) {
            Some((placeholder, len)) if placeholder.is_valid() => {
                if start < open {
                    out.push(Segment::Text(&template[start..open]));
                }
                out.push(Segment::Placeholder(
                    placeholder,
                    &template[open..open + len],
                ));
                start = open + len;
                at = start;
            }
            _ => at = open + 1,
        }
    }
    if start < template.len() {
        out.push(Segment::Text(&template[start..]));
    }
    out
}

/// Assigns each `{argument}` placeholder an input index, in order of appearance: named ones
/// share an index per name, unnamed ones get one each. `None` past [`MAX_ARGUMENTS`].
struct ArgumentSlots {
    by_name: HashMap<String, usize>,
    specs: Vec<Argument>,
}

impl ArgumentSlots {
    fn new() -> Self {
        Self {
            by_name: HashMap::new(),
            specs: Vec::new(),
        }
    }

    fn slot(&mut self, placeholder: &Placeholder) -> Option<usize> {
        let name = placeholder.attribute("name");
        if let Some(&index) = name.and_then(|n| self.by_name.get(n)) {
            return Some(index);
        }
        if self.specs.len() >= MAX_ARGUMENTS {
            return None;
        }
        let index = self.specs.len();
        self.specs.push(Argument {
            name: name.map(str::to_owned),
            default: placeholder.attribute("default").map(str::to_owned),
            options: placeholder
                .attribute("options")
                .map(|o| {
                    o.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        });
        if let Some(name) = name {
            self.by_name.insert(name.to_owned(), index);
        }
        Some(index)
    }
}

/// The inputs `template` asks for, in order (at most [`MAX_ARGUMENTS`]).
pub fn arguments(template: &str) -> Vec<Argument> {
    let mut slots = ArgumentSlots::new();
    for segment in segments(template) {
        if let Segment::Placeholder(placeholder, _) = segment
            && placeholder.keyword == "argument"
        {
            slots.slot(&placeholder);
        }
    }
    slots.specs
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn modify(value: String, modifiers: &[String]) -> String {
    modifiers
        .iter()
        .fold(value, |value, modifier| match modifier.as_str() {
            "uppercase" => value.to_uppercase(),
            "lowercase" => value.to_lowercase(),
            "trim" => value.trim().to_owned(),
            "percent-encode" => utf8_percent_encode(&value, COMPONENT).to_string(),
            "json-stringify" => json_string(&value),
            _ => value,
        })
}

/// Raycast's default renderings: `1 Jun 2022`, `3:05 pm`, `1 Jun 2022 at 3:05 pm`, `Monday`.
fn date_text(keyword: &str, z: &Zoned, format: Option<&str>) -> String {
    if let Some(format) = format {
        return datefmt::format(z, format);
    }
    let time = || datefmt::format(z, "h:mm a").to_lowercase();
    match keyword {
        "time" => time(),
        "datetime" => format!("{} at {}", datefmt::format(z, "d MMM yyyy"), time()),
        "day" => datefmt::format(z, "EEEE"),
        _ => datefmt::format(z, "d MMM yyyy"),
    }
}

/// Expands `template`. `nested` is set inside an inserted `{snippet}`, where `{snippet}`
/// stays verbatim (Raycast inserts only snippets that reference no other) and `{argument}`
/// takes its default, since the inputs belong to the outer snippet; one without a default
/// stays verbatim.
fn expand_with(template: &str, context: &Context<'_>, nested: bool) -> Expansion {
    let mut text = String::with_capacity(template.len());
    let mut cursor = None;
    let mut slots = ArgumentSlots::new();
    for segment in segments(template) {
        let (placeholder, raw) = match segment {
            Segment::Text(literal) => {
                text.push_str(literal);
                continue;
            }
            Segment::Placeholder(placeholder, raw) => (placeholder, raw),
        };
        let value = match placeholder.keyword.as_str() {
            "clipboard" => {
                let offset = placeholder
                    .attribute("offset")
                    .and_then(|o| o.parse::<usize>().ok())
                    .unwrap_or(0);
                Some(context.clipboard.get(offset).cloned().unwrap_or_default())
            }
            "snippet" if nested => None,
            "snippet" => placeholder
                .attribute("name")
                .and_then(|name| (context.snippet)(name))
                .map(|inner| expand_with(&inner, context, true).text),
            "cursor" => {
                cursor.get_or_insert_with(|| text.chars().count());
                Some(String::new())
            }
            "date" | "time" | "datetime" | "day" => {
                let moved = match placeholder.attribute("offset") {
                    Some(terms) => datefmt::offset(context.now, terms),
                    None => Some(context.now.clone()),
                };
                moved.map(|z| date_text(&placeholder.keyword, &z, placeholder.attribute("format")))
            }
            "uuid" => Some((context.uuid)()),
            "selection" => context.selection.map(str::to_owned),
            "argument" if nested => placeholder.attribute("default").map(str::to_owned),
            "argument" => slots.slot(&placeholder).map(|index| {
                context
                    .arguments
                    .get(index)
                    .filter(|v| !v.is_empty())
                    .cloned()
                    .or_else(|| slots.specs[index].default.clone())
                    .unwrap_or_default()
            }),
            _ => None,
        };
        match value {
            Some(value) => text.push_str(&modify(value, &placeholder.modifiers)),
            None => text.push_str(raw),
        }
    }
    Expansion { text, cursor }
}

/// Expands every valid placeholder in `template`.
pub fn expand(template: &str, context: &Context<'_>) -> Expansion {
    expand_with(template, context, false)
}

/// A version 4 UUID in Raycast's uppercase form from 16 random bytes.
pub fn uuid_v4(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = bytes.iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02X}");
        hex
    });
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn now() -> Zoned {
        "2022-06-15T13:44:39.945+00:00[UTC]".parse().unwrap()
    }

    fn snippets(name: &str) -> Option<String> {
        match name {
            "sig" => Some("Best, {argument name=\"who\" default=\"Ada\"}".into()),
            "loop" => Some("again {snippet name=\"loop\"}".into()),
            "date" => Some("on {date format=\"yyyy\"}".into()),
            _ => None,
        }
    }

    fn run(template: &str, arguments: &[&str]) -> Expansion {
        let now = now();
        let clipboard = vec!["  Foo Bar ".to_owned(), "older".to_owned()];
        let arguments: Vec<String> = arguments.iter().map(|&a| a.to_owned()).collect();
        let counter = Cell::new(0u8);
        let uuid = || {
            counter.set(counter.get() + 1);
            uuid_v4([counter.get(); 16])
        };
        let context = Context {
            now: &now,
            clipboard: &clipboard,
            selection: Some("picked"),
            arguments: &arguments,
            snippet: &snippets,
            uuid: &uuid,
        };
        expand(template, &context)
    }

    fn text(template: &str) -> String {
        run(template, &[]).text
    }

    #[test]
    fn raycast_manual_placeholders() {
        assert_eq!(text("{clipboard}"), "  Foo Bar ");
        assert_eq!(text("{clipboard offset=1}"), "older");
        assert_eq!(text("[{clipboard offset=5}]"), "[]");
        assert_eq!(text("{date}"), "15 Jun 2022");
        assert_eq!(text("{time}"), "1:44 pm");
        assert_eq!(text("{datetime}"), "15 Jun 2022 at 1:44 pm");
        assert_eq!(text("{day}"), "Wednesday");
        assert_eq!(text("{date format=\"yyyy-MM-dd\"}"), "2022-06-15");
        assert_eq!(text("{selection}"), "picked");
        assert_eq!(
            text("{uuid} {uuid}"),
            "01010101-0101-4101-8101-010101010101 02020202-0202-4202-8202-020202020202"
        );
    }

    #[test]
    fn raycast_manual_offsets() {
        assert_eq!(text("{date offset=\"+2y +5M\"}"), "15 Nov 2024");
        assert_eq!(text("{time offset=\"+3h +30m\"}"), "5:14 pm");
        assert_eq!(text("{day offset=-3d}"), "Sunday");
        assert_eq!(text("{datetime offset=+1h}"), "15 Jun 2022 at 2:44 pm");
        assert_eq!(
            text("{date format=\"yyyy-MM-dd\" offset=\"+3M -5d\"}"),
            "2022-09-10"
        );
        assert_eq!(text("{date offset=\"+ 2d\"}"), "{date offset=\"+ 2d\"}");
    }

    #[test]
    fn raycast_manual_modifiers() {
        assert_eq!(text("{clipboard | uppercase}"), "  FOO BAR ");
        assert_eq!(text("{clipboard | lowercase}"), "  foo bar ");
        assert_eq!(text("{clipboard | trim | uppercase}"), "FOO BAR");
        assert_eq!(text("{clipboard | trim | percent-encode}"), "Foo%20Bar");
        assert_eq!(text("{clipboard|trim|raw}"), "Foo Bar");
        assert_eq!(text("{selection | json-stringify}"), "\"picked\"");
        assert_eq!(json_string("Foo \"Bar\""), "\"Foo \\\"Bar\\\"\"");
        assert_eq!(text("{clipboard | shout}"), "{clipboard | shout}");
    }

    #[test]
    fn raycast_manual_arguments() {
        let tone = "{argument name=\"tone\"} and {argument name=\"tone\"}, {argument}";
        assert_eq!(run(tone, &["happy", "x"]).text, "happy and happy, x");
        assert_eq!(
            arguments(tone),
            vec![
                Argument {
                    name: Some("tone".into()),
                    ..Argument::default()
                },
                Argument::default()
            ]
        );
        assert_eq!(run("{argument default=\"happy\"}", &[]).text, "happy");
        assert_eq!(run("{argument default=\"happy\"}", &[""]).text, "happy");
        let options = arguments("{argument name=\"tone\" options=\"happy, sad, professional\"}");
        assert_eq!(options[0].options, vec!["happy", "sad", "professional"]);
        assert!(options[0].is_required());
        let four = "{argument}{argument}{argument}{argument}";
        assert_eq!(arguments(four).len(), MAX_ARGUMENTS);
        assert_eq!(run(four, &["a", "b", "c", "d"]).text, "abc{argument}");
    }

    #[test]
    fn snippets_nest_one_level() {
        assert_eq!(text("{snippet name=\"sig\"}"), "Best, Ada");
        assert_eq!(
            text("{snippet name=\"loop\"}"),
            "again {snippet name=\"loop\"}"
        );
        assert_eq!(text("{snippet name=\"date\"}"), "on 2022");
        assert_eq!(text("{snippet name=\"nope\"}"), "{snippet name=\"nope\"}");
        assert_eq!(text("{snippet}"), "{snippet}");
    }

    #[test]
    fn cursor_marks_the_first_position() {
        let expansion = run("Dear ö{cursor}, bye{cursor}", &[]);
        assert_eq!(expansion.text, "Dear ö, bye");
        assert_eq!(expansion.cursor, Some(6));
        assert_eq!(run("none", &[]).cursor, None);
    }

    #[test]
    fn invalid_placeholders_stay_verbatim() {
        for raw in [
            "{browser-tab}",
            "{ }",
            "{}",
            "{date",
            "{date format=\"x}",
            "{date colour=red}",
            "{date format=\"a\" format=\"b\"}",
            "{clipboard offset=-1}",
            "{clipboard | }",
            "{clipboard | trim offset=1}",
            "{{clipboard",
            "{Date}",
        ] {
            assert_eq!(text(raw), raw, "{raw}");
        }
        assert_eq!(text("{{clipboard}}"), "{  Foo Bar }");
        assert_eq!(text("json {\"a\": 1}"), "json {\"a\": 1}");
    }

    #[test]
    fn uuid_sets_version_and_variant() {
        let uuid = uuid_v4([0xff; 16]);
        assert_eq!(uuid, "FFFFFFFF-FFFF-4FFF-BFFF-FFFFFFFFFFFF");
    }
}
