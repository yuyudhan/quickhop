//! docs/configuration.md documents every key of `config.toml` and `snippets.toml` with its real
//! default, and nothing else: each table row that starts with two code spans, | `key` |
//! `default` |, must match the structs exactly.
//!
//! Defaults come from serializing the release's defaults (a development build differs only in
//! its hotkey, which the document mentions in prose). Keys of array tables (`[[quicklinks]]`,
//! `[[commands]]`, `[[snippets]]`) are written `quicklinks[].name`; their default is the value a
//! minimal entry gets, `required` for keys the minimal entry needs, or `unset`.

use std::collections::BTreeMap;

use quickhop_core::snippets::SnippetsFile;
use quickhop_core::{CommandSettings, Config, Flavour, Quicklink};
use toml::{Table, Value};

/// Every scalar or array key under `table`, as `prefix.key` = its TOML text.
fn flatten(prefix: &str, table: &Table, out: &mut BTreeMap<String, String>) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Value::Table(inner) => flatten(&path, inner, out),
            other => {
                out.insert(path, other.to_string());
            }
        }
    }
}

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn table_of(value: &impl serde::Serialize) -> TestResult<Table> {
    Ok(Table::try_from(value)?)
}

/// The first entry of an array table parsed from a fixture.
fn first<'a, T>(entries: &'a [T], what: &str) -> TestResult<&'a T> {
    entries
        .first()
        .ok_or_else(|| format!("the {what} fixture parsed to no entry").into())
}

/// The documented rows for an array table: every key of `full`, with the default `minimal`
/// serializes to, `required` if `required` names it, else `unset`.
fn array_rows(
    name: &str,
    full: &Table,
    minimal: &Table,
    required: &[&str],
    out: &mut BTreeMap<String, String>,
) {
    for key in full.keys() {
        let default = if required.contains(&key.as_str()) {
            "required".to_owned()
        } else {
            minimal
                .get(key)
                .map_or_else(|| "unset".to_owned(), ToString::to_string)
        };
        out.insert(format!("{name}[].{key}"), default);
    }
}

/// What the structs say docs/configuration.md must list.
fn expected() -> TestResult<BTreeMap<String, String>> {
    let mut rows = BTreeMap::new();
    let mut defaults = table_of(&Config::defaults(Flavour::Release))?;
    defaults.remove("quicklinks");
    defaults.remove("commands");
    flatten("", &defaults, &mut rows);

    let link_full = Quicklink {
        name: "n".into(),
        url: "u".into(),
        alias: Some("a".into()),
    };
    let config = Config::from_toml("[[quicklinks]]\nname = \"n\"\nurl = \"u\"\n")?;
    array_rows(
        "quicklinks",
        &table_of(&link_full)?,
        &table_of(first(&config.quicklinks, "quicklink")?)?,
        &["name", "url"],
        &mut rows,
    );

    let command_full = CommandSettings {
        id: "apps:x".into(),
        alias: Some("a".into()),
        enabled: false,
        hotkey: Some("Ctrl+Alt+T".into()),
        order: Some(1),
    };
    let config = Config::from_toml("[[commands]]\nid = \"apps:x\"\n")?;
    array_rows(
        "commands",
        &table_of(&command_full)?,
        &table_of(first(&config.commands, "command")?)?,
        &["id"],
        &mut rows,
    );

    let full: SnippetsFile =
        toml::from_str("[[snippets]]\nname = \"n\"\ntext = \"t\"\nkeyword = \"k\"\n")?;
    let minimal: SnippetsFile = toml::from_str("[[snippets]]\nname = \"n\"\ntext = \"t\"\n")?;
    array_rows(
        "snippets",
        &table_of(first(&full.snippets, "full snippet")?)?,
        &table_of(first(&minimal.snippets, "minimal snippet")?)?,
        &["name", "text"],
        &mut rows,
    );
    Ok(rows)
}

/// The `| `key` | `default` |` rows of the document.
fn documented(text: &str) -> TestResult<BTreeMap<String, String>> {
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // A table row splits into "", cell 1, cell 2, ..., "".
        let [_, key, default, ..] = cells.as_slice() else {
            continue;
        };
        let code = |cell: &str| {
            cell.strip_prefix('`')
                .and_then(|c| c.strip_suffix('`'))
                .map(str::to_owned)
        };
        if let (Some(key), Some(default)) = (code(key), code(default))
            && rows.insert(key.clone(), default).is_some()
        {
            return Err(format!("{key} is documented twice").into());
        }
    }
    Ok(rows)
}

#[test]
fn configuration_md_lists_every_key_with_its_default() -> TestResult<()> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/configuration.md");
    let text = std::fs::read_to_string(path)?;
    let documented = documented(&text)?;
    let expected = expected()?;

    let missing: Vec<String> = expected
        .iter()
        .filter(|(key, _)| !documented.contains_key(*key))
        .map(|(key, default)| format!("| `{key}` | `{default}` |"))
        .collect();
    assert!(
        missing.is_empty(),
        "undocumented keys:\n{}",
        missing.join("\n")
    );

    let unknown: Vec<&String> = documented
        .keys()
        .filter(|key| !expected.contains_key(*key))
        .collect();
    assert!(
        unknown.is_empty(),
        "documented keys that do not exist: {unknown:?}"
    );

    let wrong: Vec<String> = expected
        .iter()
        .filter(|(key, default)| documented.get(*key) != Some(default))
        .map(|(key, default)| format!("{key}: documented {:?}, real `{default}`", documented[key]))
        .collect();
    assert!(wrong.is_empty(), "wrong defaults:\n{}", wrong.join("\n"));
    Ok(())
}
