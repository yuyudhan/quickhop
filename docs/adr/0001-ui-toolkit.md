# ADR 0001: gpui for the launcher window

- Status: accepted; the UI spike (plan slice U0) passed its speed gate on the MacBook M4, the
  blur check and the itlinux3 half are still open (see "Spike results")
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
| Rendering and startup | GPU (Metal on macOS, Vulkan via wgpu on Linux), retained element tree, built for a 120 Hz editor | GPU (wgpu), retained Elm-style | GPU, immediate mode: redraws the whole UI per frame, fine for tools, wasteful for an always-resident app | a browser engine per window: WKWebView on macOS, WebKitGTK on Linux (slowest start, highest memory) | GPU or software, retained, small |
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
- gpui is a git dependency: `deny.toml` allows the Zed repository and the other git sources
  gpui's tree uses at the pinned revision, skips the duplicate versions that tree brings
  (`syn` 2 today), and ignores its unmaintained-crate advisories, each listed with a reason.
- Builds get heavier (gpui and wgpu). CI caches `target/`; the CLI and core can still be
  built and tested without the UI crate (`cargo test -p quickhop-core -p quickhop-providers`).
- gpui on Linux needs Vulkan; machines without a GPU use Mesa's lavapipe, which is also what
  the Linux UI tests use under a headless compositor.
- **Spike gate (slice U0):** on the MacBook M4 and on itlinux3 (sway headless and GNOME), a
  gpui window showing a 5,000-item filtered list must appear in under 50 ms after
  `quickhop toggle`, keep typing under 8 ms per frame, blur on macOS, and open as a
  layer-shell overlay on sway. If gpui misses the gate, fall back to iced with
  `iced_layershell` and record the result here as ADR 0002.

## Spike results

The spike is `crates/quickhop-ui/examples/spike.rs`: a 750×474 gpui window (`WindowKind::PopUp`,
a non-activating panel on macOS, `Blurred` background) with a search field over 5,000 fake
items ranked by `quickhop_core::Search` and a `uniform_list` of rows with match highlighting.
It is driven over a Unix socket, so the numbers come from outside the process:

    cargo build --release -p quickhop-ui --example spike
    target/release/examples/spike serve /tmp/spike.sock &
    target/release/examples/spike measure /tmp/spike.sock 50
    swiftc -O scripts/spike-keys.swift -o /tmp/spike-keys   # real key events (macOS)
    /tmp/spike-keys "$(target/release/examples/spike send /tmp/spike.sock pid | cut -d' ' -f2)" 'firefox---' 40
    target/release/examples/spike send /tmp/spike.sock stats

`measure` shows and hides the window 50 times (a show is answered after the first frame drawn
and presented while the window is key), then types five queries one key per frame (35
keystrokes, five rounds). `stats` summarises keystrokes that arrived as real key events,
posted to the spike's process only by `scripts/spike-keys.swift`.

### MacBook M4 (macOS 27.0.1, 120 Hz built-in display), 2026-11-09

Measured while other builds kept the load average at 17 to 21 on 12 cores, so these are upper
bounds.

| Gate | Target | Result | Verdict |
|------|--------|--------|---------|
| Show to first presented frame, from the IPC command (median of 50) | under 50 ms | p50 34.3 ms, p95 46.1 ms | pass |
| Typing at 5,000 items, rank + draw per keystroke (socket-driven) | under 8 ms | p50 4.3 ms, p95 7.2 ms | pass |
| Typing at 5,000 items, key event to presented frame (350 real keys) | under 8 ms | p50 7.7 ms, p95 14.9 ms | pass at the median |
| Blur visible (screenshot differs from `Opaque`) | differs | not measured: the agent's session has no Screen Recording permission, so `screencapture` fails | open |

Where the time goes:

- gpui's own frame (layout, paint, present) is small: 0.6 ms p50 for the show frame, 1.1 ms
  p50 / 3.6 ms p95 per socket keystroke and 2.2 ms p50 / 4.8 ms p95 per real key.
- Ranking dominates typing: `Search::run` over 5,000 items takes 3.0 to 5.0 ms p50 and up to
  10 ms p95, mostly because the provider clones every item per keystroke. The window code must
  rank on a worker thread with `Cancel` (as `architecture.md` plans) and providers should stop
  cloning their whole index per query; neither is a toolkit question.
- A show spends about 33 ms in macOS activation before the panel is key; the draw is under
  1 ms. Lifting gpui's 30 fps inactive-window frame limit changed nothing (p50 33.0 ms vs
  32.7 ms). Closing the window on hide and opening a new one on show (`--strategy reopen`)
  doubles the show time to p50 63.5 ms, so the daemon keeps one window and hides the app.
- The first show after start costs 140 to 520 ms: window creation plus Metal shader compilation
  (`runtime_shaders`, needed because only the Command Line Tools are installed). The daemon
  should create its window hidden at start-up.

Still open for the gate: the blur screenshot on m4 (grant Screen Recording to the terminal
once, then `spike send … background opaque` / `blurred` and compare `screencapture -l <window
id>` images), and the itlinux3 half (show and typing timings under sway and GNOME, the sway
layer-shell check).
