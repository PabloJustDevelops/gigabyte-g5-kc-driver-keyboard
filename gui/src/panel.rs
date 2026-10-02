// The root view: navigation, shared state, and the async plumbing that keeps
// everything fresh without ever blocking the UI thread.

use gpui_kit::{App, AppContext as _, Context, Entity, Subscription, Task, Window};

use crate::backend::{
    self,
    fan::RawFanStatus,
    kbd::{self, KbState},
};
use crate::theme;

use gpui_kit::component::input::InputState;
use gpui_kit::component::slider::{SliderEvent, SliderState};

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
    pub duty: Entity<gpui_kit::component::slider::SliderState>,
    /// curve editor points, 5 × (temp °C, duty %)
    pub curve: [[u32; 2]; 5],
    pub curve_temp: Vec<Entity<gpui_kit::component::slider::SliderState>>,
    pub curve_duty: Vec<Entity<gpui_kit::component::slider::SliderState>>,
    pub curve_dirty: bool,

    // infra
    pub tasks: Vec<Task<()>>,
    pub subs: Vec<Subscription>,
    /// transient error from the last action, cleared on the next success
    pub err: Option<String>,
    pub busy: bool,
}

impl Panel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
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
        let mut curve = [[50u32, 25], [65, 40], [75, 60], [85, 80], [95, 100]];
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

        let mut panel = Panel {
            view: View::Home,
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
            duty,
            curve,
            curve_temp,
            curve_duty,
            curve_dirty: false,
            tasks: Vec::new(),
            subs: Vec::new(),
            err: None,
            busy: false,
        };

        panel.wire_sliders(cx);
        panel.start_loops(cx);
        panel.refresh_profiles(cx);
        panel
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
        self.curve[i] = [t as u32, d as u32];
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

    pub fn poll_fans_now(&mut self, cx: &mut Context<Self>) {
        self.dispatch(cx, fan_read_cached, |panel, result, _cx| match result {
            Ok(fan) => {
                panel.fan = Some(fan);
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
                            panel.fan = Some(fan);
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
        let points: Vec<[u32; 2]> = self.curve.to_vec();
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
