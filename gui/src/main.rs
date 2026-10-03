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

use gpui_kit::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};

use g5kbd_gui::panel::{Panel, View};

fn main() {
    // `gpui_kit::application()` opens the platform backend (Wayland/X11 on
    // Linux) and hands us an `App`; `init` installs the component theme and
    // the default icon assets. The `Root` wrapper that open_window() applies
    // is what makes dialogs/tooltips work inside the window.
    gpui_kit::application().run(|cx: &mut App| {
        gpui_kit::init(cx);

        // `gui/src/theme.rs` is a fixed dark palette, so the component widgets
        // have to follow it instead of the desktop's light mode. Left alone,
        // the kit follows the system appearance and a light-themed desktop
        // renders white buttons inside our dark cards.
        g5kbd_gui::theme::apply_kit_theme(cx);

        // 720×600 is roughly half a large screen, which is the narrowest window
        // `layout::Breakpoint` treats as the compact one-column rail layout.
        // Below it the keyboard preview stops being legible, so the compositor
        // is asked not to go there rather than letting the content clip.
        let bounds = Bounds::centered(None, size(px(1040.), px(680.)), cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // The panel reads the window width every frame and rearranges
                // itself, so it has to be resizable in the first place.
                is_resizable: true,
                window_min_size: Some(size(px(720.), px(600.))),
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
