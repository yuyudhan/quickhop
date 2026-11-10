//! The onboarding window is tall enough for every text it shows, so its button is always whole
//! (PLT-02): each state's title and text are wrapped by macOS's own text shaping, in a headless
//! gpui app that opens no window, and the window's height must hold them, the status line, the
//! button and the padding. The texts that name the macOS 27 pane, or a long binary path, did
//! not fit the fixed 250 px the window used to have.
//!
//! Its own binary without libtest (`harness = false`): gpui's macOS platform starts only on
//! the main thread.

#[cfg(target_os = "macos")]
fn main() {
    use std::io::Write as _;

    gpui_platform::headless().run(|cx| {
        let failures = checks::run(cx);
        let mut stderr = std::io::stderr().lock();
        for failure in &failures {
            // A report that cannot be written still fails by the exit status.
            let _ = writeln!(stderr, "FAIL {failure}");
        }
        std::process::exit(i32::from(!failures.is_empty()));
    });
}

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
mod checks {
    use std::io::Write as _;
    use std::sync::Arc;

    use gpui::{App, Font, FontWeight, SharedString, TextRun, WindowTextSystem, font, px};
    use quickhop_ui::onboarding::{Onboarding, Program};
    use quickhop_ui::onboarding_window::{WIDTH, height};

    /// The window's layout, as gpui draws it (rem = 16 px): `p_6`, `gap_3`, `text_xl`,
    /// `text_sm`, `py_1`, and gpui's default line height.
    const PADDING: f32 = 24.0;
    const GAP: f32 = 12.0;
    const TITLE_SIZE: f32 = 20.0;
    const TEXT_SIZE: f32 = 14.0;
    const LINE_HEIGHT: f32 = std::f32::consts::GOLDEN_RATIO;
    const BUTTON_PADDING_Y: f32 = 4.0;
    /// The children of the content column: title, status, text, spacer, button.
    const GAPS: f32 = 4.0;
    /// The height the window had before it followed its text.
    const OLD_HEIGHT: f32 = 250.0;

    /// Every state the window shows, for `program`.
    fn states(program: &Program) -> Vec<(&'static str, Onboarding)> {
        let new = |granted: bool, hotkey: Option<&str>| {
            Onboarding::new(granted, hotkey.map(str::to_owned), program.clone())
        };
        let mut asked = new(false, Some("Alt+Space"));
        asked.asked(Ok(()));
        let mut failed = new(false, Some("Alt+Space"));
        failed.asked(Err(
            "open x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?\
             Privacy_Accessibility failed (exit status: 1); open \
             x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility \
             failed (exit status: 1)"
                .to_owned(),
        ));
        vec![
            ("not allowed", new(false, Some("Alt+Space"))),
            ("asked", asked),
            ("settings did not open", failed),
            ("allowed", new(true, Some("Cmd+Ctrl+Alt+F13"))),
            ("allowed, no hotkey", new(true, None)),
        ]
    }

    /// Lines `text` wraps to at the content width.
    fn lines(text_system: &WindowTextSystem, text: &str, font: Font, size: f32) -> f32 {
        let run = TextRun {
            len: text.len(),
            font,
            ..TextRun::default()
        };
        let wrapped = text_system
            .shape_text(
                SharedString::from(text.to_owned()),
                px(size),
                &[run],
                Some(px(WIDTH - 2.0 * PADDING)),
                None,
            )
            .unwrap_or_default();
        let count: usize = wrapped
            .iter()
            .map(|line| line.wrap_boundaries().len() + 1)
            .sum();
        f32::from(u16::try_from(count).unwrap_or(u16::MAX))
    }

    /// What the content of `model` needs, top to bottom.
    fn needed(text_system: &WindowTextSystem, model: &Onboarding) -> f32 {
        let bold = Font {
            weight: FontWeight::BOLD,
            ..font(".SystemUIFont")
        };
        let title = lines(text_system, model.title(), bold, TITLE_SIZE) * TITLE_SIZE;
        let text = lines(
            text_system,
            &model.message(),
            font(".SystemUIFont"),
            TEXT_SIZE,
        );
        let button = TEXT_SIZE * LINE_HEIGHT + 2.0 * BUTTON_PADDING_Y;
        2.0 * PADDING
            + title * LINE_HEIGHT
            + TEXT_SIZE * LINE_HEIGHT
            + text * TEXT_SIZE * LINE_HEIGHT
            + button
            + GAPS * GAP
    }

    /// Every failed check.
    pub(crate) fn run(cx: &App) -> Vec<String> {
        let text_system = WindowTextSystem::new(Arc::clone(cx.text_system()));
        let programs = [
            Program::App,
            Program::Binary("/Users/someone/.cargo/bin/quickhop".into()),
            Program::Binary(
                "/Users/someone/Library/Developer/Toolchains/a rather long folder name/quickhop \
                 builds/target/release/quickhop"
                    .into(),
            ),
        ];
        let mut failures = Vec::new();
        let mut over_old = 0;
        for program in &programs {
            for (state, model) in states(program) {
                let needed = needed(&text_system, &model);
                let window = f32::from(height(&text_system, &model));
                // The table is for reading; failures are what the exit status reports.
                let _ = writeln!(
                    std::io::stdout(),
                    "{program:?} / {state}: needs {needed:.1} px, window {window:.1} px"
                );
                if window < needed {
                    failures.push(format!(
                        "{program:?} / {state}: the window is {window:.1} px, its content \
                         needs {needed:.1} px, so the button is cut off"
                    ));
                }
                if needed > OLD_HEIGHT {
                    over_old += 1;
                }
            }
        }
        // The case this guards: the old fixed height cut the button off for the longer texts.
        if over_old == 0 {
            failures.push(format!(
                "no state needs more than the old {OLD_HEIGHT} px: the measurement is broken"
            ));
        }
        failures
    }
}
