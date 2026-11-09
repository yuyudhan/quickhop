# ADR 0001: gpui for the launcher window

- Status: accepted, confirmed by the UI spike (plan slice U0) before any other UI work
- Date: 2026-11-09

## Context

quickhop's window is shown hundreds of times a day. It has to appear within a frame or two of
the hotkey, render a filtered list of a few thousand items at 120 Hz while typing, look at home
next to Raycast on macOS (rounded, translucent, blurred), and behave as an overlay on Linux
under both X11 and Wayland. The core, the providers and the CLI exist and are toolkit-free; the
UI crate is empty.

What each option gives for the needs that decide it:

| Need | gpui (Zed) | iced | egui / eframe | Tauri (webview) | Slint |
|------|------------|------|---------------|-----------------|-------|
| Rendering and startup | GPU (Metal on macOS, Vulkan via blade on Linux), retained element tree, built for a 120 Hz editor | GPU (wgpu), retained Elm-style | GPU, immediate mode: redraws the whole UI per frame, fine for tools, wasteful for an always-resident app | a browser engine per window: WKWebView on macOS, WebKitGTK on Linux (slowest start, highest memory) | GPU or software, retained, small |
| Text | its own shaping and font fallback (Zed's) | cosmic-text | its own, weaker shaping and fallback | browser | its own |
| macOS translucency and blur | `WindowBackgroundAppearance::Blurred` (NSVisualEffectView) | transparent window, no system blur | transparent window, no blur | CSS backdrop-filter inside the page, window vibrancy via a plugin | transparent, no blur |
| Wayland overlay | native Wayland and X11 backends; `WindowKind::LayerShell` in current Zed sources; KDE blur protocol | layer-shell only through third-party forks (`iced_layershell`, COSMIC's fork) | via winit: no layer-shell | GTK: `gtk-layer-shell` workarounds | via winit: no layer-shell |
| Keyboard-first list UI | Zed's own pickers are exactly this (uniform lists, key contexts and bindings) | build from widgets | build from widgets | HTML/JS | declarative `.slint` |
| Language | Rust only | Rust only | Rust only | Rust + TypeScript | Rust + `.slint` DSL |
| Licence | Apache-2.0 | MIT | MIT/Apache-2.0 | MIT/Apache-2.0 | GPL-3.0 / royalty-free / commercial |
| Maturity risk | pre-1.0; crates.io `gpui` 0.2.x trails the Zed tree (layer-shell is newer than 0.2.2), API changes with Zed | stable releases, smaller API breaks | stable | stable | stable |

The global hotkey does not depend on the toolkit: `global-hotkey` (Carbon `RegisterEventHotKey`
on macOS, `XGrabKey` on X11) plus the XDG `GlobalShortcuts` portal (`ashpd`) or a compositor
binding to `quickhop toggle` on Wayland. Every toolkit above needs the same.

## Decision

Build the launcher with **gpui**, pinned to one reviewed Zed git revision (moved deliberately,
like the Rust toolchain), with the `wayland` and `x11` features.

Why:

1. **Speed.** A GPU-native retained UI with Zed's uniform-list virtualization keeps typing at
   frame rate over large result sets, and a resident process keeps the window one show call away.
2. **Look.** Real system blur on macOS and transparency everywhere, without a webview.
3. **Linux overlays.** The same crate drives X11 and Wayland, including layer-shell for sway,
   Hyprland, KDE and COSMIC; GNOME (no layer-shell) gets a normal undecorated centered window.
4. **One language.** Everything stays Rust: one toolchain, one test runner, one set of lints.

## Consequences

- The window code lives in `quickhop-ui` and depends on gpui; the view model (query text,
  results, selection, navigation stack, action panel state) is plain Rust in
  `quickhop-ui::model` with no gpui types, unit-tested headless. Replacing the toolkit means
  rewriting the views, not the behaviour.
- gpui is a git dependency: `deny.toml` allows the Zed repository as a source, and the
  "one version per crate" rule gets the exceptions gpui's tree needs, listed explicitly.
- Builds get heavier (gpui and blade). CI caches `target/`; the CLI and core can still be
  built and tested without the UI crate (`cargo test -p quickhop-core -p quickhop-providers`).
- gpui on Linux needs Vulkan; machines without a GPU use Mesa's lavapipe, which is also what
  the Linux UI tests use under a headless compositor.
- **Spike gate (slice U0):** on the MacBook M4 and on itlinux3 (sway headless and GNOME), a
  gpui window showing a 5,000-item filtered list must appear in under 50 ms after
  `quickhop toggle`, keep typing under 8 ms per frame, blur on macOS, and open as a
  layer-shell overlay on sway. If gpui misses the gate, fall back to iced with
  `iced_layershell` and record the result here as ADR 0002.
