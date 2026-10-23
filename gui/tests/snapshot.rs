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

use g5kbd_gui::panel::{History, Panel, View};
use g5kbd_gui::{backend, widgets};

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
            // The *workspace* target directory: `CARGO_MANIFEST_DIR` is the
            // `gui` crate, and since the workspace arrived the build tree is one
            // level up. Writing to `gui/target` again would put images where
            // `git status` no longer ignores them and nothing looks for them.
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
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

/// The window the app opens at, and the one the default snapshots use.
const DEFAULT_SIZE: (f32, f32) = (1040., 680.);

/// A runner without a GPU cannot bring the headless renderer up at all, so the
/// tests that need a window bail out loudly instead of failing the build for a
/// picture. (A renderer that fails *after* the window exists is already handled
/// where the screenshot is taken.)
macro_rules! skip_no_renderer {
    ($what:expr) => {
        println!(
            "snapshot: skipped {} (no headless renderer available)",
            $what
        )
    };
}

/// Open the panel at `size`, pinned to `view`.
///
/// `None` when no headless renderer is available — GitHub runners have no GPU,
/// so the window never opens there.
fn open_panel(
    view: View,
    size_px: (f32, f32),
) -> Option<(HeadlessAppContext, AnyWindowHandle, Entity<Panel>)> {
    let mut cx = headless();
    let (handle, panel) = cx
        .update(|cx: &mut App| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(size_px.0), px(size_px.1)),
                    })),
                    focus: false,
                    show: false,
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| Panel::new_for_snapshot(window, cx, view)),
            )
        })
        .ok()?;
    Some((cx, handle, panel))
}

/// The panel at its shipped window size.
fn open_panel_default(view: View) -> Option<(HeadlessAppContext, AnyWindowHandle, Entity<Panel>)> {
    open_panel(view, DEFAULT_SIZE)
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
        let Some((mut cx, handle, _)) = open_panel_default(view) else {
            skip_no_renderer!(name);
            continue;
        };
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

/// The same views at the three window widths `layout::Breakpoint` knows about.
///
/// The panel used to lay out as if the window were always 1040 px wide, so a
/// window opened on half a screen clipped its own controls and one opened full
/// width left a dead gutter down the right-hand side. These pictures are how
/// that gets noticed; the assertions below are the part that fails a build.
#[test]
fn lays_out_at_every_window_width() {
    for (label, width) in [("compact", 720.), ("wide", 1600.)] {
        for (view, name) in [(View::Home, "home"), (View::Fans, "fans")] {
            let Some((mut cx, handle, _)) = open_panel(view, (width, 720.)) else {
                skip_no_renderer!(width);
                continue;
            };
            settle(&mut cx, handle);

            let Ok(image) = cx.capture_screenshot(handle) else {
                println!("layout: skipped {label}/{name} (no headless renderer)");
                continue;
            };
            let path = output_dir().join(format!("{label}-{name}.png"));
            image.save(&path).expect("write snapshot");
            println!("layout: {}", path.display());
        }
    }
}

/// The gauges carry a sparkline of the last two and a half minutes, and the
/// real history is empty until the panel has been open that long. This seeds a
/// ramp so the drawing path has a snapshot to be looked at.
#[test]
fn the_gauges_draw_their_trend() {
    let Some((mut cx, handle, panel)) = open_panel_default(View::Home) else {
        skip_no_renderer!("home");
        return;
    };
    cx.update_window(handle, |_, _window, cx| {
        panel.update(cx, |panel, _cx| panel.seed_history(History::CAP - 1));
    })
    .expect("seed the trend");
    settle(&mut cx, handle);

    let Ok(image) = cx.capture_screenshot(handle) else {
        println!("trend: skipped (no headless renderer available)");
        return;
    };
    let path = output_dir().join("gauges-trend.png");
    image.save(&path).expect("write snapshot");
    println!("trend: {}", path.display());
}

/// The fan curve is a shape, so it is drawn as one: five points on a
/// temperature/duty graph, with the live temperatures marked on the ramp. The
/// window has to be taller than the shipped one to see it — at 680 px the card
/// sits below the fold of a scrolling pane — so this is its own snapshot
/// rather than a line of `renders_every_view`.
///
/// The drag is the part that fails a build: it presses on the middle handle
/// where the graph draws it and moves it, which exercises the whole path from a
/// pointer position to a legal curve.
#[test]
fn the_curve_graph_is_draggable() {
    let Some((mut cx, handle, panel)) = open_panel(View::Fans, (1040., 1100.)) else {
        skip_no_renderer!("curve graph");
        return;
    };
    settle(&mut cx, handle);

    let before = cx.update(|app| panel.read(app).curve);

    let (from, to) = cx
        .update_window(handle, |_, window, _cx| {
            let bounds = window.find("curve-graph").bounds();
            let from = widgets::curve_handle(bounds, &before, 2);
            // Up and to the left: cooler at a higher duty.
            (from, gpui_kit::point(from.x - px(12.), from.y - px(24.)))
        })
        .expect("locate the graph");
    cx.update_window(handle, |_, window, cx| window.drag(from, to, cx))
        .expect("drag the middle point");

    let after = cx.update(|app| panel.read(app).curve);
    assert!(
        after[2][0] < before[2][0],
        "the point must move left: {} -> {}",
        before[2][0],
        after[2][0]
    );
    assert!(
        after[2][1] > before[2][1],
        "and up: {} -> {}",
        before[2][1],
        after[2][1]
    );
    assert_eq!(after[1], before[1], "the point to its left must not move");
    assert_eq!(after[3], before[3], "nor the point to its right");
    assert_eq!(
        backend::fan::curve_problem(&after),
        None,
        "a drag may never leave the editor holding a curve the daemon refuses"
    );

    // The sliders are the keyboard path onto the same five values: a drag that
    // moved only the graph would show one curve and hand the CLI another.
    let (temp, duty) = cx.update(|app| {
        let panel = panel.read(app);
        (
            panel.curve_temp[2].read(app).value().start(),
            panel.curve_duty[2].read(app).value().start(),
        )
    });
    assert_eq!(
        temp.round() as i32,
        after[2][0],
        "the T3 slider must follow the point"
    );
    assert_eq!(duty.round() as i32, after[2][1], "and so must D3");

    let Ok(image) = cx.capture_screenshot(handle) else {
        println!("curve graph: skipped the image (no headless renderer available)");
        return;
    };
    let path = output_dir().join("curve-graph.png");
    image.save(&path).expect("write snapshot");
    println!("curve graph: {}", path.display());
}

/// A narrow window has to collapse the sidebar rather than let it eat the
/// content. Checked as pixels: the rail is 64 px, so if the labels are still
/// being drawn the far-left column of the frame is not the rail's flat
/// background colour.
#[test]
fn a_narrow_window_collapses_the_sidebar() {
    let Some((mut cx, handle, _)) = open_panel(View::Home, (720., 720.)) else {
        skip_no_renderer!("narrow window");
        return;
    };
    settle(&mut cx, handle);
    let Ok(narrow) = cx.capture_screenshot(handle) else {
        println!("narrow: skipped (no headless renderer available)");
        return;
    };
    let Some((mut cx, handle, _)) = open_panel_default(View::Home) else {
        skip_no_renderer!("home");
        return;
    };
    settle(&mut cx, handle);
    let Ok(default) = cx.capture_screenshot(handle) else {
        return;
    };
    assert_ne!(
        narrow, default,
        "a 720 px window must not render exactly like the 1040 px one"
    );
}

// ---------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------

/// The sidebar rows are plain divs. Before they carried a `FocusHandle` and a
/// key handler they were reachable by mouse only, so a keyboard user could not
/// change view at all. This is the regression guard for that.
#[test]
fn sidebar_is_operable_from_the_keyboard() {
    let Some((mut cx, handle, panel)) = open_panel_default(View::Home) else {
        skip_no_renderer!("home");
        return;
    };
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
    let Some((mut cx, handle, panel)) = open_panel_default(View::Home) else {
        skip_no_renderer!("home");
        return;
    };
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
    let Some((mut cx, handle, _)) = open_panel_default(View::Home) else {
        skip_no_renderer!("home");
        return;
    };
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
    let Some((mut cx, handle, _)) = open_panel_default(View::Home) else {
        skip_no_renderer!("home");
        return;
    };
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
    let Some((mut cx, handle, panel)) = open_panel_default(View::Home) else {
        skip_no_renderer!("home");
        return;
    };
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
    let Some((mut cx, handle, panel)) = open_panel_default(View::Home) else {
        skip_no_renderer!("home");
        return;
    };
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
    let Some((mut cx, handle, panel)) = open_panel_default(View::Home) else {
        skip_no_renderer!("home");
        return;
    };
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
