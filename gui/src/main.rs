// g5kbd-gui — native GPUI panel for the Gigabyte G5 keyboard backlight and
// fans. This replaces the previous Tauri (webview) panel: same shape, no
// browser, no Node toolchain.
//
// Every hardware change is delegated to the `g5kbd`/`g5fan` CLIs exactly as
// before — they remain the single source of truth for the protocol, state
// persistence, the fan daemon and the polkit scope:
//
//   GPUI app  --std::process::Command-->  g5kbd / g5fan CLI
//                (LED node / EC)          (root via pkexec only for fan writes)
//
// `use gpui_kit::*` IS gpui: the kit facade re-exports the whole framework
// and pins the matching gpui-pre-* snapshot crates, so this file needs no
// direct `gpui` dependency.

mod backend;
mod panel;
mod theme;
mod views;
mod widgets;

use gpui_kit::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};

use panel::{Panel, View};

fn main() {
    // `gpui_kit::application()` opens the platform backend (Wayland/X11 on
    // Linux) and hands us an `App`; `init` installs the component theme and
    // the default icon assets. The `Root` wrapper that open_window() applies
    // is what makes dialogs/tooltips work inside the window.
    gpui_kit::application().run(|cx: &mut App| {
        gpui_kit::init(cx);

        let bounds = Bounds::centered(None, size(px(1040.), px(680.)), cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("g5kbd — Gigabyte G5".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            |window, cx| {
                use gpui_kit::AppContext as _;
                cx.new(|cx| Panel::new(window, cx))
            },
        )
        .expect("failed to open the g5kbd window");

        cx.activate(true);
    });
}

// Silence the "unused" nav placeholder so the enum is exercised in debug runs.
#[allow(dead_code)]
fn _nav_used(v: View) -> &'static str {
    v.title()
}
