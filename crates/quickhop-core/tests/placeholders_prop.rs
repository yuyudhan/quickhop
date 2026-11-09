//! Property tests for placeholder expansion: it never panics, and text that holds no valid
//! placeholder comes back unchanged.

use std::fmt::Write as _;

use jiff::Zoned;
use proptest::prelude::*;
use quickhop_core::placeholders::{Context, arguments, expand};

const KEYWORDS: [&str; 10] = [
    "clipboard",
    "snippet",
    "cursor",
    "date",
    "time",
    "datetime",
    "day",
    "uuid",
    "selection",
    "argument",
];

fn expand_text(template: &str, values: &[String]) -> String {
    let now: Zoned = "2024-02-29T23:59:59.999+05:30[Asia/Kolkata]"
        .parse()
        .unwrap_or_default();
    let clipboard = vec!["clip".to_owned()];
    let snippet = |name: &str| Some(format!("<{name} {{clipboard}} {{snippet name=\"x\"}}>"));
    let uuid = || "UUID".to_owned();
    let context = Context {
        now: &now,
        clipboard: &clipboard,
        selection: None,
        arguments: values,
        snippet: &snippet,
        uuid: &uuid,
    };
    expand(template, &context).text
}

/// Placeholder-shaped text the engine must leave alone: an unknown keyword with arbitrary
/// attributes and modifiers.
fn unknown_placeholder() -> impl Strategy<Value = String> {
    (
        "[a-z][a-z-]{0,10}",
        proptest::collection::vec(("[a-z]{1,6}", "[a-zA-Z0-9 +-]{0,8}"), 0..3),
        proptest::collection::vec("[a-z-]{1,8}", 0..2),
    )
        .prop_filter("known keyword", |(keyword, _, _)| {
            !KEYWORDS.contains(&keyword.as_str())
        })
        .prop_map(|(keyword, attributes, modifiers)| {
            let mut text = format!("{{{keyword}");
            for (key, value) in attributes {
                let _ = write!(text, " {key}=\"{value}\"");
            }
            for modifier in modifiers {
                let _ = write!(text, " | {modifier}");
            }
            text.push('}');
            text
        })
}

proptest! {
    #[test]
    fn never_panics(template in ".{0,200}", values in proptest::collection::vec(".{0,8}", 0..4)) {
        let _ = expand_text(&template, &values);
        let _ = arguments(&template);
    }

    #[test]
    fn placeholder_like_noise_never_panics(
        parts in proptest::collection::vec(
            prop_oneof![
                Just("{".to_owned()), Just("}".to_owned()), Just("|".to_owned()),
                Just("\"".to_owned()), Just("=".to_owned()), Just(" ".to_owned()),
                Just("date".to_owned()), Just("offset".to_owned()), Just("+3d".to_owned()),
                Just("format".to_owned()), Just("'".to_owned()), Just("argument".to_owned()),
                Just("snippet".to_owned()), Just("name".to_owned()), "[a-zA-Z]{1,3}",
            ],
            0..40,
        )
    ) {
        let template = parts.concat();
        let _ = expand_text(&template, &[]);
        let _ = arguments(&template);
    }

    #[test]
    fn unknown_placeholders_stay_verbatim(
        parts in proptest::collection::vec(
            prop_oneof!["[^{]{0,12}", unknown_placeholder()],
            0..8,
        )
    ) {
        let template = parts.concat();
        prop_assert_eq!(expand_text(&template, &[]), template.clone());
        prop_assert!(arguments(&template).is_empty());
    }
}
