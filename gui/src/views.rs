// The four views. Each renders from `Panel` state; actions call back into
// Panel methods, which own the async plumbing.
//
// Every view takes a `Breakpoint` and lays itself out in two lanes when the
// window is wide, one when it is not. That is the whole of the responsive
// story: the views know how to stack themselves, and `layout` knows when.

use gpui_kit::{
    Context as GpuiContext, IntoElement, ParentElement, SharedString, Styled, Window,
    component::button::Button,
    component::input::InputState,
    component::slider::Slider,
    component::{Icon, IconName},
    div,
    prelude::*,
    px,
};

use crate::backend::{self, fan::FanMode};
use crate::layout::Breakpoint;
use crate::motion;
use crate::panel::{Panel, View};
use crate::theme;
use crate::widgets::{self, Section};
use gpui_kit::component::scroll::ScrollableElement as _;

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// The glyph each sidebar row collapses to. All four are in the kit's default
/// icon bundle, so they resolve without adding an asset source; the full
/// Lucide catalogue exists in the crate but only the default subset is
/// embedded for offline use.
fn nav_icon(index: usize) -> IconName {
    match index {
        0 => IconName::LayoutDashboard,
        1 => IconName::Palette,
        2 => IconName::Folder,
        _ => IconName::Cpu,
    }
}

/// Lay `primary` and `secondary` out side by side when there is room, stacked
/// when there is not.
///
/// The two lanes get 2:1 rather than 1:1 because the left one always carries
/// the thing with more to show (the keyboard, the mode chips, the save form).
///
/// `flex_basis(0)` is what makes the ratio mean anything — without it a lane
/// keeps its intrinsic width and only the leftover space is shared. `min_w(0)`
/// is just as load-bearing: a flex item's automatic minimum size is its
/// min-content width, so the curve editor's five slider columns would floor the
/// right lane at ~1000 px and starve the left one down to whatever was left,
/// which is how the first attempt at this rendered 1:4 instead of 2:1.
fn lanes(
    layout: Breakpoint,
    primary: Vec<gpui_kit::AnyElement>,
    secondary: Vec<gpui_kit::AnyElement>,
) -> gpui_kit::AnyElement {
    let column = |grow: f32, children: Vec<gpui_kit::AnyElement>| {
        div()
            .flex()
            .flex_col()
            .gap(layout.gap())
            .flex_grow(grow)
            .flex_basis(px(0.))
            .min_w(px(0.))
            .min_h(px(0.))
            .children(children)
    };
    if layout.lanes() == 1 {
        return column(1., primary)
            .child(column(1., secondary))
            .w_full()
            .into_any_element();
    }
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_start()
        .gap(layout.gap())
        .child(column(2., primary))
        .child(column(1., secondary))
        .into_any_element()
}

fn sidebar(
    panel: &mut Panel,
    layout: Breakpoint,
    window: &mut Window,
    cx: &mut GpuiContext<Panel>,
) -> gpui_kit::AnyElement {
    let active = panel.view;
    let labels: [(&'static str, &'static str, bool); 4] = [
        ("Overview", "state · backend", active == View::Home),
        ("Lighting", "colour · effects", active == View::Lighting),
        ("Profiles", "saved sets", active == View::Profiles),
        ("Fans", "modes · curves", active == View::Fans),
    ];
    let labelled = layout.sidebar_labels();

    div()
        .w(layout.sidebar_w())
        .h_full()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(theme::LINE)
        .bg(theme::FIELD)
        .py_3()
        .gap_0p5()
        // Collapsed, the rows are icon-only and the gutter between them and the
        // window edge has to shrink with them, or the icons float in the middle
        // of a wide empty band.
        .when(labelled, |el| el.px_3())
        .when(!labelled, |el| el.px_2())
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .when(labelled, |el| el.px_2().pb_3())
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::BOLD)
                        .text_color(theme::TEXT)
                        // The wordmark is the first thing to go: at 64 px it
                        // would be clipped, and the icon column reads as a rail
                        // without it.
                        .when(labelled, |el| el.child("g5kbd"))
                        .when(!labelled, |el| {
                            el.child(
                                Icon::new(IconName::Settings)
                                    .size(px(20.))
                                    .text_color(theme::hsla(theme::ACCENT, 1.)),
                            )
                        }),
                )
                .when(labelled, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(theme::FAINT)
                            .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                    )
                }),
        )
        .children(
            labels
                .iter()
                .enumerate()
                .map(|(i, (label, sub, is_active))| {
                    let target = match *label {
                        "Overview" => View::Home,
                        "Lighting" => View::Lighting,
                        "Profiles" => View::Profiles,
                        _ => View::Fans,
                    };
                    let focus = panel.nav_focus[i].clone();
                    // `.hover()` applies its styles the instant the hitbox flips,
                    // which is what made the sidebar feel like it snapped. Easing
                    // the fill instead needs the pointer position as a value the
                    // row can transition toward, so the row is told where it is.
                    let warmth = motion::hover((i, "nav"), panel.hover_nav == Some(i), window, cx);
                    let resting = if *is_active {
                        theme::mix(theme::PANEL2, theme::ACCENT, 0.14)
                    } else {
                        theme::FIELD
                    };
                    let hovered = if *is_active {
                        theme::mix(theme::PANEL2, theme::ACCENT, 0.26)
                    } else {
                        theme::PANEL
                    };
                    div()
                        .id(*label)
                        .px_2()
                        .py_1p5()
                        .rounded_sm()
                        .cursor_pointer()
                        .flex()
                        .flex_col()
                        .when(labelled, |el| el.items_start())
                        // Collapsed, the row is a square button: without a fixed
                        // height and centred content the icon sits off-axis in a
                        // strip only two icons wide.
                        .when(!labelled, |el| {
                            el.h(layout.sidebar_w() - px(28.))
                                .justify_center()
                                .items_center()
                        })
                        .bg(motion::blend(resting, hovered, warmth))
                        // The web panel marked the active row with an accent-tinted
                        // fill, a hairline border and an accent rule on the left. The
                        // port had a flat grey fill, which is why the sidebar read as
                        // "nothing is selected" at a glance.
                        .border_l_2()
                        .when(*is_active, |el| el.border_color(theme::ACCENT))
                        .when(!*is_active, |el| {
                            el.border_color(gpui_kit::rgba(0x00000000))
                        })
                        .on_hover(cx.listener(move |panel, on, _window, cx| {
                            panel.set_hover_nav(i, *on, cx);
                        }))
                        // Keyboard support: a plain div with on_click is invisible to
                        // the tab ring and announces nothing to a screen reader.
                        .track_focus(&focus)
                        .role(accesskit::Role::Tab)
                        .focus_visible(|style| {
                            style.border_color(theme::mix(theme::ACCENT, theme::WHITE, 0.3))
                        })
                        .on_key_down(cx.listener(
                            move |panel, event: &gpui_kit::KeyDownEvent, _window, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    panel.set_view(target, cx);
                                    cx.stop_propagation();
                                }
                            },
                        ))
                        .on_click(cx.listener(move |panel, _event, _window, cx| {
                            panel.set_view(target, cx);
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(Icon::new(nav_icon(i)).size(px(16.)).text_color(
                                    theme::hsla(
                                        if *is_active {
                                            theme::ACCENT
                                        } else {
                                            theme::MUTED
                                        },
                                        1.,
                                    ),
                                ))
                                // In the collapsed rail the icon has to stay
                                // centred, so the label is dropped rather than
                                // the row re-aligned.
                                .when(labelled, |el| {
                                    el.child(
                                        div()
                                            .text_sm()
                                            .text_color(if *is_active {
                                                theme::ACCENT
                                            } else {
                                                theme::TEXT
                                            })
                                            .child(*label),
                                    )
                                }),
                        )
                        .when(labelled, |el| {
                            el.child(div().text_xs().text_color(theme::FAINT).child(*sub))
                        })
                        .into_any_element()
                }),
        )
        // the watchdog chip lives in the sidebar footer, like the web panel
        .child(div().flex_1())
        .children(match (panel.watchdog, panel.fan_error.is_some()) {
            (Some(true), _) => Some(
                div()
                    .text_xs()
                    .text_color(theme::OK)
                    .when(labelled, |el| el.px_2().child("daemon on"))
                    // Collapsed there is no room for the word, so the status
                    // becomes the breathing dot the sidebar already uses
                    // elsewhere. It is the same signal in less space.
                    .when(!labelled, |el| {
                        el.flex()
                            .justify_center()
                            .child(div().size(px(7.)).rounded_full().bg(theme::OK))
                    }),
            ),
            (Some(false), _) => Some(
                div()
                    .text_xs()
                    .text_color(theme::WARN)
                    .when(labelled, |el| el.px_2().child("daemon off"))
                    .when(!labelled, |el| {
                        el.flex()
                            .justify_center()
                            .child(div().size(px(7.)).rounded_full().bg(theme::WARN))
                    }),
            ),
            _ => None,
        })
        .into_any_element()
}

fn error_banner(panel: &Panel) -> gpui_kit::AnyElement {
    let msg = panel
        .err
        .as_ref()
        .or(panel.fan_error.as_ref())
        .map(String::as_str);
    match msg {
        None => div().into_any_element(),
        Some(text) => div()
            .rounded_sm()
            .border_1()
            .border_color(theme::mix(theme::PANEL, theme::DANGER, 0.35))
            .bg(theme::mix(theme::PANEL, theme::DANGER, 0.12))
            .px_3()
            .py_2()
            .text_sm()
            .text_color(theme::DANGER)
            .child(text.to_string())
            .into_any_element(),
    }
}

fn view_header(view: View) -> gpui_kit::AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .text_xl()
                .font_weight(gpui_kit::FontWeight::BOLD)
                .text_color(theme::TEXT)
                .child(view.title()),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme::FAINT)
                .child(view.subtitle()),
        )
        .into_any_element()
}

/// A small mode chip used by the Fans view.
fn mode_chip(
    panel: &Panel,
    index: usize,
    mode: FanMode,
    window: &mut Window,
    cx: &mut GpuiContext<Panel>,
) -> gpui_kit::AnyElement {
    let active = panel
        .fan
        .as_ref()
        .and_then(|f| FanMode::from_status(&f.mode))
        .map(|m| m == mode)
        .unwrap_or(false);
    let focus = panel.mode_focus[index].clone();
    // Same reasoning as the sidebar rows: the border and fill ease toward the
    // pointer rather than snapping to it.
    let warmth = motion::hover((index, "mode"), panel.hover_mode == Some(index), window, cx);
    // Opaque tints, not `rgba(0x00aaff22)`: `mix` forces alpha to 1, so a
    // translucent accent here would turn the selected chip into a solid blue
    // slab with its own sub-label unreadable on top of it.
    let resting = if active {
        theme::mix(theme::FIELD, theme::ACCENT, 0.16)
    } else {
        theme::FIELD
    };
    let hovered = if active {
        theme::mix(theme::FIELD, theme::ACCENT, 0.26)
    } else {
        theme::PANEL2
    };
    let edge = if active {
        theme::ACCENT
    } else {
        motion::blend(theme::LINE, theme::LINE_STRONG, warmth)
    };
    div()
        .id(SharedString::from(mode.label()))
        .cursor_pointer()
        .px_3()
        .py_2()
        .rounded_sm()
        .border_1()
        .flex()
        .flex_col()
        .gap_0p5()
        .border_color(edge)
        .bg(motion::blend(resting, hovered, warmth))
        .on_hover(cx.listener(move |panel, on, _window, cx| {
            panel.set_hover_mode(index, *on, cx);
        }))
        .track_focus(&focus)
        .role(accesskit::Role::RadioButton)
        .focus_visible(|style| style.border_color(theme::mix(theme::ACCENT, theme::WHITE, 0.3)))
        .on_key_down(
            cx.listener(move |panel, event: &gpui_kit::KeyDownEvent, _window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    panel.fan_action(vec!["mode".into(), mode.as_cli_arg().to_string()], cx);
                    cx.stop_propagation();
                }
            }),
        )
        .on_click(cx.listener(move |panel, _event, _window, cx| {
            panel.fan_action(vec!["mode".into(), mode.as_cli_arg().to_string()], cx);
        }))
        .child(
            div()
                .text_sm()
                .text_color(if active { theme::ACCENT } else { theme::TEXT })
                .child(mode.label()),
        )
        .child(div().text_xs().text_color(theme::FAINT).child(match mode {
            FanMode::Auto => "firmware curve",
            FanMode::Turbo => "full speed",
            FanMode::Silent => "quiet curve",
            FanMode::MaxQ => "quietest curve",
            FanMode::Custom => "your curve",
        }))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Lighting
// ---------------------------------------------------------------------------

fn lighting(
    panel: &mut Panel,
    layout: Breakpoint,
    window: &mut Window,
    cx: &mut GpuiContext<Panel>,
) -> gpui_kit::AnyElement {
    let hex_now = theme::hex_of(panel.accent());
    // The readout is just a word, which reads as static even when the keyboard
    // is mid-breathe. The web had a pulsing dot next to it; this is that dot,
    // breathing over `BREATH`. Under reduced motion it samples the end of the
    // animation instead and sits still, which is the point of it.
    let live = motion::breath("effect-live", window, cx);

    lanes(
        layout,
        vec![
            widgets::keyboard_preview(panel.accent(), panel.glow(), panel.kbd.enabled)
                .into_any_element(),
            Section::new("Colour", SharedString::from(format!("#{hex_now}")))
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .child(slider_row("R", &panel.ch_red, cx))
                        .child(slider_row("G", &panel.ch_green, cx))
                        .child(slider_row("B", &panel.ch_blue, cx)),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .items_center()
                        .child(input_compact(&panel.hex_input))
                        .child(Button::new("apply-hex").label("Apply").compact().on_click(
                            cx.listener(|panel, _event, _window, cx| {
                                let hex = panel.hex_input.read(cx).value().to_string();
                                if let Some(c) = theme::parse_hex(&hex) {
                                    let hex = theme::hex_of(c);
                                    panel.kbd_action(vec!["color".into(), hex], cx);
                                } else {
                                    panel.err = Some("hex must be 6 digits (rrggbb)".into());
                                    cx.notify();
                                }
                            }),
                        ))
                        .children(
                            [
                                ("red", 0xff0000u32),
                                ("green", 0x00ff00),
                                ("blue", 0x0000ff),
                                ("white", 0xffffff),
                                ("off", 0x000000),
                            ]
                            .iter()
                            .map(|(name, hex)| {
                                Button::new(SharedString::from(*name))
                                    .label(*name)
                                    .compact()
                                    .on_click(cx.listener(move |panel, _event, _window, cx| {
                                        let hex = format!("{hex:06x}");
                                        panel.kbd_action(vec!["color".into(), hex], cx);
                                    }))
                                    .into_any_element()
                            }),
                        ),
                )
                .into_any_element(),
        ],
        vec![
            Section::new(
                "Brightness",
                SharedString::from(format!("{}%", panel.kbd.brightness)),
            )
            .child(slider_full(&panel.brightness, cx))
            .into_any_element(),
            Section::new("Power", if panel.kbd.enabled { "on" } else { "off" })
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(Button::new("power-on").label("On").on_click(cx.listener(
                            |panel, _e, _w, cx| {
                                panel.kbd_action(vec!["on".into()], cx);
                            },
                        )))
                        .child(Button::new("power-off").label("Off").on_click(cx.listener(
                            |panel, _e, _w, cx| {
                                panel.kbd_action(vec!["off".into()], cx);
                            },
                        ))),
                )
                .into_any_element(),
            Section::new("Effects", panel.effect_mode.unwrap_or("static"))
                .when(panel.effect_mode.is_some(), |el| {
                    el.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size(px(7.))
                                    .rounded_full()
                                    .bg(theme::OK)
                                    .opacity(0.35 + 0.65 * live),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::FAINT)
                                    .child(format!("{} running", panel.effect_mode.unwrap_or(""))),
                            ),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .items_center()
                        .child(
                            Button::new("fx-breathe")
                                .label("Breathe")
                                .on_click(cx.listener(|panel, _e, _w, cx| {
                                    let speed = panel.speed.read(cx).value().start() as u8;
                                    panel.effect_mode = Some("breathe");
                                    panel.kbd_action(
                                        vec![
                                            "effect".into(),
                                            "breathe".into(),
                                            "--speed".into(),
                                            speed.to_string(),
                                            "--bg".into(),
                                        ],
                                        cx,
                                    );
                                })),
                        )
                        .child(Button::new("fx-cycle").label("Cycle").on_click(cx.listener(
                            |panel, _e, _w, cx| {
                                let speed = panel.speed.read(cx).value().start() as u8;
                                panel.effect_mode = Some("cycle");
                                panel.kbd_action(
                                    vec![
                                        "effect".into(),
                                        "cycle".into(),
                                        "--speed".into(),
                                        speed.to_string(),
                                        "--bg".into(),
                                    ],
                                    cx,
                                );
                            },
                        )))
                        .child(Button::new("fx-stop").label("Stop").on_click(cx.listener(
                            |panel, _e, _w, cx| {
                                panel.effect_mode = None;
                                panel.kbd_action(vec!["effect".into(), "stop".into()], cx);
                            },
                        )))
                        .child(div().w_2())
                        .child(div().text_xs().text_color(theme::FAINT).child("speed"))
                        .child(slider_inline(&panel.speed, cx)),
                )
                .into_any_element(),
        ],
    )
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

fn profiles(
    panel: &mut Panel,
    layout: Breakpoint,
    cx: &mut GpuiContext<Panel>,
) -> gpui_kit::AnyElement {
    let rows: Vec<String> = panel.profiles.clone();
    let rows: Vec<(String, String)> = rows
        .iter()
        .map(|row| match row.split_once('\t') {
            Some((n, l)) => (n.to_string(), l.to_string()),
            None => (row.clone(), String::new()),
        })
        .collect();

    lanes(
        layout,
        vec![
            Section::new("Save the current look", "colour · brightness · power")
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .items_center()
                        .child(input_compact(&panel.profile_name))
                        .child(
                            Button::new("profile-save")
                                .label("Save")
                                .on_click(cx.listener(|panel, _e, _w, cx| {
                                    let name =
                                        panel.profile_name.read(cx).value().trim().to_string();
                                    if name.is_empty() {
                                        panel.err = Some("give the profile a name".into());
                                        cx.notify();
                                        return;
                                    }
                                    panel.profile_name.update(cx, |_s, _cx| {});
                                    panel.kbd_action(
                                        vec!["profile".into(), "save".into(), name],
                                        cx,
                                    );
                                    panel.refresh_profiles(cx);
                                })),
                        ),
                )
                .into_any_element(),
        ],
        vec![
            Section::new(
                "Saved profiles",
                SharedString::from(format!("{} profiles", rows.len())),
            )
            .children(if rows.is_empty() {
                vec![
                    div()
                        .text_sm()
                        .text_color(theme::FAINT)
                        .child("nothing saved yet")
                        .into_any_element(),
                ]
            } else {
                rows.iter()
                    .map(|(name, label)| {
                        // owned, because the button callbacks are 'static
                        let name: String = name.clone();
                        let label: String = label.clone();
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .gap_3()
                            .py_1()
                            .border_b_1()
                            .border_color(theme::LINE)
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme::TEXT)
                                            .child(name.to_string()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::FAINT)
                                            .child(label.to_string()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child({
                                        let name = name.clone();
                                        Button::new(SharedString::from(format!("apply-{name}")))
                                            .label("Apply")
                                            .compact()
                                            .on_click(cx.listener(move |panel, _e, _w, cx| {
                                                panel.kbd_action(
                                                    vec![
                                                        "profile".into(),
                                                        "apply".into(),
                                                        name.clone(),
                                                    ],
                                                    cx,
                                                );
                                            }))
                                    })
                                    .child({
                                        let name = name.clone();
                                        Button::new(SharedString::from(format!("del-{name}")))
                                            .label("Delete")
                                            .compact()
                                            .on_click(cx.listener(move |panel, _e, _w, cx| {
                                                panel.kbd_action(
                                                    vec![
                                                        "profile".into(),
                                                        "delete".into(),
                                                        name.clone(),
                                                    ],
                                                    cx,
                                                );
                                                panel.refresh_profiles(cx);
                                            }))
                                    }),
                            )
                            .into_any_element()
                    })
                    .collect()
            })
            .into_any_element(),
        ],
    )
}

// ---------------------------------------------------------------------------
// Fans
// ---------------------------------------------------------------------------

/// What the fan panel knows right now. `panel.fan` is `None` both while the
/// first `g5fan status` is still in flight and when `g5fan` is genuinely
/// missing, and the old code reported the second as the first — so opening
/// the panel on a healthy machine flashed "g5fan is not installed" for three
/// seconds.
enum FanState {
    Loading,
    Missing(String),
    Ready(backend::fan::RawFanStatus),
}

fn fan_state(panel: &Panel) -> FanState {
    if let Some(f) = panel.fan.clone() {
        return FanState::Ready(f);
    }
    match panel.fan_error.clone() {
        Some(error) => FanState::Missing(error),
        None => FanState::Loading,
    }
}

/// A card for the two states where there is nothing to plot yet.
fn fan_unavailable(panel: &Panel, state: &FanState) -> gpui_kit::AnyElement {
    let (readout, message) = match state {
        FanState::Loading => ("reading…", "asking g5fan for a snapshot".to_string()),
        FanState::Missing(error) => (
            "unavailable",
            format!("{error} — run `g5fan doctor` on a terminal"),
        ),
        FanState::Ready(_) => unreachable!("fan_unavailable takes the states without a snapshot"),
    };
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(error_banner(panel))
        .child(
            Section::new("Fans", readout)
                .child(div().text_sm().text_color(theme::FAINT).child(message)),
        )
        .into_any_element()
}

fn fans(
    panel: &mut Panel,
    layout: Breakpoint,
    window: &mut Window,
    cx: &mut GpuiContext<Panel>,
) -> gpui_kit::AnyElement {
    let f = match fan_state(panel) {
        FanState::Ready(f) => f,
        state => return fan_unavailable(panel, &state),
    };

    let stale = f.stale || !f.daemon;
    let curve_mode = FanMode::from_status(&f.mode)
        .map(|m| m.is_curve())
        .unwrap_or(false);

    lanes(
        layout,
        vec![
            Section::new("Mode", SharedString::from(f.mode.clone()))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .children(
                            FanMode::ALL
                                .iter()
                                .enumerate()
                                .map(|(i, m)| mode_chip(panel, i, *m, window, cx)),
                        ),
                )
                .when(stale, |el| {
                    el.child(
                        div()
                            .rounded_sm()
                            .border_1()
                            .border_color(theme::mix(theme::PANEL, theme::WARN, 0.35))
                            .bg(theme::mix(theme::PANEL, theme::WARN, 0.12))
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(theme::WARN)
                            .child(format!(
                                "the fan daemon is not running — values are {} s old and nothing is evaluating curves. Start it with: sudo systemctl start g5fan-watchdog.service",
                                f.age_s as u32
                            )),
                    )
                })
                .when(curve_mode, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(theme::FAINT)
                            .child(format!(
                                "driven by the daemon every 5 s · releases both fans to the firmware curve at {} °C",
                                f.ceiling_c as i32
                            )),
                    )
                })
                .into_any_element(),
            Section::new("Fans", SharedString::from(format!("ceiling {} °C", f.ceiling_c as i32)))
                .child(gauge_grid(&f, "fans", layout, window, cx))
                .into_any_element(),
        ],
        vec![
            Section::new(
                "Manual duty",
                if f.mode == "manual" {
                    SharedString::from(format!("{}%", f.manual_duty.unwrap_or(0)))
                } else {
                    "not active".into()
                },
            )
            .child(slider_full(&panel.duty, cx))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("back-auto")
                            .label("Back to auto")
                            .on_click(cx.listener(|panel, _e, _w, cx| {
                                panel.fan_action(vec!["auto".into()], cx);
                            })),
                    )
                    .child(
                        Button::new("turbo")
                            .label("Turbo")
                            .on_click(cx.listener(|panel, _e, _w, cx| {
                                panel.fan_action(vec!["turbo".into()], cx);
                            })),
                    ),
            )
            .into_any_element(),
            curve_editor(panel, layout, cx, &f),
        ],
    )
}

/// The fan duty and temperature readouts as one wrapping grid.
///
/// Overview and Fans share this so the two views cannot drift apart: the same
/// snapshot has to read the same way on both, or the user has to learn the
/// layout twice.
fn gauge_grid(
    f: &backend::fan::RawFanStatus,
    channel: &'static str,
    layout: Breakpoint,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) -> gpui_kit::AnyElement {
    let min_w = layout.gauge_min_w();
    div()
        .flex()
        .flex_wrap()
        .gap_6()
        .children(f.fans.iter().enumerate().map(|(i, fan)| {
            widgets::labeled_gauge(
                channel,
                i,
                &fan.label,
                format!("{}%", fan.duty_pct.unwrap_or(0)),
                fan.rpm
                    .map(|rpm| format!("{rpm} rpm"))
                    .unwrap_or_else(|| "stopped".into()),
                fan.duty_pct.unwrap_or(0) as f32 / 100.,
                theme::ACCENT.into(),
                min_w,
                window,
                cx,
            )
        }))
        .children(
            [("CPU", f.cpu_temp_c), ("GPU", f.gpu_temp_c)]
                .iter()
                .enumerate()
                .filter_map(|(i, (label, temp))| {
                    temp.map(|t| {
                        widgets::labeled_gauge(
                            channel,
                            // Offset past the fans so the two never share a bar
                            // id inside one view.
                            100 + i,
                            label,
                            format!("{t} °C"),
                            format!("ceiling {} °C", f.ceiling_c as i32),
                            t as f32 / f.ceiling_c as f32,
                            widgets::temp_color(t),
                            min_w,
                            window,
                            cx,
                        )
                    })
                }),
        )
        .into_any_element()
}

fn curve_editor(
    panel: &mut Panel,
    layout: Breakpoint,
    cx: &mut GpuiContext<Panel>,
    f: &backend::fan::RawFanStatus,
) -> gpui_kit::AnyElement {
    let stored = backend::fan::format_curve(&f.custom_curve);
    let current = backend::fan::format_curve(&panel.curve);
    let unsaved = stored != current;
    let problem = backend::fan::curve_problem(&panel.curve);
    let using_custom = f.mode == "custom";

    Section::new(
        "Fan curve",
        SharedString::from(if unsaved {
            "unsaved"
        } else if using_custom {
            "in use"
        } else {
            "5 points"
        }),
    )
    .child(
        div()
            .w_full()
            .text_xs()
            .text_color(theme::FAINT)
            .child("the daemon turns this ramp into duty: at or below T1 the fans hold D1, at or above T5 they hold D5, and between points the duty is interpolated. Duty must never fall as it gets hotter."),
    ).child(
            div()
                .flex()
                .flex_wrap()
                .gap_4()
                .children((0..5).map(|i| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    // Below the wide layout a lane is the whole pane and five
                    // columns of sliders would leave each one unreadably short,
                    // so they wrap two or three to a row instead.
                    .min_w(if layout.lanes() == 2 { px(150.) } else { px(120.) })
                    .flex_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::MUTED)
                            .child(format!("T{} °C / D{} %", i + 1, i + 1)),
                    )
                    .child(slider_inline(&panel.curve_temp[i], cx))
                    .child(slider_inline(&panel.curve_duty[i], cx))
                    .into_any_element()
            })),
    )
    .children(problem.map(|p| {
        div()
            .text_sm()
            .text_color(theme::DANGER)
            .child(p)
            .into_any_element()
    }))
    .child(
        div()
            .flex()
            .gap_2()
            .child(
                Button::new("curve-apply")
                    .label(if using_custom { "Apply curve" } else { "Save and use" })
                    .on_click(cx.listener(|panel, _e, _w, cx| {
                        panel.apply_curve(cx);
                    })),
            )
            .child(
                Button::new("curve-reset")
                    .label("Reset to shipped")
                    .on_click(cx.listener(|panel, _e, window, cx| {
                        panel.reset_curve(window, cx);
                    })),
            ),
    )
    .into_any_element()
}

// ---------------------------------------------------------------------------
// Slider + input helpers
// ---------------------------------------------------------------------------

fn slider_row(
    label: &str,
    state: &gpui_kit::Entity<gpui_kit::component::slider::SliderState>,
    _cx: &mut GpuiContext<Panel>,
) -> gpui_kit::AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .flex_1()
        .child(
            div()
                .text_xs()
                .text_color(theme::MUTED)
                .child(label.to_string()),
        )
        .child(Slider::new(state))
        .into_any_element()
}

fn slider_full(
    state: &gpui_kit::Entity<gpui_kit::component::slider::SliderState>,
    _cx: &mut GpuiContext<Panel>,
) -> gpui_kit::AnyElement {
    div().w_full().child(Slider::new(state)).into_any_element()
}

fn slider_inline(
    state: &gpui_kit::Entity<gpui_kit::component::slider::SliderState>,
    _cx: &mut GpuiContext<Panel>,
) -> gpui_kit::AnyElement {
    div()
        .w_full()
        .min_w(px(96.))
        .child(Slider::new(state))
        .into_any_element()
}

fn input_compact(state: &gpui_kit::Entity<InputState>) -> gpui_kit::AnyElement {
    div()
        .w(px(140.))
        .child(gpui_kit::component::input::Input::new(state))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Panel::render
// ---------------------------------------------------------------------------

use gpui_kit::{Render, Window as GpuiWindow};

impl Render for Panel {
    fn render(&mut self, window: &mut GpuiWindow, cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let view = self.view;
        // One decision per frame, read from the window rather than tracked in
        // state: a resize has to take effect on the very next paint, and a
        // cached copy would be one frame stale at best and forever stale if the
        // observer were dropped.
        let layout = Breakpoint::for_window(window);
        // The web changed pages instantly; this fades the column in over
        // `SWITCH` instead, keyed on the view generation so returning to a view
        // replays it.
        let fade = motion::fade(self.view_seq, window, cx);

        let content = match view {
            View::Home => home(self, layout, window, cx),
            View::Lighting => lighting(self, layout, window, cx),
            View::Profiles => profiles(self, layout, cx),
            View::Fans => fans(self, layout, window, cx),
        };

        let gutter = layout.gutter();
        let column = div()
            .flex()
            .flex_col()
            .gap(layout.gap())
            .w_full()
            // Not fully transparent at 0: the first frame after a switch would
            // otherwise be a blank pane.
            .opacity(0.25 + 0.75 * fade)
            .child(view_header(view))
            .child(error_banner(self))
            .when(self.busy, |el| {
                el.child(div().text_xs().text_color(theme::FAINT).child("working…"))
            })
            .child(content);

        div()
            .id("panel-root")
            .flex()
            .size_full()
            .bg(theme::BG)
            .text_color(theme::TEXT)
            .font(theme::ui_font())
            // Tab traversal. GPUI keeps the tab ring in the window and hands it
            // out one stop at a time, so this just has to ask for the next one;
            // the root listens on the bubble phase, which means every control
            // inside it can drive it.
            .on_key_down(
                cx.listener(|_panel, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key.as_str() != "tab" {
                        return;
                    }
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }),
            )
            .child(sidebar(self, layout, window, cx))
            .child(
                div().flex_1().h_full().overflow_y_scrollbar().child(
                    div()
                        .w_full()
                        .px(gutter)
                        .py(gutter)
                        // Only the wide layout caps the column, and then it
                        // centres it. The old `max_w(940)` applied at every size,
                        // which is what left the right-hand gutter empty on any
                        // window wider than the panel's nominal size.
                        .when_some(layout.content_max_w(), |el, max| el.mx_auto().max_w(max))
                        .child(column),
                ),
            )
    }
}

// ---------------------------------------------------------------------------
// Home
// ---------------------------------------------------------------------------

fn home(
    panel: &Panel,
    layout: Breakpoint,
    window: &mut Window,
    cx: &mut GpuiContext<Panel>,
) -> gpui_kit::AnyElement {
    lanes(
        layout,
        vec![
            Section::new("Keyboard", if panel.kbd.enabled { "on" } else { "off" })
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .items_center()
                        .child(
                            // the colour swatch, glowing in the live colour
                            div()
                                .size_10()
                                .rounded_full()
                                .bg(widgets::glow_mix(
                                    theme::PANEL2,
                                    panel.accent(),
                                    panel.glow(),
                                ))
                                .border_1()
                                .border_color(theme::LINE_STRONG),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(div().text_sm().text_color(theme::TEXT).child(format!(
                                    "#{:02x}{:02x}{:02x}",
                                    panel.kbd.red, panel.kbd.green, panel.kbd.blue
                                )))
                                .child(div().text_xs().text_color(theme::FAINT).child(format!(
                                    "brightness {}% · backend {}",
                                    panel.kbd.brightness, panel.kbd.backend
                                ))),
                        ),
                )
                .child(widgets::keyboard_preview(
                    panel.accent(),
                    panel.glow(),
                    panel.kbd.enabled,
                ))
                .into_any_element(),
        ],
        vec![
            Section::new(
                "Fans",
                panel
                    .fan
                    .as_ref()
                    .map(|f| SharedString::from(f.mode.clone()))
                    .unwrap_or_else(|| "unavailable".into()),
            )
            .child(match &panel.fan {
                None if panel.fan_error.is_none() => div()
                    .text_sm()
                    .text_color(theme::FAINT)
                    .child("reading the fan snapshot…")
                    .into_any_element(),
                Some(f) => gauge_grid(f, "home", layout, window, cx),
                None => div()
                    .text_sm()
                    .text_color(theme::FAINT)
                    .child(match &panel.fan_error {
                        Some(e) => format!("{e} — run `g5fan doctor` on a terminal"),
                        None => "reading the fan snapshot…".to_string(),
                    })
                    .into_any_element(),
            })
            .into_any_element(),
        ],
    )
}
