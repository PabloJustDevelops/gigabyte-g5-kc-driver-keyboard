//! Offscreen tests for the panel: the visual snapshots that made the UI
//! checkable, plus the keyboard guarantees that are easy to lose.
//!
//! ```sh
//! cargo test --test snapshot -- --nocapture
//! ```
//!
//! `G5KBD_SNAPSHOT_DIR` overrides the output directory. The images are meant
//! to be looked at, not asserted against: pinning pixels breaks on every font
//! or theme change, which is exactly what the visual rework is changing.
//!
//! The panel reaches the hardware through the `g5kbd`/`g5fan` CLIs, so a
//! machine without them renders the honest "unavailable" states, which is
//! itself worth seeing.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, AssetSource, Bounds, Entity, HeadlessAppContext, Result,
    SharedString, WindowBounds, WindowOptions, px, size, test::TestWindowExt as _,
};

use g5kbd_gui::panel::{Panel, View};

/// The kit's icon bundle, so components render their glyphs offscreen too.
struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}

fn output_dir() -> PathBuf {
    let dir = std::env::var_os("G5KBD_SNAPSHOT_DIR").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("snapshots")
        },
        PathBuf::from,
    );
    std::fs::create_dir_all(&dir).expect("create snapshot directory");
    dir
}

fn headless() -> HeadlessAppContext {
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(gpui_kit::init);
    // The test platform reports `WindowAppearance::Light`, which would render
    // the component widgets in light mode inside our dark cards. The app
    // applies the same theme override in `main.rs`.
    cx.update(g5kbd_gui::theme::apply_kit_theme);
    cx
}

/// Open the panel at its real window size, pinned to `view`.
fn open_panel(view: View) -> (HeadlessAppContext, AnyWindowHandle, Entity<Panel>) {
    let mut cx = headless();
    let (handle, panel) = cx
        .update(|cx: &mut App| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(1040.), px(680.)),
                    })),
                    focus: false,
                    show: false,
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| Panel::new_for_snapshot(window, cx, view)),
            )
        })
        .expect("open headless window");
    (cx, handle, panel)
}

fn render(cx: &mut HeadlessAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
    })
    .expect("render a frame");
}

/// Render, then let the clock run past the longest transition and render again.
///
/// The panel fades its content in over `SWITCH`, so a single frame captures a
/// half-transparent pane. The motion helpers sample the background executor's
/// clock, which `advance_clock` moves, so settling is deterministic here — the
/// same reason the scheduler exists.
fn settle(cx: &mut HeadlessAppContext, handle: AnyWindowHandle) {
    render(cx, handle);
    cx.advance_clock(g5kbd_gui::motion::SWITCH * 2);
    render(cx, handle);
}

#[test]
fn renders_every_view() {
    for (view, name) in [
        (View::Home, "home"),
        (View::Lighting, "lighting"),
        (View::Profiles, "profiles"),
        (View::Fans, "fans"),
    ] {
        let (mut cx, handle, _) = open_panel(view);
        settle(&mut cx, handle);

        // CI runners have no GPU, so `WgpuHeadlessRenderer::new()` may not come
        // up. Skipping loudly beats failing the build for a picture.
        let Ok(image) = cx.capture_screenshot(handle) else {
            println!("snapshot: skipped {name} (no headless renderer available)");
            continue;
        };
        let path = output_dir().join(format!("{name}.png"));
        image.save(&path).expect("write snapshot");
        println!("snapshot: {}", path.display());
    }
}

// ---------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------

/// The sidebar rows are plain divs. Before they carried a `FocusHandle` and a
/// key handler they were reachable by mouse only, so a keyboard user could not
/// change view at all. This is the regression guard for that.
#[test]
fn sidebar_is_operable_from_the_keyboard() {
    let (mut cx, handle, panel) = open_panel(View::Home);
    render(&mut cx, handle);

    cx.update_window(handle, |_, window, cx| {
        let row = panel.read(cx).nav_focus[1].clone();
        row.focus(window, cx);
        window.press("enter", cx);
    })
    .expect("focus the Lighting row and press enter");

    assert_eq!(cx.update(|app| panel.read(app).view), View::Lighting);
}

/// Space must work as well as Enter, the way a native button does.
#[test]
fn sidebar_rows_activate_on_space() {
    let (mut cx, handle, panel) = open_panel(View::Home);
    render(&mut cx, handle);

    cx.update_window(handle, |_, window, cx| {
        let row = panel.read(cx).nav_focus[3].clone();
        row.focus(window, cx);
        window.press("space", cx);
    })
    .expect("focus the Fans row and press space");

    assert_eq!(cx.update(|app| panel.read(app).view), View::Fans);
}

/// Tab has to move focus into the panel at all; otherwise the handles the tests
/// above focus by hand are unreachable in real use.
#[test]
fn tab_moves_focus_into_the_panel() {
    let (mut cx, handle, _) = open_panel(View::Home);
    render(&mut cx, handle);

    let before = cx
        .update_window(handle, |_, window, cx| window.focused(cx))
        .expect("read the focused handle");
    assert!(before.is_none(), "nothing should be focused to begin with");

    cx.update_window(handle, |_, window, cx| {
        window.focus_next(cx);
    })
    .expect("move to the next tab stop");

    let after = cx
        .update_window(handle, |_, window, cx| window.focused(cx))
        .expect("read the focused handle");
    assert!(after.is_some(), "Tab must land on a control");
}

/// Shift+Tab walks back out again.
#[test]
fn shift_tab_moves_focus_back() {
    let (mut cx, handle, _) = open_panel(View::Home);
    render(&mut cx, handle);

    cx.update_window(handle, |_, window, cx| {
        window.focus_next(cx);
    })
    .expect("first tab stop");
    let first = cx
        .update_window(handle, |_, window, cx| window.focused(cx))
        .expect("read");

    cx.update_window(handle, |_, window, cx| {
        window.focus_prev(cx);
    })
    .expect("shift+tab");
    let back = cx
        .update_window(handle, |_, window, cx| window.focused(cx))
        .expect("read");

    assert_ne!(first, back, "Shift+Tab must not stay on the same stop");
}

// ---------------------------------------------------------------------------
// Motion
// ---------------------------------------------------------------------------

/// A view switch arms the fade, and switching to the view already showing does
/// not. The fade is keyed on `view_seq`, so a stray bump would replay it.
#[test]
fn switching_views_arms_the_fade() {
    let (mut cx, handle, panel) = open_panel(View::Home);
    settle(&mut cx, handle);

    let start = cx.update(|app| panel.read(app).view_seq);
    assert_eq!(start, 0, "the first view must not animate on open");

    cx.update_window(handle, |_, _window, cx| {
        panel.update(cx, |panel, cx| panel.set_view(View::Fans, cx));
    })
    .expect("switch to Fans");
    settle(&mut cx, handle);

    cx.update_window(handle, |_, _window, cx| {
        panel.update(cx, |panel, cx| panel.set_view(View::Fans, cx));
    })
    .expect("switch to Fans again");
    render(&mut cx, handle);

    assert_eq!(
        cx.update(|app| panel.read(app).view_seq),
        start + 1,
        "re-selecting the current view must not re-arm the fade"
    );
    assert_eq!(cx.update(|app| panel.read(app).view), View::Fans);
}

/// The content column is drawn at reduced opacity on the frame a switch
/// happens and fully opaque once the fade has run. This is the difference
/// between a transition and a no-op, checked as pixels.
#[test]
fn the_view_fades_in_rather_than_snapping() {
    let (mut cx, handle, panel) = open_panel(View::Home);
    settle(&mut cx, handle);
    let Ok(before) = cx.capture_screenshot(handle) else {
        println!("fade: skipped (no headless renderer available)");
        return;
    };

    cx.update_window(handle, |_, _window, cx| {
        panel.update(cx, |panel, cx| panel.set_view(View::Profiles, cx));
    })
    .expect("switch view");
    render(&mut cx, handle);

    // Same view, clock not advanced: the first frame of the fade.
    cx.update_window(handle, |_, _window, cx| {
        panel.update(cx, |panel, cx| panel.set_view(View::Profiles, cx));
    })
    .expect("no-op switch to keep the generation put");
    render(&mut cx, handle);
    let mid = match cx.capture_screenshot(handle) {
        Ok(image) => image,
        Err(_) => return,
    };

    settle(&mut cx, handle);
    let after = match cx.capture_screenshot(handle) {
        Ok(image) => image,
        Err(_) => return,
    };

    assert_ne!(
        mid, after,
        "the fade must leave the first frame dimmer than the settled one"
    );
    assert_ne!(
        before, after,
        "switching views must actually change what is on screen"
    );
}

/// Reduced motion has to collapse the transition, not merely shorten it: with
/// the flag set, running the clock past `SWITCH` must not change a single
/// pixel, because the fade was already at its end state on the first frame.
#[test]
fn reduced_motion_leaves_the_fade_settled() {
    let (mut cx, handle, panel) = open_panel(View::Home);
    cx.update(|app| app.set_reduce_motion(true));
    settle(&mut cx, handle);

    cx.update_window(handle, |_, _window, cx| {
        panel.update(cx, |panel, cx| panel.set_view(View::Profiles, cx));
    })
    .expect("switch view");
    // Deliberately no clock advance: this is the first frame after the switch.
    render(&mut cx, handle);
    let first = match cx.capture_screenshot(handle) {
        Ok(image) => image,
        Err(_) => {
            println!("reduced motion: skipped (no headless renderer available)");
            return;
        }
    };

    settle(&mut cx, handle);
    let settled = cx
        .capture_screenshot(handle)
        .expect("capture the settled frame");

    assert!(
        first.to_vec() == settled.to_vec(),
        "with reduced motion the first frame must already be the final one"
    );
}
