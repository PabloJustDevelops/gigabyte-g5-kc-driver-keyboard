// The four views. Each renders from `Panel` state; actions call back into
// Panel methods, which own the async plumbing.

use gpui_kit::{
    Context as GpuiContext, IntoElement, ParentElement, SharedString, Styled,
    component::button::Button, component::input::InputState, component::slider::Slider, div,
    prelude::*, px, rgb,
};

use crate::backend::{self, fan::FanMode};
use crate::panel::{Panel, View};
use crate::theme;
use crate::widgets::{self, Section};
use gpui_kit::component::scroll::ScrollableElement as _;

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

fn sidebar(panel: &mut Panel, cx: &mut GpuiContext<Panel>) -> gpui_kit::AnyElement {
    let active = panel.view;
    let labels: [(&'static str, &'static str, bool); 4] = [
        ("Overview", "state · backend", active == View::Home),
        ("Lighting", "colour · effects", active == View::Lighting),
        ("Profiles", "saved sets", active == View::Profiles),
        ("Fans", "modes · curves", active == View::Fans),
    ];

    div()
        .w(px(220.))
        .h_full()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(theme::LINE)
        .bg(theme::FIELD)
        .p_3()
        .gap_0p5()
        .child(
            div()
                .px_2()
                .pb_3()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::BOLD)
                        .text_color(theme::TEXT)
                        .child("g5kbd"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::FAINT)
                        .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                ),
        )
        .children(labels.iter().map(|(label, sub, is_active)| {
            let target = match *label {
                "Overview" => View::Home,
                "Lighting" => View::Lighting,
                "Profiles" => View::Profiles,
                _ => View::Fans,
            };
            div()
                .id(*label)
                .px_2()
                .py_1p5()
                .rounded_sm()
                .cursor_pointer()
                .flex()
                .flex_col()
                .when(*is_active, |el| el.bg(theme::PANEL2))
                .hover(|el| el.bg(theme::PANEL))
                .on_click(cx.listener(move |panel, _event, _window, cx| {
                    panel.view = target;
                    cx.notify();
                }))
                .child(
                    div()
                        .text_sm()
                        .text_color(if *is_active {
                            theme::ACCENT
                        } else {
                            theme::TEXT
                        })
                        .child(*label),
                )
                .child(div().text_xs().text_color(theme::FAINT).child(*sub))
                .into_any_element()
        }))
        // the watchdog chip lives in the sidebar footer, like the web panel
        .child(div().flex_1())
        .children(match (panel.watchdog, panel.fan_error.is_some()) {
            (Some(true), _) => Some(div().text_xs().text_color(theme::OK).child("daemon on")),
            (Some(false), _) => Some(div().text_xs().text_color(theme::WARN).child("daemon off")),
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
            .border_color(rgb(0x5c2430))
            .bg(rgb(0x241318))
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
fn mode_chip(panel: &Panel, mode: FanMode, cx: &GpuiContext<Panel>) -> gpui_kit::AnyElement {
    let active = panel
        .fan
        .as_ref()
        .and_then(|f| FanMode::from_status(&f.mode))
        .map(|m| m == mode)
        .unwrap_or(false);
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
        .when(active, |el| {
            el.border_color(theme::ACCENT)
                .bg(gpui_kit::rgba(0x00aaff22))
        })
        .when(!active, |el| el.border_color(theme::LINE).bg(theme::FIELD))
        .hover(|el| el.border_color(theme::LINE_STRONG))
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

fn lighting(panel: &mut Panel, cx: &mut GpuiContext<Panel>) -> gpui_kit::AnyElement {
    let hex_now = theme::hex_of(panel.accent());

    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(widgets::keyboard_preview(
            panel.accent(),
            panel.glow(),
            panel.kbd.enabled,
        ))
        .child(
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
                ),
        )
        .child(
            Section::new(
                "Brightness",
                SharedString::from(format!("{}%", panel.kbd.brightness)),
            )
            .child(slider_full(&panel.brightness, cx)),
        )
        .child(
            Section::new("Power", if panel.kbd.enabled { "on" } else { "off" }).child(
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
            ),
        )
        .child(
            Section::new("Effects", panel.effect_mode.unwrap_or("static")).child(
                div()
                    .flex()
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
                    .child(div().w_6())
                    .child(div().text_xs().text_color(theme::FAINT).child("speed"))
                    .child(slider_inline(&panel.speed, cx)),
            ),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

fn profiles(panel: &mut Panel, cx: &mut GpuiContext<Panel>) -> gpui_kit::AnyElement {
    let rows: Vec<String> = panel.profiles.clone();
    let rows: Vec<(String, String)> = rows
        .iter()
        .map(|row| match row.split_once('\t') {
            Some((n, l)) => (n.to_string(), l.to_string()),
            None => (row.clone(), String::new()),
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            Section::new("Save the current look", "colour · brightness · power").child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(input_compact(&panel.profile_name))
                    .child(
                        Button::new("profile-save")
                            .label("Save")
                            .on_click(cx.listener(|panel, _e, _w, cx| {
                                let name = panel.profile_name.read(cx).value().trim().to_string();
                                if name.is_empty() {
                                    panel.err = Some("give the profile a name".into());
                                    cx.notify();
                                    return;
                                }
                                panel.profile_name.update(cx, |_s, _cx| {});
                                panel.kbd_action(vec!["profile".into(), "save".into(), name], cx);
                                panel.refresh_profiles(cx);
                            })),
                    ),
            ),
        )
        .child(
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
            }),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Fans
// ---------------------------------------------------------------------------

fn fans(panel: &mut Panel, cx: &mut GpuiContext<Panel>) -> gpui_kit::AnyElement {
    let Some(f) = panel.fan.clone() else {
        return div()
            .flex()
            .flex_col()
            .gap_4()
            .child(error_banner(panel))
            .child(
                Section::new("Fans", "unavailable").child(
                    div()
                        .text_sm()
                        .text_color(theme::FAINT)
                        .child("g5fan is not installed or the snapshot is missing — run `g5fan doctor` on a terminal"),
                ),
            )
            .into_any_element();
    };

    let stale = f.stale || !f.daemon;
    let curve_mode = FanMode::from_status(&f.mode)
        .map(|m| m.is_curve())
        .unwrap_or(false);

    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(error_banner(panel))
        .child(
            Section::new("Mode", SharedString::from(f.mode.clone()))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .children(FanMode::ALL.iter().map(|m| mode_chip(panel, *m, cx))),
                )
                .when(stale, |el| {
                    el.child(
                        div()
                            .rounded_sm()
                            .border_1()
                            .border_color(rgb(0x4d3a1a))
                            .bg(rgb(0x241c0e))
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
                }),
        )
        .child(
            Section::new("Fans", SharedString::from(format!("ceiling {} °C", f.ceiling_c as i32)))
                .child(
                    div()
                        .flex()
                        .gap_6()
                        .children(f.fans.iter().map(|fan| {
                            widgets::labeled_gauge(
                                &fan.label,
                                format!("{}%", fan.duty_pct.unwrap_or(0)),
                                fan.rpm
                                    .map(|rpm| format!("{rpm} rpm"))
                                    .unwrap_or_else(|| "stopped".into()),
                                fan.duty_pct.unwrap_or(0) as f32 / 100.,
                                theme::ACCENT.into(),
                            )
                        }))
                        .children(
                            [
                                ("CPU", f.cpu_temp_c),
                                ("GPU", f.gpu_temp_c),
                            ]
                            .iter()
                            .filter_map(|(label, temp)| {
                                temp.map(|t| {
                                    widgets::labeled_gauge(
                                        label,
                                        format!("{t} °C"),
                                        format!("ceiling {} °C", f.ceiling_c as i32),
                                        t as f32 / f.ceiling_c as f32,
                                        widgets::temp_color(t),
                                    )
                                })
                            }),
                        ),
                ),
        )
        .child(
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
            ),
        )
        .child(curve_editor(panel, cx, &f))
        .into_any_element()
}

fn curve_editor(
    panel: &mut Panel,
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
            .text_xs()
            .text_color(theme::FAINT)
            .child("the daemon turns this ramp into duty: at or below T1 the fans hold D1, at or above T5 they hold D5, and between points the duty is interpolated. Duty must never fall as it gets hotter."),
    )
    .child(
        div()
            .flex()
            .gap_4()
            .children((0..5).map(|i| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .min_w(px(120.))
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
        .w(px(160.))
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
    fn render(&mut self, _window: &mut GpuiWindow, cx: &mut GpuiContext<Self>) -> impl IntoElement {
        let view = self.view;

        let content = match view {
            View::Home => home(self),
            View::Lighting => lighting(self, cx),
            View::Profiles => profiles(self, cx),
            View::Fans => fans(self, cx),
        };

        div()
            .flex()
            .size_full()
            .bg(theme::BG)
            .text_color(theme::TEXT)
            .font_family("IBM Plex Sans")
            .child(sidebar(self, cx))
            .child(
                div().flex_1().h_full().overflow_y_scrollbar().child(
                    div()
                        .flex()
                        .flex_col()
                        .p_6()
                        .gap_4()
                        .max_w(px(980.))
                        .child(view_header(view))
                        .child(error_banner(self))
                        .when(self.busy, |el| {
                            el.child(div().text_xs().text_color(theme::FAINT).child("working…"))
                        })
                        .child(content),
                ),
            )
    }
}

// ---------------------------------------------------------------------------
// Home
// ---------------------------------------------------------------------------

fn home(panel: &Panel) -> gpui_kit::AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
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
                )),
        )
        .child(
            Section::new(
                "Fans",
                panel
                    .fan
                    .as_ref()
                    .map(|f| SharedString::from(f.mode.clone()))
                    .unwrap_or_else(|| "unavailable".into()),
            )
            .child(match &panel.fan {
                Some(f) => div()
                    .flex()
                    .gap_6()
                    .children(f.fans.iter().map(|fan| {
                        widgets::labeled_gauge(
                            &fan.label,
                            format!("{}%", fan.duty_pct.unwrap_or(0)),
                            fan.rpm
                                .map(|rpm| format!("{rpm} rpm"))
                                .unwrap_or_else(|| "stopped".into()),
                            fan.duty_pct.unwrap_or(0) as f32 / 100.,
                            theme::ACCENT.into(),
                        )
                    }))
                    .children(
                        [("CPU", f.cpu_temp_c), ("GPU", f.gpu_temp_c)]
                            .iter()
                            .filter_map(|(label, temp)| {
                                temp.map(|t| {
                                    widgets::labeled_gauge(
                                        label,
                                        format!("{t} °C"),
                                        format!("ceiling {} °C", f.ceiling_c as i32),
                                        t as f32 / f.ceiling_c as f32,
                                        widgets::temp_color(t),
                                    )
                                })
                            }),
                    )
                    .into_any_element(),
                None => div()
                    .text_sm()
                    .text_color(theme::FAINT)
                    .child("g5fan is not answering — run `g5fan doctor` on a terminal")
                    .into_any_element(),
            }),
        )
        .into_any_element()
}
