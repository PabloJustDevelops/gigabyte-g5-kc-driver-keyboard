// Bespoke widgets the component library does not ship: the card section used
// by every view, the fan/temperature bars, and the glowing keyboard preview.
// These are the "custom" half of the UI — the ready-made gpui-component
// widgets (Button, Slider, Switch, Input, Toggle) cover the rest.

use gpui_kit::{
    AnyElement, Hsla, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div,
    px, relative,
};

use crate::theme::{self, mix};

// ---------------------------------------------------------------------------
// Section — the card every group of controls lives in
// ---------------------------------------------------------------------------

#[derive(gpui_kit::IntoElement)]
pub struct Section {
    title: &'static str,
    readout: SharedString,
    children: Vec<gpui_kit::AnyElement>,
}

impl Section {
    pub fn new(title: &'static str, readout: impl Into<SharedString>) -> Self {
        Self {
            title,
            readout: readout.into(),
            children: Vec::new(),
        }
    }
}

impl ParentElement for Section {
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui_kit::AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Section {
    fn render(self, _window: &mut Window, _cx: &mut gpui_kit::App) -> impl IntoElement {
        div()
            .rounded_md()
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::PANEL)
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::BOLD)
                            .text_color(theme::TEXT)
                            .child(self.title),
                    )
                    .child(div().text_xs().text_color(theme::FAINT).child(self.readout)),
            )
            .children(self.children)
    }
}

// ---------------------------------------------------------------------------
// Bar — the horizontal fill used by fan duty and temperature
// ---------------------------------------------------------------------------

pub fn bar_fill(frac: f32, color: Hsla) -> AnyElement {
    div()
        .h(px(6.))
        .w_full()
        .rounded_full()
        .bg(theme::FIELD)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(frac.clamp(0., 1.)))
                .rounded_full()
                .bg(color),
        )
        .into_any_element()
}

/// Temperature colour: ok under 60 °C, warn under 75 °C, danger beyond.
pub fn temp_color(temp_c: i32) -> Hsla {
    if temp_c < 60 {
        theme::OK.into()
    } else if temp_c < 75 {
        theme::WARN.into()
    } else {
        theme::DANGER.into()
    }
}

// ---------------------------------------------------------------------------
// LabeledGauge — "CPU  52 °C" + bar, used for fans and temperatures alike
// ---------------------------------------------------------------------------

pub fn labeled_gauge(
    label: &str,
    value: String,
    sub: String,
    frac: f32,
    color: Hsla,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .min_w(px(180.))
        .flex_1()
        .child(
            div()
                .flex()
                .justify_between()
                .items_baseline()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::MUTED)
                        .child(label.to_string()),
                )
                .child(div().text_xs().text_color(theme::FAINT).child(sub)),
        )
        .child(
            div().flex().items_baseline().gap_1().child(
                div()
                    .text_size(px(22.))
                    .font_weight(gpui_kit::FontWeight::BOLD)
                    .text_color(theme::TEXT)
                    .child(value),
            ),
        )
        .child(bar_fill(frac, color))
        .into_any_element()
}

/// Tint helper shared by the keyboard preview: the resting colour takes the
/// live LED colour as `t` approaches 1.
pub fn glow_mix(resting: gpui_kit::Rgba, accent: gpui_kit::Rgba, t: f32) -> gpui_kit::Rgba {
    mix(resting, accent, t)
}

// ---------------------------------------------------------------------------
// KeyboardPreview — the live glowing keyboard, drawn as keycaps
// ---------------------------------------------------------------------------

/// One keycap: a label, its width in flex units (≈ key width), or a gap.
#[derive(Clone, Copy)]
pub struct KeyDef {
    pub label: &'static str,
    pub u: f32,
    pub gap: bool,
}

const fn key(label: &'static str, u: f32) -> KeyDef {
    KeyDef {
        label,
        u,
        gap: false,
    }
}

const fn gap(u: f32) -> KeyDef {
    KeyDef {
        label: "",
        u,
        gap: true,
    }
}

/// The G5 KC layout (the `Ñ` gives away the machine this was built on).
pub const KB_ROWS: &[&[KeyDef]] = &[
    &[
        key("esc", 1.1),
        key("F1", 1.),
        key("F2", 1.),
        key("F3", 1.),
        key("F4", 1.),
        gap(0.4),
        key("F5", 1.),
        key("F6", 1.),
        key("F7", 1.),
        key("F8", 1.),
        gap(0.4),
        key("F9", 1.),
        key("F10", 1.),
        key("F11", 1.),
        key("F12", 1.),
        key("prt", 1.1),
    ],
    &[
        key("`", 1.),
        key("1", 1.),
        key("2", 1.),
        key("3", 1.),
        key("4", 1.),
        key("5", 1.),
        key("6", 1.),
        key("7", 1.),
        key("8", 1.),
        key("9", 1.),
        key("0", 1.),
        key("-", 1.),
        key("=", 1.),
        key("⌫", 2.),
    ],
    &[
        key("tab", 1.5),
        key("Q", 1.),
        key("W", 1.),
        key("E", 1.),
        key("R", 1.),
        key("T", 1.),
        key("Y", 1.),
        key("U", 1.),
        key("I", 1.),
        key("O", 1.),
        key("P", 1.),
        key("[", 1.),
        key("]", 1.),
        key("\\", 1.5),
    ],
    &[
        key("caps", 1.8),
        key("A", 1.),
        key("S", 1.),
        key("D", 1.),
        key("F", 1.),
        key("G", 1.),
        key("H", 1.),
        key("J", 1.),
        key("K", 1.),
        key("L", 1.),
        key("Ñ", 1.),
        key(";", 1.),
        key("'", 1.),
        key("⏎", 2.2),
    ],
    &[
        key("shift", 2.3),
        key("Z", 1.),
        key("X", 1.),
        key("C", 1.),
        key("V", 1.),
        key("B", 1.),
        key("N", 1.),
        key("M", 1.),
        key(",", 1.),
        key(".", 1.),
        key("/", 1.),
        key("shift", 2.7),
    ],
    &[
        key("ctrl", 1.25),
        key("win", 1.),
        key("alt", 1.25),
        gap(0.5),
        key("", 5.5),
        gap(0.5),
        key("alt", 1.25),
        key("fn", 1.),
        key("ctrl", 1.25),
    ],
];

/// The keyboard as a keycap grid, tinted toward the live LED colour.
pub fn keyboard_preview(accent: gpui_kit::Rgba, glow: f32, on: bool) -> AnyElement {
    let glow = if on { glow.clamp(0., 1.) } else { 0. };
    let body = glow_mix(theme::PANEL2, accent, 0.45 * glow);
    let edge = glow_mix(theme::LINE, accent, 0.6 * glow);
    let legend = glow_mix(theme::MUTED, accent, 0.5 * glow);

    div()
        .rounded_md()
        .border_1()
        .border_color(theme::LINE)
        .bg(theme::FIELD)
        .p_3()
        .flex()
        .flex_col()
        .gap_1()
        .children(KB_ROWS.iter().map(|row| {
            let total: f32 = row.iter().map(|k| k.u).sum();
            div()
                .flex()
                .gap_1()
                .children(row.iter().map(|k| {
                    if k.gap {
                        return div().w(relative(k.u / total)).into_any_element();
                    }
                    div()
                        .w(relative(k.u / total))
                        .h(px(26.))
                        .rounded_xs()
                        .border_1()
                        .border_color(edge)
                        .bg(body)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_size(px(8.))
                                .text_color(legend)
                                .child(k.label.to_string()),
                        )
                        .into_any_element()
                }))
                .into_any_element()
        }))
        .into_any_element()
}
