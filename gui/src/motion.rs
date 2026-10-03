//! Motion, ported from the web panel's timings.
//!
//! The web CSS used `transition: 120ms` on hover and press, and a short fade
//! when the page changed. This module keeps those numbers in one place so a
//! future tweak is a one-line change rather than a hunt through the views.
//!
//! Everything routes through `gpui_base`'s motion helpers rather than
//! `Animation`/`with_animation` directly. Those consult `App::reduce_motion`
//! on every sample, and `gpui_kit::init` reads the desktop's reduced-motion
//! preference into that flag through the XDG portal — so following the system
//! setting is free, and `main.rs` and the snapshot harness both get it because
//! both call `gpui_kit::init`.
//!
//! The distinction the two helpers draw matters here:
//!
//! - [`hover`] is a [`transition`]: a fixed-duration ease toward a target. Right
//!   for a colour that flips between two known values.
//! - [`glide`] is a [`spring`]: it carries velocity across a retarget. Right
//!   for the gauges, whose target moves every poll and would otherwise restart
//!   their easing from a standstill each time.

use std::time::Duration;

use gpui_kit::base::{
    Easing, IterationCount, Keyframe, Keyframes, PlaybackDirection, Spring, Timing, Transition,
    TransitionId, animate_keyframes, spring, transition,
};
use gpui_kit::{App, Window};

use crate::theme;
use gpui_kit::Rgba;

/// Hover and press feedback. The web panel's `--t-fast`.
pub const FEEDBACK: Duration = Duration::from_millis(120);

/// The view-switch fade. Long enough to read as motion, short enough not to
/// delay the content the user asked for.
pub const SWITCH: Duration = Duration::from_millis(180);

/// One breath of the "an effect is running" dot.
pub const BREATH: Duration = Duration::from_millis(1600);

/// How long a gauge takes to catch up with a new reading.
pub const GLIDE: Duration = Duration::from_millis(280);

/// CSS `ease`, which is what `--t-fast` was declared with.
fn ease() -> Easing {
    Easing::Ease
}

/// 0 → 1 as `on` turns true, over [`FEEDBACK`].
///
/// `id` must be stable for the element it decorates: the value lives in window
/// state under that id, so a fresh id is what makes the animation replay. It
/// also has to be unique per element, or two elements would share one value.
pub fn hover(id: impl Into<TransitionId>, on: bool, window: &mut Window, cx: &mut App) -> f32 {
    transition(
        id,
        if on { 1.0 } else { 0.0 },
        Transition::new(FEEDBACK).easing(ease()),
        window,
        cx,
    )
}

/// Fade `content` in when the view changes.
///
/// `seq` is the view generation: it increments on every switch, so returning
/// to a view already seen replays the fade instead of resuming a stale one.
pub fn fade(seq: u64, window: &mut Window, cx: &mut App) -> f32 {
    // Built per call rather than cached: `Keyframes` holds an `Easing`, which
    // holds an `Rc`, so it is neither `Send` nor `Sync` and cannot live in a
    // `static`. Two keyframes is cheaper than the machinery to avoid that.
    let frames = Keyframes::try_new([Keyframe::new(0.0, 0.0), Keyframe::new(1.0, 1.0)])
        .expect("fade keyframes run from 0 to 1");
    animate_keyframes(
        (seq, "view-fade"),
        &frames,
        Timing::new(SWITCH).ease(ease()),
        window,
        cx,
    )
    .value
}

/// The pulsing dot shown while a keyboard effect is running.
///
/// Infinite and alternating, so it breathes rather than blinks. Under reduced
/// motion `animate_keyframes` returns the end of the animation and asks for no
/// further frames, which leaves a steady dot — visible, but not moving.
pub fn breath(id: impl Into<TransitionId>, window: &mut Window, cx: &mut App) -> f32 {
    let frames = Keyframes::try_new([Keyframe::new(0.0, 0.0), Keyframe::new(1.0, 1.0)])
        .expect("breath keyframes run from 0 to 1");
    animate_keyframes(
        id,
        &frames,
        Timing::new(BREATH)
            .iterations(IterationCount::Infinite)
            .direction(PlaybackDirection::Alternate)
            .ease(Easing::EaseInOut),
        window,
        cx,
    )
    .value
}

/// Ease a value toward `to` over [`GLIDE`], keeping velocity across retargets.
///
/// The gauges poll every three seconds, so their target is always moving. A
/// spring is the right tool: interrupting a fixed-duration ease mid-flight
/// restarts its curve from zero velocity, which reads as a stutter every time
/// the value shifts.
pub fn glide(id: impl Into<TransitionId>, to: f32, window: &mut Window, cx: &mut App) -> f32 {
    spring(id, to, Spring::new(GLIDE), window, cx)
}

/// Blend `from` towards `to` by `t`, for callers that hold a 0→1 amount from
/// [`hover`] and need a colour out of it.
pub fn blend(from: Rgba, to: Rgba, t: f32) -> Rgba {
    theme::mix(from, to, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_matches_the_web_timing() {
        assert_eq!(FEEDBACK, Duration::from_millis(120));
        assert!(SWITCH > FEEDBACK, "a view switch should outlast a hover");
    }

    #[test]
    fn blending_is_the_same_mix_the_palette_uses() {
        let a = theme::FIELD;
        let b = theme::ACCENT;
        assert_eq!(blend(a, b, 0.0), a);
        assert_eq!(blend(a, b, 1.0), b);
        assert_eq!(blend(a, b, 0.5), theme::mix(a, b, 0.5));
    }
}
