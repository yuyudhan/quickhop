//! The colours of quickhop's windows for the `theme` setting: a dark and a light palette, and
//! `system` picking one by the system appearance. Colours are `0xRRGGBB` for [`rgb`] and
//! `0xRRGGBBAA` for [`rgba`]-style translucent ones.
//!
//! [`rgb`]: https://docs.rs/gpui/latest/gpui/fn.rgb.html
//! [`rgba`]: https://docs.rs/gpui/latest/gpui/fn.rgba.html

use quickhop_core::Theme;

/// Every colour a window draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Whether this is the dark palette.
    pub dark: bool,
    /// The launcher's and the HUD's translucent background (`0xRRGGBBAA`).
    pub background: u32,
    /// The background of opaque windows: settings, onboarding.
    pub window: u32,
    /// Ordinary text.
    pub text: u32,
    /// Titles and matched characters.
    pub strong: u32,
    /// Secondary text: subtitles, placeholders, hints.
    pub muted: u32,
    /// The selected row (`0xRRGGBBAA`).
    pub selection: u32,
    /// Lines between parts of a window (`0xRRGGBBAA`).
    pub border: u32,
    /// The action panel's background (`0xRRGGBBAA`).
    pub panel: u32,
    /// Input fields and switches that are off (`0xRRGGBBAA`).
    pub field: u32,
    /// The focused field's border, and switches that are on.
    pub accent: u32,
    /// Text on [`Palette::accent`].
    pub on_accent: u32,
    /// Success: a finished action, a granted permission.
    pub success: u32,
    /// Failure.
    pub failure: u32,
    /// Something still running.
    pub progress: u32,
    /// Waiting for the user.
    pub warning: u32,
    /// The 16 ANSI colours drawn on this background: normal, then bright.
    pub ansi: [u32; 16],
}

/// The dark palette (Nord), quickhop's look so far.
pub const DARK: Palette = Palette {
    dark: true,
    background: 0x2e34_40b0,
    window: 0x002e_3440,
    text: 0x00d8_dee9,
    strong: 0x00ff_ffff,
    muted: 0x008f_96a3,
    selection: 0xffff_ff1f,
    border: 0xffff_ff1a,
    panel: 0x1e22_2af0,
    field: 0xffff_ff14,
    accent: 0x0088_c0d0,
    on_accent: 0x002e_3440,
    success: 0x00a3_be8c,
    failure: 0x00bf_616a,
    progress: 0x0088_c0d0,
    warning: 0x00eb_cb8b,
    ansi: [
        0x004c_566a,
        0x00ff_5555,
        0x00a3_be8c,
        0x00eb_cb8b,
        0x0081_a1c1,
        0x00b4_8ead,
        0x0088_c0d0,
        0x00e5_e9f0,
        0x0061_6e88,
        0x00ff_7a7a,
        0x00b9_d4a5,
        0x00f0_d8a8,
        0x009a_b5d1,
        0x00c8_a6c3,
        0x008f_bcbb,
        0x00ec_eff4,
    ],
};

/// The light palette: the same hues, dark enough to read on white.
pub const LIGHT: Palette = Palette {
    dark: false,
    background: 0xf4f6_f9c8,
    window: 0x00f4_f6f9,
    text: 0x002e_3440,
    strong: 0x0000_0000,
    muted: 0x0060_6a7a,
    selection: 0x0000_0018,
    border: 0x0000_0018,
    panel: 0xffff_fff4,
    field: 0x0000_000d,
    accent: 0x0035_6fb0,
    on_accent: 0x00ff_ffff,
    success: 0x0037_7a3a,
    failure: 0x00b0_2a37,
    progress: 0x0035_6fb0,
    warning: 0x009a_6700,
    ansi: [
        0x002e_3440,
        0x00b0_2a37,
        0x0037_7a3a,
        0x009a_6700,
        0x0035_6fb0,
        0x0085_4d8f,
        0x0018_7a8a,
        0x004c_566a,
        0x0061_6e88,
        0x00d0_3a47,
        0x0047_9a4a,
        0x00b8_8000,
        0x0045_80c8,
        0x00a0_60b0,
        0x0020_9aa8,
        0x0000_0000,
    ],
};

/// The palette of `theme` while the system appearance is dark (`system_dark`) or light.
pub fn palette(theme: Theme, system_dark: bool) -> &'static Palette {
    let dark = match theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => system_dark,
    };
    if dark { &DARK } else { &LIGHT }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG's relative luminance of an `0xRRGGBB` colour.
    fn luminance(colour: u32) -> f64 {
        let channel = |shift: u32| {
            let c = f64::from((colour >> shift) & 0xff) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn system_follows_the_appearance_and_the_others_do_not() {
        assert!(palette(Theme::System, true).dark);
        assert!(!palette(Theme::System, false).dark);
        for system_dark in [true, false] {
            assert!(palette(Theme::Dark, system_dark).dark);
            assert!(!palette(Theme::Light, system_dark).dark);
        }
    }

    #[test]
    fn text_is_readable_on_each_window_background() {
        for palette in [&DARK, &LIGHT] {
            let dark = palette.dark;
            for (name, colour) in [("text", palette.text), ("strong", palette.strong)] {
                let ratio = contrast(colour, palette.window);
                assert!(ratio >= 4.5, "{name} on dark={dark}: {ratio:.2}");
            }
            let on_accent = contrast(palette.on_accent, palette.accent);
            assert!(
                on_accent >= 4.5,
                "accent label on dark={dark}: {on_accent:.2}"
            );
            // Hints and status lines: WCAG's minimum for large text and UI parts.
            for (name, colour) in [
                ("muted", palette.muted),
                ("success", palette.success),
                ("failure", palette.failure),
                ("warning", palette.warning),
            ] {
                let ratio = contrast(colour, palette.window);
                assert!(ratio >= 3.0, "{name} on dark={dark}: {ratio:.2}");
            }
        }
    }
}
