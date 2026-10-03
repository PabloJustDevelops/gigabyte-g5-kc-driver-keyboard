//! Offscreen snapshots of the panel, so the UI can be looked at without a
//! display. This is the desktop counterpart of a browser test runner: it
//! renders the real `Panel` through GPUI's headless renderer and writes one
//! PNG per view to `target/snapshots/`.
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
    App, AppContext as _, AssetSource, Bounds, Entity, HeadlessAppContext, Result, SharedString,
    WindowBounds, WindowOptions, px, size, test::TestWindowExt as _,
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

/// Render one view at 1040×680 (the panel's real window size) and save it.
fn snapshot(view: View, name: &str) {
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

    let _: &Entity<Panel> = &panel;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
    })
    .expect("render a frame");

    let Ok(image) = cx.capture_screenshot(handle) else {
        println!("snapshot: skipped {name} (no headless renderer available)");
        return;
    };
    let path = output_dir().join(format!("{name}.png"));
    image.save(&path).expect("write snapshot");
    println!("snapshot: {}", path.display());
}

#[test]
fn renders_every_view() {
    // CI runners have no GPU, so `WgpuHeadlessRenderer::new()` may not come
    // up. Skipping loudly beats failing the build for a picture.
    for (view, name) in [
        (View::Home, "home"),
        (View::Lighting, "lighting"),
        (View::Profiles, "profiles"),
        (View::Fans, "fans"),
    ] {
        snapshot(view, name);
    }
}
