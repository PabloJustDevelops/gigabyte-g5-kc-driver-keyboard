// The root view: navigation, shared state, and the async plumbing that keeps
// everything fresh without ever blocking the UI thread.

use gpui_kit::{App, AppContext as _, Context, Entity, FocusHandle, Subscription, Task, Window};

use crate::backend::{
    self,
    fan::{FanMode, RawFanStatus},
    kbd::{self, KbState},
};
use crate::theme;

// The curve type is the daemon's own (`[temperature °C, duty %]`), not a panel
// invention: the editor hands the same values to `g5fan curve set` that the
// validator in `g5core` checked.
use g5core::Point;

use gpui_kit::component::input::InputState;
use gpui_kit::component::slider::{SliderEvent, SliderState};

// ---------------------------------------------------------------------------
// History — the readings behind the gauges
// ---------------------------------------------------------------------------

/// One poll's worth of readings.
#[derive(Clone, Debug, Default)]
pub struct Sample {
    /// duty %, one per fan, in the order `g5fan` reports them
    pub duty: Vec<f32>,
    pub rpm: Vec<f32>,
    pub cpu_c: Option<f32>,
    pub gpu_c: Option<f32>,
}

/// A short ring of recent samples.
///
/// The panel polls the fan daemon every three seconds, so a gauge that shows
/// only the current number steps like a clock. With a ring behind it the same
/// number reads as a trend, and the sparkline costs nothing but this buffer.
#[derive(Debug)]
pub struct History {
    samples: std::collections::VecDeque<Sample>,
    cap: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    /// 48 samples is two and a half minutes at the poll interval: long enough
    /// to see a curve, short enough that the sparkline still reads as "now".
    pub const CAP: usize = 48;

    pub fn new() -> Self {
        Self {
            samples: std::collections::VecDeque::with_capacity(Self::CAP),
            cap: Self::CAP,
        }
    }

    pub fn record(&mut self, fan: &RawFanStatus) {
        let sample = Sample {
            duty: fan
                .fans
                .iter()
                .map(|f| f.duty_pct.unwrap_or(0) as f32)
                .collect(),
            rpm: fan.fans.iter().map(|f| f.rpm.unwrap_or(0) as f32).collect(),
            cpu_c: fan.cpu_temp_c.map(|t| t as f32),
            gpu_c: fan.gpu_temp_c.map(|t| t as f32),
        };
        if self.samples.len() == self.cap {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// One series out of the ring, oldest first. `f` picks which reading of
    /// each sample; samples where it is missing are dropped, so a temperature
    /// that the daemon did not report does not become a zero on the graph.
    pub fn series<T>(&self, f: impl Fn(&Sample) -> Option<T>) -> Vec<T> {
        self.samples.iter().filter_map(f).collect()
    }

    /// Fan `index`'s rpm over time.
    pub fn rpm(&self, index: usize) -> Vec<f32> {
        self.series(|s| s.rpm.get(index).copied())
    }

    /// Temperature `index` (0 = CPU, 1 = GPU) over time.
    pub fn temp(&self, index: usize) -> Vec<f32> {
        self.series(|s| if index == 0 { s.cpu_c } else { s.gpu_c })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A panel is expensive to build in a unit test (it reads the hardware), so
    /// the clamping is tested as the free function it is.
    use super::clamp_curve_point as clamp;

    #[test]
    fn a_point_cannot_be_dragged_past_its_neighbours() {
        let curve = [[50, 25], [65, 40], [75, 60], [85, 80], [95, 100]];
        // Dragging the middle point far to the left stops one degree above its
        // predecessor; the same drag upward stops at the duty of the point to
        // its right, because duty may not fall as the curve heats — the two
        // bounds belong to the same drag, not to two different ones.
        assert_eq!(clamp(&curve, 2, 10., 90.), [66, 80]);
        // And the other way: one degree below its successor's temperature, no
        // lower than its predecessor's duty.
        assert_eq!(clamp(&curve, 2, 200., 10.), [84, 40]);
    }

    #[test]
    fn duty_never_falls_as_the_curve_heats() {
        let curve = [[50, 25], [65, 40], [75, 60], [85, 80], [95, 100]];
        // Pulling the last point's duty down stops at its predecessor's.
        assert_eq!(clamp(&curve, 4, 95., 0.), [95, 80]);
        // The first point has no predecessor, so only the absolute bounds apply.
        assert_eq!(clamp(&curve, 0, -50., -50.), [20, 0]);
        assert_eq!(clamp(&curve, 4, 500., 500.), [110, 100]);
    }

    #[test]
    fn every_clamped_point_survives_the_validators() {
        let mut curve = [[50, 25], [65, 40], [75, 60], [85, 80], [95, 100]];
        // Drag each point hard in each direction: whatever the pointer does, the
        // curve the CLI would be handed has to stay legal.
        for i in 0..curve.len() {
            for (temp, duty) in [(0., 0.), (200., 100.), (10., 90.), (200., 0.)] {
                curve[i] = clamp(&curve, i, temp, duty);
                assert_eq!(
                    crate::backend::fan::curve_problem(&curve),
                    None,
                    "point {i} dragged to {temp}/{duty} broke the curve"
                );
            }
        }
    }

    fn snapshot(duty: u8, rpm: u32, cpu: i32) -> RawFanStatus {
        serde_json::from_str(&format!(
            r#"{{"mode":"auto","backend":"ec","driver":true,"manual_duty":null,
                 "curve":null,"custom_curve":[],"presets":{{}},
                 "fans":[{{"label":"CPU","duty_pct":{duty},"rpm":{rpm}}},
                         {{"label":"GPU","duty_pct":{duty},"rpm":{rpm}}}],
                 "cpu_temp_c":{cpu},"gpu_temp_c":40,"ceiling_c":100,
                 "daemon":true,"age_s":1,"stale":false}}"#
        ))
        .expect("snapshot parses")
    }

    #[test]
    fn the_ring_keeps_only_the_last_window() {
        let mut history = History::new();
        for i in 0..History::CAP + 10 {
            history.record(&snapshot(50, 1000 + i as u32, 50));
        }
        assert_eq!(history.len(), History::CAP);
        // Oldest first: the ring dropped the first ten readings.
        assert_eq!(history.rpm(0).first().copied(), Some((1000 + 10) as f32));
        assert_eq!(
            history.rpm(0).last().copied(),
            Some((1000 + History::CAP + 9) as f32)
        );
    }

    #[test]
    fn every_fan_gets_its_own_series() {
        let mut history = History::new();
        history.record(&snapshot(30, 1200, 45));
        history.record(&snapshot(80, 4800, 61));
        assert_eq!(history.rpm(0), vec![1200., 4800.]);
        assert_eq!(history.temp(0), vec![45., 61.]);
        assert_eq!(history.temp(1), vec![40., 40.]);
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Home,
    Lighting,
    Profiles,
    Fans,
}

impl View {
    pub fn title(self) -> &'static str {
        match self {
            View::Home => "Overview",
            View::Lighting => "Lighting",
            View::Profiles => "Profiles",
            View::Fans => "Fans",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            View::Home => "keyboard state at a glance",
            View::Lighting => "colour · brightness · effects",
            View::Profiles => "saved colour sets",
            View::Fans => "fan modes, duty and curve",
        }
    }
}

// ---------------------------------------------------------------------------
// Slider tags — which slider fired a Change/Release event
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SliderTag {
    Brightness,
    Red,
    Green,
    Blue,
    Speed,
    Duty,
}

// ---------------------------------------------------------------------------
// Panel state
// ---------------------------------------------------------------------------

pub struct Panel {
    pub view: View,

    // keyboard state (polled every second)
    pub kbd: KbState,

    // lighting widgets
    pub brightness: Entity<gpui_kit::component::slider::SliderState>,
    pub ch_red: Entity<gpui_kit::component::slider::SliderState>,
    pub ch_green: Entity<gpui_kit::component::slider::SliderState>,
    pub ch_blue: Entity<gpui_kit::component::slider::SliderState>,
    pub speed: Entity<gpui_kit::component::slider::SliderState>,
    /// the effect the panel started ("breathe" / "cycle"), if running
    pub effect_mode: Option<&'static str>,

    // hex entry
    pub hex_input: Entity<gpui_kit::component::input::InputState>,

    // profiles
    pub profile_name: Entity<gpui_kit::component::input::InputState>,
    pub profiles: Vec<String>,

    // fans
    pub fan: Option<RawFanStatus>,
    pub fan_error: Option<String>,
    pub watchdog: Option<bool>,
    /// Readings behind the gauges, fed by the same poll that sets `fan`.
    pub history: History,
    pub duty: Entity<gpui_kit::component::slider::SliderState>,
    /// curve editor points, 5 × (temp °C, duty %)
    pub curve: [Point; 5],
    pub curve_temp: Vec<Entity<gpui_kit::component::slider::SliderState>>,
    pub curve_duty: Vec<Entity<gpui_kit::component::slider::SliderState>>,
    pub curve_dirty: bool,
    /// Which curve point the pointer is dragging on the graph, if any.
    pub curve_drag: Option<usize>,

    // infra
    /// One focus handle per sidebar row. The rows are plain divs, so without
    /// these they cannot be reached with the keyboard at all.
    pub nav_focus: [FocusHandle; 4],
    /// One per fan-mode chip, same reason as `nav_focus`.
    pub mode_focus: Vec<FocusHandle>,
    /// Bumped on every view change. The fade is keyed on it, so returning to a
    /// view already seen replays the animation instead of resuming a stale one.
    pub view_seq: u64,
    /// Which sidebar row the pointer is over, if any. Hover styling cannot be
    /// transitioned by the element itself — `.hover()` applies its styles the
    /// instant the hitbox flips — so the row is told where the pointer is and
    /// eases toward that value itself.
    pub hover_nav: Option<usize>,
    /// Same, for the fan-mode chips.
    pub hover_mode: Option<usize>,
    pub tasks: Vec<Task<()>>,
    pub subs: Vec<Subscription>,
    /// transient error from the last action, cleared on the next success
    pub err: Option<String>,
    pub busy: bool,
}

/// Clamp a proposed curve point so the result still passes `curve_problem`.
///
/// Temperatures must strictly rise and duty must never fall as it heats, or
/// `g5fan curve set` refuses the result and the editor is left showing
/// something the daemon will not accept. Clamping to the neighbours is what
/// stops a drag from walking a point through the one beside it.
pub fn clamp_curve_point(curve: &[Point], i: usize, temp: f32, duty: f32) -> Point {
    let mut t = temp.clamp(20., 110.);
    let mut d = duty.clamp(0., 100.);
    if i > 0 {
        t = t.max(curve[i - 1][0] as f32 + 1.);
        d = d.max(curve[i - 1][1] as f32);
    }
    if i + 1 < curve.len() {
        t = t.min(curve[i + 1][0] as f32 - 1.);
        d = d.min(curve[i + 1][1] as f32);
    }
    [t.round() as i32, d.round() as i32]
}

impl Panel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut panel = Self::build(window, cx, View::Home);
        panel.wire_sliders(cx);
        panel.start_loops(cx);
        panel.refresh_profiles(cx);
        panel
    }

    /// Same panel, pinned to `view` and without the polling loops or the
    /// profile fetch. `tests/snapshot.rs` uses this to render one view to a
    /// PNG: a background loop would keep the headless test dispatcher busy and
    /// the profile fetch would race the first frame.
    pub fn new_for_snapshot(window: &mut Window, cx: &mut Context<Self>, view: View) -> Self {
        Self::build(window, cx, view)
    }

    fn build(window: &mut Window, cx: &mut Context<Self>, view: View) -> Self {
        // Both reads are plain file/process access, done once synchronously so
        // the sliders can be seeded with the real values. The startup cost is
        // one `g5fan status --cached` (~30 ms); the keyboard read is file I/O.
        let kbd = kbd::current_state();
        let fan = fan_read_cached();

        let slider = |min: f32, max: f32, value: f32, cx: &mut Context<Self>| {
            cx.new(|_| {
                SliderState::new()
                    .min(min)
                    .max(max)
                    .step(1.)
                    .default_value(value)
            })
        };

        let brightness = slider(0., 100., kbd.brightness as f32, cx);
        let ch_red = slider(0., 255., kbd.red as f32, cx);
        let ch_green = slider(0., 255., kbd.green as f32, cx);
        let ch_blue = slider(0., 255., kbd.blue as f32, cx);
        let speed = slider(1., 10., 5., cx);

        let hex_input = cx.new(|cx| InputState::new(window, cx).placeholder("rrggbb"));
        let profile_name = cx.new(|cx| InputState::new(window, cx).placeholder("name"));

        // Curve editor: seed from the saved custom curve (or the shipped one).
        let custom = fan
            .as_ref()
            .ok()
            .map(|f| f.custom_curve.clone())
            .unwrap_or_else(|| vec![[50, 25], [65, 40], [75, 60], [85, 80], [95, 100]]);
        let mut curve: [Point; 5] = [[50, 25], [65, 40], [75, 60], [85, 80], [95, 100]];
        for (i, point) in custom.iter().take(5).enumerate() {
            curve[i] = [point[0].min(110), point[1].min(100)];
        }
        let curve_temp = (0..5)
            .map(|i| slider(20., 110., curve[i][0] as f32, cx))
            .collect();
        let curve_duty = (0..5)
            .map(|i| slider(0., 100., curve[i][1] as f32, cx))
            .collect();
        let duty = slider(
            0.,
            100.,
            fan.as_ref().ok().and_then(|f| f.manual_duty).unwrap_or(50) as f32,
            cx,
        );

        // Seed the trend from the snapshot the sliders were seeded from, so the
        // first frame already has something to draw rather than a flat line
        // that fills in three seconds later.
        let mut history = History::new();
        if let Ok(snapshot) = fan.as_ref() {
            history.record(snapshot);
        }

        Panel {
            view,
            kbd,
            brightness,
            ch_red,
            ch_green,
            ch_blue,
            speed,
            effect_mode: None,
            hex_input,
            profile_name,
            profiles: Vec::new(),
            fan: fan.ok(),
            fan_error: None,
            watchdog: None,
            history,
            duty,
            curve,
            curve_temp,
            curve_duty,
            curve_dirty: false,
            curve_drag: None,
            // `cx.focus_handle()` starts with `tab_stop: false`, which keeps the
            // handle out of the tab order even though `track_focus` puts it in the
            // tab-stop map. Without this, Tab walks straight past the sidebar.
            nav_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            mode_focus: FanMode::UI_MODES
                .iter()
                .map(|_| cx.focus_handle().tab_stop(true))
                .collect(),
            view_seq: 0,
            hover_nav: None,
            hover_mode: None,
            tasks: Vec::new(),
            subs: Vec::new(),
            err: None,
            busy: false,
        }
    }

    /// Switch to `view` and arm the fade.
    ///
    /// Every caller goes through here rather than assigning `self.view`: the
    /// mouse handler, the keyboard handler and the snapshot tests each look the
    /// same from the outside, and a switch that forgot to bump `view_seq` would
    /// silently not animate.
    pub fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        if self.view == view {
            return;
        }
        self.view = view;
        self.view_seq += 1;
        cx.notify();
    }

    /// Record which sidebar row the pointer is over, for the hover transition.
    pub fn set_hover_nav(&mut self, index: usize, on: bool, cx: &mut Context<Self>) {
        let next = on.then_some(index);
        if self.hover_nav != next {
            self.hover_nav = next;
            cx.notify();
        }
    }

    /// Same, for the fan-mode chips.
    pub fn set_hover_mode(&mut self, index: usize, on: bool, cx: &mut Context<Self>) {
        let next = on.then_some(index);
        if self.hover_mode != next {
            self.hover_mode = next;
            cx.notify();
        }
    }

    // -- slider wiring --------------------------------------------------------

    /// Connect every SliderState to its action. `Change` fires continuously
    /// while dragging and writes the sysfs node directly (the udev rule makes
    /// it world-writable); `Release` spawns the CLI once to persist.
    fn wire_sliders(&mut self, cx: &mut Context<Self>) {
        let tag = |s: &Entity<SliderState>, t: SliderTag| (s.clone(), t);
        let pairs = [
            tag(&self.brightness, SliderTag::Brightness),
            tag(&self.ch_red, SliderTag::Red),
            tag(&self.ch_green, SliderTag::Green),
            tag(&self.ch_blue, SliderTag::Blue),
            tag(&self.speed, SliderTag::Speed),
            tag(&self.duty, SliderTag::Duty),
        ];
        for (state, tag) in pairs {
            // The closure needs its own handle: `state` is still borrowed by
            // the subscribe() call itself.
            let emitter = state.clone();
            let sub = cx.subscribe(&state, move |panel, _emitter, event, cx| {
                panel.on_slider_event(&emitter, tag, event, cx);
            });
            self.subs.push(sub);
        }
        for i in 0..5 {
            let t = self.curve_temp[i].clone();
            let d = self.curve_duty[i].clone();
            let sub = cx.subscribe(&t, move |panel, _state, event, cx| {
                if let SliderEvent::Release(_) = event {
                    panel.sync_curve_point(i, cx);
                }
            });
            self.subs.push(sub);
            let sub = cx.subscribe(&d, move |panel, _state, event, cx| {
                if let SliderEvent::Release(_) = event {
                    panel.sync_curve_point(i, cx);
                }
            });
            self.subs.push(sub);
        }
    }

    fn on_slider_event(
        &mut self,
        state: &Entity<SliderState>,
        tag: SliderTag,
        event: &SliderEvent,
        cx: &mut Context<Self>,
    ) {
        let value = |state: &Entity<SliderState>, cx: &App| state.read(cx).value().start();
        match tag {
            SliderTag::Brightness => {
                let level = value(state, cx) as u8;
                match event {
                    SliderEvent::Change(_) => {
                        // live write, no CLI spawn mid-drag
                        let _ = backend::brightness_raw(level);
                        self.kbd.brightness = level;
                        cx.notify();
                    }
                    SliderEvent::Release(_) => {
                        self.kbd_action(vec!["brightness".into(), level.to_string()], cx);
                    }
                }
            }
            SliderTag::Red | SliderTag::Green | SliderTag::Blue => {
                let r = value(&self.ch_red, cx) as u8;
                let g = value(&self.ch_green, cx) as u8;
                let b = value(&self.ch_blue, cx) as u8;
                match event {
                    SliderEvent::Change(_) => {
                        let _ = backend::color_raw(r, g, b);
                        self.kbd.red = r;
                        self.kbd.green = g;
                        self.kbd.blue = b;
                        self.kbd.enabled = true;
                        cx.notify();
                    }
                    SliderEvent::Release(_) => {
                        let hex = format!("{r:02x}{g:02x}{b:02x}");
                        self.kbd_action(vec!["color".into(), hex], cx);
                    }
                }
            }
            SliderTag::Speed => {
                if let SliderEvent::Release(_) = event {
                    self.apply_effect_speed(state, cx);
                }
            }
            SliderTag::Duty => {
                if let SliderEvent::Release(_) = event {
                    let pct = value(state, cx) as u8;
                    self.fan_action(vec!["manual".into(), pct.to_string()], cx);
                }
            }
        }
    }

    fn apply_effect_speed(&mut self, state: &Entity<SliderState>, cx: &mut Context<Self>) {
        let Some(mode) = self.effect_mode else {
            return;
        };
        let speed = state.read(cx).value().start() as u8;
        self.kbd_action(
            vec![
                "effect".into(),
                mode.into(),
                "--speed".into(),
                speed.to_string(),
                "--bg".into(),
            ],
            cx,
        );
    }

    /// One curve point changed on release: copy the sliders into `curve` and
    /// refresh the editor readout. Applying still goes through the button.
    fn sync_curve_point(&mut self, i: usize, cx: &mut Context<Self>) {
        let t = self.curve_temp[i].read(cx).value().start();
        let d = self.curve_duty[i].read(cx).value().start();
        self.curve[i] = [t as i32, d as i32];
        self.curve_dirty = true;
        cx.notify();
    }

    /// The live accent: the LED colour when the keyboard is on, otherwise the
    /// resting accent (everything greys out with the keyboard).
    pub fn accent(&self) -> gpui_kit::Rgba {
        if self.kbd.enabled {
            gpui_kit::rgb(
                ((self.kbd.red as u32) << 16)
                    | ((self.kbd.green as u32) << 8)
                    | self.kbd.blue as u32,
            )
        } else {
            theme::ACCENT
        }
    }

    /// Glow strength 0..1 from the brightness — drives the keycap tint.
    pub fn glow(&self) -> f32 {
        if self.kbd.enabled {
            0.15 + 0.85 * (self.kbd.brightness as f32 / 100.)
        } else {
            0.
        }
    }

    // -- async plumbing -----------------------------------------------------

    /// Run blocking work on the background executor, then update this panel.
    pub fn dispatch<R, F, C>(&mut self, cx: &mut Context<Self>, work: F, after: C)
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
        C: FnOnce(&mut Self, R, &mut Context<Self>) + 'static,
    {
        let task = cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { work() }).await;
            let _ = this.update(cx, |panel, cx| after(panel, result, cx));
        });
        self.tasks.push(task);
    }

    /// A CLI action: run `g5kbd`, surface the error banner, refresh state.
    pub fn kbd_action(&mut self, args: Vec<String>, cx: &mut Context<Self>) {
        self.busy = true;
        self.err = None;
        cx.notify();
        self.dispatch(
            cx,
            move || backend::run_cli(&args.iter().map(String::as_str).collect::<Vec<_>>()),
            |panel, result, cx| {
                panel.busy = false;
                if let Err(e) = result {
                    panel.err = Some(e);
                }
                panel.kbd = kbd::current_state();
                cx.notify();
            },
        );
    }

    /// A privileged fan action, through pkexec + polkit.
    pub fn fan_action(&mut self, args: Vec<String>, cx: &mut Context<Self>) {
        self.busy = true;
        self.err = None;
        cx.notify();
        self.dispatch(
            cx,
            move || backend::run_fan_cli(&args.iter().map(String::as_str).collect::<Vec<_>>()),
            |panel, result, cx| {
                panel.busy = false;
                match result {
                    Err(e) => panel.err = Some(e),
                    Ok(_) => panel.err = None,
                }
                panel.poll_fans_now(cx);
            },
        );
    }

    /// Move curve point `i` to (`temp`, `duty`), keeping the curve valid.
    ///
    /// The graph enforces [`clamp_curve_point`] while the pointer is down, so
    /// the editor can never hold a curve the daemon would refuse.
    pub fn move_curve_point(
        &mut self,
        i: usize,
        temp: f32,
        duty: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let [t, d] = clamp_curve_point(&self.curve, i, temp, duty);
        self.curve[i] = [t, d];
        // The sliders are the keyboard path onto the same five values; leaving
        // them behind would show one curve while another was being edited.
        self.curve_temp[i].update(cx, |s, cx| s.set_value(t as f32, window, cx));
        self.curve_duty[i].update(cx, |s, cx| s.set_value(d as f32, window, cx));
        self.curve_dirty = true;
        cx.notify();
    }

    /// Pick up curve point `i` for dragging.
    pub fn begin_curve_drag(&mut self, i: usize, cx: &mut Context<Self>) {
        self.curve_drag = Some(i);
        cx.notify();
    }

    pub fn end_curve_drag(&mut self, cx: &mut Context<Self>) {
        if self.curve_drag.take().is_some() {
            cx.notify();
        }
    }

    /// Fill the trend with a synthetic ramp built from the current snapshot.
    ///
    /// The real history needs a minute of uptime to have anything to draw, so
    /// without this the sparklines would never appear in a snapshot and the one
    /// piece of the gauges that cannot be checked from the code would be the
    /// one nobody ever looked at. `tests/snapshot.rs` calls it; nothing in the
    /// app does.
    pub fn seed_history(&mut self, steps: usize) {
        let Some(snapshot) = self.fan.clone() else {
            return;
        };
        self.history = History::new();
        let mut previous = snapshot.clone();
        for step in 0..steps {
            // A plausible shape rather than noise: idle, then a load, then back
            // down. What is being checked is that the line is drawn at all.
            let phase = step as f32 / steps.max(1) as f32;
            let wave = (phase * std::f32::consts::PI * 2.).sin();
            let mut next = previous.clone();
            for (i, fan) in next.fans.iter_mut().enumerate() {
                let swing = 10. + 22. * wave + i as f32 * 6.;
                fan.duty_pct = Some((50. + swing).clamp(0., 100.) as u8);
                fan.rpm = Some((2400. + 900. * wave + i as f32 * 400.).max(0.) as u32);
            }
            next.cpu_temp_c = Some((52. + 14. * wave).round() as i32);
            next.gpu_temp_c = Some((48. + 9. * wave).round() as i32);
            self.history.record(&next);
            previous = next;
        }
    }

    /// Record a fresh fan snapshot.
    ///
    /// Everything that lands a snapshot goes through here rather than assigning
    /// `fan` directly: the gauges' trend is fed from the same value, and a poll
    /// that skipped it would leave a flat line nobody could explain.
    pub fn set_fan(&mut self, fan: RawFanStatus) {
        self.history.record(&fan);
        self.fan = Some(fan);
    }

    pub fn poll_fans_now(&mut self, cx: &mut Context<Self>) {
        self.dispatch(cx, fan_read_cached, |panel, result, _cx| match result {
            Ok(fan) => {
                panel.set_fan(fan);
                panel.fan_error = None;
            }
            Err(e) => panel.fan_error = Some(e),
        });
    }
    pub fn refresh_profiles(&mut self, cx: &mut Context<Self>) {
        self.dispatch(
            cx,
            || backend::run_cli(&["profile", "list"]),
            |panel, result, cx| {
                panel.profiles = result
                    .map(|out| out.lines().map(str::to_string).collect())
                    .unwrap_or_default();
                cx.notify();
            },
        );
    }

    // -- poll loops ----------------------------------------------------------

    fn start_loops(&mut self, cx: &mut Context<Self>) {
        // keyboard state, once a second (file reads)
        let kbd_loop = cx.spawn(async move |this, cx| {
            loop {
                let state = cx.background_spawn(async { kbd::current_state() }).await;
                let _ = this.update(cx, |panel, cx| {
                    panel.kbd = state;
                    cx.notify();
                });
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
            }
        });

        // fan snapshot + watchdog, every three seconds
        let fan_loop = cx.spawn(async move |this, cx| {
            loop {
                let fan = cx.background_spawn(async { fan_read_cached() }).await;
                let wd = cx
                    .background_spawn(async { backend::fan_watchdog_active() })
                    .await;
                let _ = this.update(cx, |panel, cx| {
                    match fan {
                        Ok(fan) => {
                            panel.set_fan(fan);
                            panel.fan_error = None;
                        }
                        Err(e) => panel.fan_error = Some(e),
                    }
                    panel.watchdog = Some(wd);
                    cx.notify();
                });
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(3))
                    .await;
            }
        });

        self.tasks.push(kbd_loop);
        self.tasks.push(fan_loop);
    }
}

/// One `g5fan status --cached --json`, parsed. The snapshot is world-readable
/// and the command never elevates, so this is safe to call from anywhere.
fn fan_read_cached() -> Result<RawFanStatus, String> {
    let out = backend::run_fan_cli_unprivileged(&["status", "--cached", "--json"])?;
    serde_json::from_str(&out).map_err(|e| format!("could not parse `g5fan status --json`: {e}"))
}

impl Panel {
    /// Write the edited curve and select custom, mirroring the web panel's
    /// "Save and use" button. Validation mirrors `g5fan curve set`.
    pub fn apply_curve(&mut self, cx: &mut Context<Self>) {
        let points: Vec<Point> = self.curve.to_vec();
        if let Some(problem) = backend::fan::curve_problem(&points) {
            self.err = Some(problem);
            cx.notify();
            return;
        }
        let mut args = vec!["curve".to_string(), "set".to_string()];
        args.extend(points.iter().map(|p| format!("{}:{}", p[0], p[1])));
        self.fan_action(args, cx);
        self.curve_dirty = false;
    }

    pub fn reset_curve(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shipped = self
            .fan
            .as_ref()
            .and_then(|f| f.presets.get("custom"))
            .cloned()
            .unwrap_or_else(|| vec![[50, 25], [65, 40], [75, 60], [85, 80], [95, 100]]);
        for (i, p) in shipped.iter().take(5).enumerate() {
            self.curve[i] = [p[0], p[1]];
            self.curve_temp[i].update(cx, |s, cx| s.set_value(p[0] as f32, window, cx));
            self.curve_duty[i].update(cx, |s, cx| s.set_value(p[1] as f32, window, cx));
        }
        self.curve_dirty = true;
        cx.notify();
    }
}
