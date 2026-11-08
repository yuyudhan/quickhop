//! Script commands from the fixture directory: discovery on every OS, execution of all
//! four modes on Unix (the fixtures are `/bin/sh` scripts).

use crate::common;
use quickhop_core::{Action, Item, ScriptMode};
use quickhop_providers::scripts::ScriptsProvider;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn find<'a>(items: &'a [Item], title: &str) -> Result<&'a Item, String> {
    items
        .iter()
        .find(|i| i.title == title)
        .ok_or_else(|| format!("{title} not found"))
}

#[test]
fn discovers_valid_scripts_only() {
    let (provider, warnings) = ScriptsProvider::load(&[common::fixtures().join("scripts")], None);
    let mut titles: Vec<&str> = provider.all().iter().map(|i| i.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(
        titles,
        vec![
            "Compact Count",
            "Empty Trash",
            "Failing Script",
            "Hello World",
            "Inline Status",
            "Search Docs",
            "Silent Toggle",
            "Slow Script",
            "Where Am I",
        ]
    );
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("invalid.sh") && warnings[0].contains("@raycast.mode"));
}

#[test]
fn items_describe_the_script() -> TestResult {
    let provider = common::scripts();
    let hello = find(provider.all(), "Hello World")?;
    assert_eq!(hello.id.key, "hello.sh");
    assert_eq!(hello.subtitle.as_deref(), Some("Greetings"));
    assert_eq!(hello.icon.as_deref(), Some("👋"));
    let Some(Action::Script {
        mode,
        arguments,
        confirm,
        ..
    }) = hello.default_action()
    else {
        return Err("Hello World runs a script".into());
    };
    assert_eq!(*mode, ScriptMode::FullOutput);
    assert!(arguments.is_empty() && !confirm);
    let search = find(provider.all(), "Search Docs")?;
    let Some(Action::Script { arguments, .. }) = search.default_action() else {
        return Err("Search Docs runs a script".into());
    };
    assert_eq!(arguments.len(), 2);
    assert!(find(provider.all(), "Empty Trash")?.actions[0].needs_confirmation());
    Ok(())
}

#[test]
fn unreadable_directories_are_warnings() {
    let (provider, warnings) =
        ScriptsProvider::load(&[common::fixtures().join("no-such-dir")], None);
    assert_eq!(provider.all().len(), 0);
    assert_eq!(warnings.len(), 1);
}

#[cfg(unix)]
mod execution {
    use std::time::Duration;

    use quickhop_core::Action;
    use quickhop_providers::scripts::{RunError, ScriptsProvider, run};

    use super::{TestResult, find};
    use crate::common;

    const TIMEOUT: Duration = Duration::from_secs(10);

    fn action(title: &str) -> Result<Action, Box<dyn std::error::Error>> {
        let provider = common::scripts();
        let item = find(provider.all(), title)?;
        item.default_action()
            .cloned()
            .ok_or_else(|| "no action".into())
    }

    #[test]
    fn full_output_compact_and_silent() -> TestResult {
        assert_eq!(
            run(&action("Hello World")?, &[], TIMEOUT)?.message,
            "Hello\nWorld"
        );
        assert_eq!(
            run(&action("Compact Count")?, &[], TIMEOUT)?.message,
            "three"
        );
        assert_eq!(
            run(&action("Silent Toggle")?, &[], TIMEOUT)?.message,
            "toggled"
        );
        Ok(())
    }

    #[test]
    fn inline_scripts_show_their_first_line() -> TestResult {
        let (provider, warnings) = ScriptsProvider::load(
            &[common::fixtures().join("scripts")],
            Some(Duration::from_secs(10)),
        );
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(
            find(provider.all(), "Inline Status")?.subtitle.as_deref(),
            Some("all good")
        );
        Ok(())
    }

    #[test]
    fn arguments_are_checked_and_encoded() -> TestResult {
        let search = action("Search Docs")?;
        let out = run(&search, &["a b".into(), "rust".into()], TIMEOUT)?;
        assert_eq!(out.message, "searching a%20b on rust");
        assert_eq!(
            run(&search, &["x".into()], TIMEOUT)?.message,
            "searching x on all"
        );
        assert!(matches!(
            run(&search, &[], TIMEOUT),
            Err(RunError::Arguments(_))
        ));
        assert!(matches!(
            run(&search, &["x".into(), "go".into()], TIMEOUT),
            Err(RunError::Arguments(_))
        ));
        Ok(())
    }

    #[test]
    fn failures_timeouts_and_working_directory() -> TestResult {
        let failed = run(&action("Failing Script")?, &[], TIMEOUT)?;
        assert!(!failed.success);
        assert_eq!(failed.message, "bad thing");
        assert!(matches!(
            run(&action("Slow Script")?, &[], Duration::from_millis(200)),
            Err(RunError::Timeout(_))
        ));
        let cwd = run(&action("Where Am I")?, &[], TIMEOUT)?.message;
        let expected = common::fixtures().canonicalize()?;
        assert_eq!(std::path::Path::new(&cwd).canonicalize()?, expected);
        Ok(())
    }

    #[test]
    fn non_script_actions_are_rejected() {
        let open = Action::Open { target: "x".into() };
        assert!(matches!(
            run(&open, &[], TIMEOUT),
            Err(RunError::NotAScript)
        ));
    }
}
