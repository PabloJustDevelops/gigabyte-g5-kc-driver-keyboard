// Bespoke widgets the component library does not ship: the card section used
// by every view, the fan/temperature bars, and the glowing keyboard preview.
// These are the "custom" half of the UI — the ready-made gpui-component
// widgets (Button, Slider, Switch, Input, Toggle) cover the rest.

use gpui_kit::{
    AnyElement, App, BoxShadow, Div, IntoElement, ParentElement, Pixels, RenderOnce, SharedString,
    Styled, Window, canvas, div, point, px, relative,
};

use crate::motion;
use crate::theme::{self, mix};

// ---------------------------------------------------------------------------
// Type scale
// ---------------------------------------------------------------------------
//
// Five sizes, and every piece of text in the panel is one of them. The panel
// used to reach for `text_sm` and `text_xs` inline, which made "a label" and
// "a value under a label" the same size and left the eye with nothing to rank
// the screen by. The scale is 22 / 15 / 13 / 12 / 11, and only the 22 is
// monospace: numbers line up, prose does not need to.

// The number itself: the loudest thing on a card, and the only mono at 22.
pub fn value(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(22.))
        .line_height(px(24.))
        .font(theme::mono_font())
        .font_weight(gpui_kit::FontWeight::BOLD)
        .text_color(theme::TEXT)
        .child(text.into())
}

// A view title.
pub fn title(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(20.))
        .line_height(px(24.))
        .font_weight(gpui_kit::FontWeight::BOLD)
        .text_color(theme::TEXT)
        .child(text.into())
}

// A card's group label: small, bold, dimmed. Uppercase because it is a label,
// not a sentence — the web panel set these in uppercase too.
pub fn label(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(11.))
        .line_height(px(14.))
        .font_weight(gpui_kit::FontWeight::BOLD)
        .text_color(theme::FAINT)
        .child(text.into().to_uppercase())
}

// Prose: help text, warnings, anything the user has to read rather than scan.
pub fn body(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(13.))
        .line_height(px(18.))
        .text_color(theme::MUTED)
        .child(text.into())
}

// A secondary line: what a value means, where it came from.
pub fn meta(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(theme::FAINT)
        .child(text.into())
}

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
        // The card is a single flat panel, so what separates its parts is a
        // hairline under the header rather than a second box around the body.
        // Three nested borders is what made the panel read as a stack of
        // identical tiles instead of as one surface with structure in it.
        div()
            .rounded_md()
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::PANEL)
            // A one-pixel light edge along the top: the panel sits on a darker
            // window, and without it the card has no lit side.
            .shadow(vec![BoxShadow::new(
                px(0.),
                px(1.),
                theme::hsla(theme::WHITE, 0.04),
            )])
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .gap_3()
                    .pb_3()
                    .border_b_1()
                    .border_color(theme::LINE)
                    .child(label(self.title))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font(theme::mono_font())
                            .text_color(theme::MUTED)
                            .child(self.readout),
                    ),
            )
            .children(self.children)
    }
}

// ---------------------------------------------------------------------------
// Bar — the horizontal fill used by fan duty and temperature
// ---------------------------------------------------------------------------

/// `id` has to be stable and unique per bar: the spring keeps its position and
/// velocity in window state under it, so a fresh id is what makes the bar jump
/// instead of glide.
pub fn bar_fill(
    id: impl Into<gpui_kit::base::TransitionId>,
    frac: f32,
    color: gpui_kit::Rgba,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    // Fan duty and temperature are polled every three seconds, so the target
    // moves in steps. A spring carries velocity through those steps; a
    // fixed-duration ease would restart from a standstill each time and read as
    // a stutter on every reading.
    let width = motion::glide(id, frac.clamp(0., 1.), window, cx);
    div()
        .h(px(6.))
        .w_full()
        .rounded_full()
        .bg(theme::FIELD)
        .overflow_hidden()
        .child(div().h_full().w(relative(width)).rounded_full().bg(color))
        .into_any_element()
}

/// Temperature colour: ok under 60 °C, warn under 75 °C, danger beyond.
pub fn temp_color(temp_c: i32) -> gpui_kit::Rgba {
    if temp_c < 60 {
        theme::OK
    } else if temp_c < 75 {
        theme::WARN
    } else {
        theme::DANGER
    }
}

// ---------------------------------------------------------------------------
// Gauges
// ---------------------------------------------------------------------------
//
// Fan duty and temperature are not the same measurement, so they are not the
// same shape: duty is a proportion of a known whole and reads as an arc,
// temperature reads against a ceiling and reads as a scale with a marker. Both
// carry a sparkline of the last two and a half minutes, because the panel polls
// every three seconds and a bare number steps like a clock.

/// A gauge's caption. Not the uppercase `label`: this names a specific channel,
/// and shouting the name of a fan at the user is noise.
fn label_widget(text: &str) -> Div {
    div()
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(gpui_kit::FontWeight::BOLD)
        .text_color(theme::MUTED)
        .child(text.to_string())
}

/// Scale `series` into 0..1 for drawing.
///
/// `series` arrives already in its own units. The floor matters: a stopped fan
/// reads 0 rpm, and without a floor the sparkline would rescale itself to that
/// single zero on every poll and the trace would twitch for no reason.
fn normalise(series: &[f32], floor: f32) -> Vec<f32> {
    let peak = series
        .iter()
        .copied()
        .fold(floor, |a, b| if b > a { b } else { a });
    if peak <= 0. {
        return vec![0.; series.len()];
    }
    series.iter().map(|v| (v / peak).clamp(0., 1.)).collect()
}

/// An arc from `from` degrees clockwise, covering `frac` of `sweep` degrees.
///
/// Angles are degrees clockwise from twelve o'clock, so the ring starts at
/// seven-thirty and the gap lands at the bottom where nothing overlaps the
/// label. `None` when there is nothing to draw: a zero-length arc is a
/// degenerate path, and lyon rejects it.
fn arc(
    cx: Pixels,
    cy: Pixels,
    r: Pixels,
    stroke: Pixels,
    from_deg: f32,
    sweep_deg: f32,
    frac: f32,
) -> Option<gpui_kit::Path<Pixels>> {
    let frac = frac.clamp(0., 1.);
    let sweep = sweep_deg * frac;
    if sweep < 0.2 {
        return None;
    }
    let at = |deg: f32| {
        let rad = deg.to_radians();
        point(cx + r * rad.sin(), cy - r * rad.cos())
    };
    let mut path = gpui_kit::PathBuilder::stroke(stroke);
    path.move_to(at(from_deg));
    path.arc_to(
        point(r, r),
        px(0.),
        sweep > 180.,
        true,
        at(from_deg + sweep),
    );
    path.build().ok()
}

/// The duty ring: a track, the filled arc, and the percentage inside it.
///
/// `id` identifies the spring that drives the arc, so the ring carries velocity
/// through the three-second steps the same way the bar used to.
pub fn duty_ring(
    id: impl Into<gpui_kit::base::TransitionId>,
    frac: f32,
    readout: String,
    color: gpui_kit::Rgba,
) -> AnyElement {
    let size = px(78.);
    let stroke = px(7.);
    // Resolved here so the canvas closure owns it outright: a canvas paints
    // from a `'static` callback, which a borrowed id could not satisfy.
    let id = id.into();
    // The ring is an absolutely-positioned paint layer and the readout is an
    // ordinary centred child. Doing it the other way round — an in-flow canvas
    // under an inset overlay — put the readout at the bottom of the ring,
    // because the overlay's insets resolve against a box the canvas had
    // already sized.
    div()
        .relative()
        .size(size)
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            canvas(
                move |_bounds, window, cx| {
                    // Sampled here rather than in the caller: the ring is the only
                    // thing in the cell that animates, and sampling per frame keeps
                    // it in step with the element that is painting it.
                    motion::glide(id.clone(), frac.clamp(0., 1.), window, cx)
                },
                move |bounds, frac, window, _cx| {
                    let c = bounds.center();
                    let r = (bounds.size.width - stroke) / 2.;
                    if let Some(track) = arc(c.x, c.y, r, stroke, 225., 270., 1.) {
                        window.paint_path(track, theme::hsla(theme::LINE_STRONG, 0.6));
                    }
                    if let Some(filled) = arc(c.x, c.y, r, stroke, 225., 270., frac) {
                        window.paint_path(filled, color);
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        .child(
            div()
                .text_size(px(18.))
                .line_height(px(20.))
                .font(theme::mono_font())
                .font_weight(gpui_kit::FontWeight::BOLD)
                .text_color(theme::TEXT)
                .child(readout),
        )
        .into_any_element()
}

/// The trend behind a gauge.
///
/// One point is drawn as a flat line rather than nothing: on the first frame
/// after opening the panel there is exactly one sample, and an empty box where
/// a graph should be reads as a broken widget.
fn sparkline(series: Vec<f32>, color: gpui_kit::Rgba, height: Pixels) -> AnyElement {
    // The series arrives already scaled to 0..1 by the caller. Scaling it again
    // here — against its own peak — drew every line flat along the top, which
    // is exactly what a broken sparkline looks like.
    let points = series.clone();
    div()
        .relative()
        .w_full()
        .h(height)
        // A canvas has no intrinsic size: left in the flow it is laid out with
        // zero height and every trace collapses onto one line. The ring gets
        // its size the same way.
        .child(
            canvas(
                move |_bounds, _window, _cx| points,
                move |bounds, points, window, _cx| {
                    if points.len() < 2 {
                        return;
                    }
                    let step = bounds.size.width / (points.len() - 1) as f32;
                    let at = |i: usize| {
                        point(
                            bounds.origin.x + step * i as f32,
                            bounds.origin.y + bounds.size.height * (1. - points[i].clamp(0., 1.)),
                        )
                    };
                    // The area under the trace, at low opacity: it is what makes the
                    // line read as a magnitude rather than as a squiggle.
                    let mut area = gpui_kit::PathBuilder::fill();
                    area.move_to(at(0));
                    for i in 1..points.len() {
                        area.line_to(at(i));
                    }
                    area.line_to(point(
                        bounds.origin.x + bounds.size.width,
                        bounds.origin.y + bounds.size.height,
                    ));
                    area.line_to(point(bounds.origin.x, bounds.origin.y + bounds.size.height));
                    area.close();
                    if let Ok(path) = area.build() {
                        window.paint_path(path, theme::hsla(color, 0.1));
                    }
                    let mut line = gpui_kit::PathBuilder::stroke(px(1.5));
                    line.move_to(at(0));
                    for i in 1..points.len() {
                        line.line_to(at(i));
                    }
                    if let Ok(path) = line.build() {
                        window.paint_path(path, color);
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        .into_any_element()
}

/// One fan: ring for the duty, sparkline of the rpm behind it.
#[allow(clippy::too_many_arguments)]
pub fn duty_gauge(
    channel: &'static str,
    index: usize,
    label: &str,
    duty_pct: Option<u8>,
    rpm: Option<u32>,
    rpm_history: &[f32],
    color: gpui_kit::Rgba,
    min_w: Pixels,
) -> AnyElement {
    // 1000 rpm as the floor: a stopped fan is genuinely 0, but scaling to it
    // turns every other reading into a full-height spike.
    let trend = normalise(rpm_history, 1000.);
    div()
        .flex()
        .flex_col()
        .gap_1()
        .min_w(min_w)
        .flex_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(duty_ring(
                    (index, channel),
                    duty_pct.unwrap_or(0) as f32 / 100.,
                    format!("{}%", duty_pct.unwrap_or(0)),
                    color,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(label_widget(label))
                        .child(meta(
                            rpm.map(|r| format!("{r} rpm"))
                                .unwrap_or_else(|| "stopped".into()),
                        ))
                        .child(sparkline(trend, color, px(20.))),
                ),
        )
        .into_any_element()
}

/// One temperature: the value against the ceiling, the trend behind it, and a
/// bar with a tick at the warning colour so the scale is not just a bar that
/// happens to change hue.
#[allow(clippy::too_many_arguments)]
pub fn temp_gauge(
    channel: &'static str,
    index: usize,
    label: &str,
    temp_c: i32,
    ceiling_c: i32,
    history: &[f32],
    color: gpui_kit::Rgba,
    min_w: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let ceiling = ceiling_c.max(1) as f32;
    let trend = history
        .iter()
        .map(|t| (t / ceiling).clamp(0., 1.))
        .collect::<Vec<f32>>();
    let frac = (temp_c as f32 / ceiling).clamp(0., 1.);
    div()
        .flex()
        .flex_col()
        .gap_1()
        .min_w(min_w)
        .flex_1()
        .child(
            div()
                .flex()
                .justify_between()
                .items_baseline()
                .gap_2()
                .child(label_widget(label))
                .child(meta(format!("ceiling {} °C", ceiling_c))),
        )
        .child(
            div()
                .flex()
                .items_baseline()
                .gap_1()
                .child(value(format!("{temp_c}")))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(16.))
                        .text_color(theme::MUTED)
                        .child("°C"),
                ),
        )
        .child(sparkline(trend, color, px(18.)))
        // The tick is what turns "a bar" into a scale: 75 °C is where the
        // colour changes, and a reader should not have to remember that.
        .child(
            div()
                .relative()
                .w_full()
                .h(px(6.))
                .child(bar_fill((index, channel), frac, color, window, cx))
                .child(
                    div()
                        .absolute()
                        // Relative, not 75 px: the bar is a fraction of
                        // whatever the lane gave it, so a fixed offset put the
                        // tick in the wrong place on every width but one.
                        .left(relative(0.75))
                        .top(px(-2.))
                        .h(px(10.))
                        .w(px(1.))
                        .bg(theme::hsla(theme::WARN, 0.5)),
                ),
        )
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

    // Ratios come from the web panel's styles.css: the face takes 26% of the
    // LED colour, the border 45%, and the legend brightens towards white. The
    // port used 45% on the face, which turned a white keyboard into a flat
    // grey slab — the single most visible regression in the rewrite.
    let body = glow_mix(theme::PANEL2, accent, 0.26 * glow);
    let edge = glow_mix(theme::LINE, accent, 0.45 * glow);
    let legend = glow_mix(theme::MUTED, theme::WHITE, 0.55 * glow);

    // `inset 0 1px 0 rgba(255,255,255,0.05)`: the highlight along the top of a
    // keycap that makes it read as a raised key instead of a flat swatch.
    let highlight = BoxShadow::new(px(0.), px(1.), theme::hsla(theme::WHITE, 0.05)).inset();

    div()
        .rounded_md()
        .border_1()
        .border_color(theme::LINE)
        // `inset 0 2px 14px rgba(0,0,0,0.55)`: the recessed well the keys sit in.
        .shadow(vec![
            BoxShadow::new(px(0.), px(2.), theme::hsla(theme::BLACK, 0.55))
                .blur_radius(px(14.))
                .inset(),
        ])
        .bg(theme::FIELD)
        .p_3()
        .flex()
        .flex_col()
        .gap_1()
        .children(KB_ROWS.iter().map(|row| {
            div()
                .flex()
                .gap_1()
                .children(row.iter().map(|k| {
                    // flex_grow, not a percentage width: percentages are computed
                    // against the row before the gaps are added, so a 16-key row
                    // overflowed its card by the width of all its gaps.
                    let span = || div().flex_grow(k.u).flex_basis(px(0.));
                    if k.gap {
                        return span().into_any_element();
                    }
                    // The web panel glowed with `0 0 calc(3px + p*12px)
                    // calc(p*1.5px)`, but that was drawn under a much darker
                    // keycap. At this face ratio the same radius reads as a fog
                    // over the whole board, so it is halved.
                    let glow_shadow = (glow > 0.01).then(|| {
                        BoxShadow::new(px(0.), px(0.), theme::hsla(accent, 0.42 * glow))
                            .blur_radius(px(2. + 6. * glow))
                            .spread_radius(px(1.5 * glow))
                    });

                    let mut key = span()
                        .h(px(26.))
                        .rounded_xs()
                        .border_1()
                        .border_color(edge)
                        .bg(body)
                        .shadow(vec![highlight.clone()]);
                    if let Some(glow_shadow) = glow_shadow {
                        key = key.shadow(vec![highlight.clone(), glow_shadow]);
                    }

                    key.flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_size(px(9.))
                                .text_color(legend)
                                .child(k.label.to_string()),
                        )
                        .into_any_element()
                }))
                .into_any_element()
        }))
        .into_any_element()
}
