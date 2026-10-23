//! The fan curve, the mode vocabulary and the `status --json` contract.
//!
//! Everything here is pure: no EC, no sysfs, no root. It is the part that
//! decides *what duty to command*, which is the part worth being careful about.
//!
//! The messages produced by [`validate_curve`] are the daemon's own wording,
//! copied verbatim from `src/g5fan.py` — the panel used to phrase them slightly
//! differently ("60%" against the daemon's "60 %"), and a user comparing the two
//! halves of the product should not have to wonder whether they disagree.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// A curve holds 2..5 points (the firmware's own table has five).
pub const CURVE_POINTS: usize = 5;

/// A point is `[temperature °C, duty %]`.
///
/// `i32`, not `u32`: a curve typed on the command line can contain a negative
/// number, and the daemon answers those with "temperature -5 is outside
/// 0..120 °C" rather than "that is not a point". Keeping them representable is
/// what lets both halves reject them with the same sentence.
pub type Point = [i32; 2];

/// The five modes the Windows Control Center exposes (its oem.ini calls them
/// 0:Auto 1:Max 3:Silent 5:MAXQ 6:Custom — "Max" is labelled Turbo in the UI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FanMode {
    Auto,
    Turbo,
    Silent,
    MaxQ,
    Custom,
    /// A duty the user pinned. It is a real mode of the daemon (and of
    /// `parse_mode`), but the panel has no chip for it: it is selected through
    /// the duty slider, not by picking a curve.
    Manual,
}

impl FanMode {
    /// Every mode the daemon has.
    pub const ALL: [FanMode; 6] = [
        FanMode::Auto,
        FanMode::Turbo,
        FanMode::Silent,
        FanMode::MaxQ,
        FanMode::Custom,
        FanMode::Manual,
    ];

    /// The modes the panel offers as chips, in the order it draws them.
    pub const UI_MODES: [FanMode; 5] = [
        FanMode::Auto,
        FanMode::Turbo,
        FanMode::Silent,
        FanMode::MaxQ,
        FanMode::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FanMode::Auto => "Auto",
            FanMode::Turbo => "Turbo",
            FanMode::Silent => "Silent",
            FanMode::MaxQ => "MaxQ",
            FanMode::Custom => "Custom",
            FanMode::Manual => "Manual",
        }
    }

    /// The word handed to `g5fan mode <word>`.
    pub fn as_cli_arg(self) -> &'static str {
        match self {
            FanMode::Auto => "auto",
            FanMode::Turbo => "turbo",
            FanMode::Silent => "silent",
            FanMode::MaxQ => "maxq",
            FanMode::Custom => "custom",
            FanMode::Manual => "manual",
        }
    }

    /// Curve-driven modes are evaluated by the daemon, not written once.
    pub fn is_curve(self) -> bool {
        matches!(self, FanMode::Silent | FanMode::MaxQ | FanMode::Custom)
    }

    pub fn from_status(mode: &str) -> Option<FanMode> {
        match mode {
            "auto" => Some(FanMode::Auto),
            "turbo" => Some(FanMode::Turbo),
            "silent" => Some(FanMode::Silent),
            "maxq" => Some(FanMode::MaxQ),
            "custom" => Some(FanMode::Custom),
            "manual" => Some(FanMode::Manual),
            _ => None,
        }
    }
}

/// Something wrong with a curve, phrased the way the daemon phrases it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurveError {
    message: String,
}

impl CurveError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CurveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CurveError {}

// --------------------------------------------------------------------------
// The built-in curves
// --------------------------------------------------------------------------
// Deliberately the same numbers as `PROFILES` in src/g5fan.py: these are what
// the fans actually run, so a "prettier" value here would be a lie.
pub const SILENT: &[Point] = &[[45, 15], [60, 25], [72, 40], [82, 65], [92, 100]];
pub const MAXQ: &[Point] = &[[48, 12], [62, 20], [75, 35], [85, 60], [92, 100]];
pub const CUSTOM: &[Point] = &[[50, 25], [65, 40], [75, 60], [85, 80], [95, 100]];

/// The curve-driven modes, in the order the UI lists them.
pub const CURVE_MODES: [&str; 3] = ["silent", "maxq", "custom"];

/// The built-in points for a curve mode, or `None` for a direct mode.
pub fn preset(name: &str) -> Option<&'static [Point]> {
    match name {
        "silent" => Some(SILENT),
        "maxq" => Some(MAXQ),
        "custom" => Some(CUSTOM),
        _ => None,
    }
}

// --------------------------------------------------------------------------
// The maths
// --------------------------------------------------------------------------
/// Piecewise-linear duty % for `temp`, clamped at the curve's ends.
///
/// `f64` and round-half-to-even, because this is a reimplementation of Python's
/// `round(d0 + (d1 - d0) * (temp - t0) / (t1 - t0))` and the two must agree on
/// every input, ties included.
pub fn duty_for(points: &[Point], temp: i32) -> u32 {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        // No curve means no fan command; the CLI would refuse an empty curve
        // long before this, and the panel's graph asks with nothing to draw.
        return 0;
    };
    if temp <= first[0] {
        return first[1].max(0) as u32;
    }
    if temp >= last[0] {
        return last[1].max(0) as u32;
    }
    for pair in points.windows(2) {
        let (t0, d0) = (pair[0][0], pair[0][1]);
        let (t1, d1) = (pair[1][0], pair[1][1]);
        if t0 <= temp && temp <= t1 {
            if t1 == t0 {
                return d1.max(0) as u32;
            }
            let value =
                f64::from(d0) + f64::from(d1 - d0) * f64::from(temp - t0) / f64::from(t1 - t0);
            return value.round_ties_even() as u32;
        }
    }
    last[1].max(0) as u32
}

/// Coerce to a safe curve: 2..5 points, rising temperatures, and duties that
/// never fall as the temperature climbs.
///
/// Anything else is refused rather than silently reordered — a curve that dips
/// is how a CPU cooks.
pub fn validate_curve(points: &[Point]) -> Result<(), CurveError> {
    if !(2..=CURVE_POINTS).contains(&points.len()) {
        return Err(CurveError::new(format!(
            "a curve needs 2..{CURVE_POINTS} points (got {})",
            points.len()
        )));
    }
    for [temp, duty] in points {
        if !(0..=120).contains(temp) {
            return Err(CurveError::new(format!(
                "temperature {temp} is outside 0..120 °C"
            )));
        }
        if !(0..=100).contains(duty) {
            return Err(CurveError::new(format!("duty {duty} is outside 0..100 %")));
        }
    }
    for pair in points.windows(2) {
        let (t0, d0) = (pair[0][0], pair[0][1]);
        let (t1, d1) = (pair[1][0], pair[1][1]);
        if t1 <= t0 {
            return Err(CurveError::new(format!(
                "temperatures must rise: {t0} °C then {t1} °C"
            )));
        }
        if d1 < d0 {
            return Err(CurveError::new(format!(
                "duty falls from {d0} % at {t0} °C to {d1} % at {t1} °C — a fan must not slow \
                 down as it gets hotter"
            )));
        }
    }
    Ok(())
}

/// The first thing wrong with a curve, or `None`. The panel's form of the
/// question, so it refuses exactly what the daemon refuses.
pub fn curve_problem(points: &[Point]) -> Option<String> {
    validate_curve(points)
        .err()
        .map(|e| e.message().to_string())
}

/// `'55:30'` → `(55, 30)`: a temperature in °C and a duty in %.
pub fn parse_point(text: &str) -> Result<Point, CurveError> {
    let complaint = || CurveError::new(format!("'{text}' is not a T:D point (for example 65:40)"));
    let (temp, duty) = text.split_once(':').ok_or_else(complaint)?;
    let temp = temp.trim().parse::<i32>().map_err(|_| complaint())?;
    let duty = duty.trim().parse::<i32>().map_err(|_| complaint())?;
    Ok([temp, duty])
}

/// Format a curve the way `g5fan curve set` takes it: `T:D` pairs.
pub fn format_curve(points: &[Point]) -> String {
    points
        .iter()
        .map(|p| format!("{}:{}", p[0], p[1]))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Accept `auto`, `turbo`/`max`, `silent`, `maxq`, `custom`, `manual:60` and
/// `manual 60`.
pub fn parse_mode(text: &str) -> Result<(FanMode, Option<u8>), CurveError> {
    let t = text.trim().to_lowercase();
    match t.as_str() {
        "auto" | "default" => return Ok((FanMode::Auto, None)),
        "turbo" | "max" => return Ok((FanMode::Turbo, None)),
        "silent" => return Ok((FanMode::Silent, None)),
        "maxq" => return Ok((FanMode::MaxQ, None)),
        "custom" => return Ok((FanMode::Custom, None)),
        _ => {}
    }
    if let Some(rest) = t.strip_prefix("manual") {
        // "manual:60" and "manual 60" both work; a bare "manual" keeps the duty
        // it already has.
        let rest = rest.strip_prefix(':').unwrap_or(rest).trim();
        if rest.is_empty() {
            return Ok((FanMode::Manual, None));
        }
        let pct = rest
            .parse::<i32>()
            .map_err(|_| CurveError::new("manual mode needs a percentage, e.g. manual:60"))?;
        if !(0..=100).contains(&pct) {
            return Err(CurveError::new(format!(
                "manual duty must be 0..100 (got {pct})"
            )));
        }
        let duty = u8::try_from(pct)
            .map_err(|_| CurveError::new(format!("manual duty must be 0..100 (got {pct})")))?;
        return Ok((FanMode::Manual, Some(duty)));
    }
    Err(CurveError::new(format!(
        "unknown mode '{text}' (try: auto, turbo, silent, maxq, custom, manual:60)"
    )))
}

/// How a mode is written in `g5fan status` and after a successful switch.
pub fn mode_label(mode: &str, duty: Option<u8>, points: Option<&[Point]>) -> String {
    if mode == "manual"
        && let Some(duty) = duty
    {
        return format!("manual {duty}%");
    }
    if mode == "turbo" {
        return "turbo (100%)".to_string();
    }
    if let Some(points) = points {
        return format!("{mode} (curve {})", format_curve(points));
    }
    mode.to_string()
}

// --------------------------------------------------------------------------
// The status contract
// --------------------------------------------------------------------------
// `g5fan status --json` is the panel's only way of looking at the fans, so these
// field names are a contract: the CLI's key-set test (in tests/test_g5fan.py)
// and the parse test below are the two halves of it. A rename that happens on
// one side makes the panel draw an empty fan list instead of failing, which is
// exactly the kind of bug nobody notices.

/// One fan's live reading.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawFanReading {
    pub label: String,
    pub duty_pct: Option<u8>,
    pub rpm: Option<u32>,
    pub tacho: Option<u32>,
}

/// Everything the panel needs to draw the fans.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawFanStatus {
    pub mode: String,
    pub backend: String,
    pub driver: bool,
    pub manual_duty: Option<u8>,
    /// the curve actually in play (null unless a curve mode is selected)
    pub curve: Option<Vec<Point>>,
    /// the saved custom curve, whether or not it is in play
    pub custom_curve: Vec<Point>,
    /// the built-in presets, so the panel never hard-codes a copy
    pub presets: BTreeMap<String, Vec<Point>>,
    pub fans: Vec<RawFanReading>,
    pub cpu_temp_c: Option<i32>,
    pub gpu_temp_c: Option<i32>,
    pub ceiling_c: f64,
    pub daemon: bool,
    /// When the reading was taken, in seconds since the epoch. This is what
    /// [`crate::state::read_snapshot`] turns into an age, and the age is how the
    /// panel tells a live daemon from one that died, so a reading without one is
    /// not a reading.
    #[serde(default)]
    pub generated: f64,
    /// Filled in by the reader, not by the daemon: the snapshot on disk carries
    /// a generation time and the reader subtracts.
    #[serde(default)]
    pub age_s: f64,
    #[serde(default)]
    pub stale: bool,
}

/// The `status --json` field sets, as one place both sides can point at.
pub const STATUS_KEYS: [&str; 13] = [
    "mode",
    "backend",
    "driver",
    "manual_duty",
    "curve",
    "custom_curve",
    "presets",
    "fans",
    "cpu_temp_c",
    "gpu_temp_c",
    "ceiling_c",
    "daemon",
    "generated",
];
pub const FAN_KEYS: [&str; 4] = ["label", "duty_pct", "rpm", "tacho"];

#[cfg(test)]
mod tests {
    use super::*;

    const RAMP: &[Point] = &[[40, 10], [60, 30], [80, 70], [100, 100]];

    #[test]
    fn duty_clamps_below_and_above() {
        assert_eq!(duty_for(RAMP, 0), 10);
        assert_eq!(duty_for(RAMP, 40), 10);
        assert_eq!(duty_for(RAMP, 100), 100);
        assert_eq!(duty_for(RAMP, 150), 100);
    }

    #[test]
    fn duty_interpolates_between_points() {
        assert_eq!(duty_for(RAMP, 60), 30);
        assert_eq!(duty_for(RAMP, 70), 50);
        assert_eq!(duty_for(RAMP, 50), 20);
    }

    /// The tie the panel's old `f32`/`round()` copy got wrong: 10.5 rounds to 10
    /// half-to-even, to 11 away from zero.
    #[test]
    fn duty_rounds_ties_to_even_like_python() {
        assert_eq!(duty_for(&[[40, 10], [42, 11]], 41), 10);
    }

    #[test]
    fn duty_never_falls_as_it_heats() {
        let duties: Vec<u32> = (0..=120).map(|t| duty_for(RAMP, t)).collect();
        assert!(duties.windows(2).all(|w| w[0] <= w[1]), "{duties:?}");
    }

    #[test]
    fn duty_without_a_curve_is_zero() {
        assert_eq!(duty_for(&[], 60), 0);
    }

    #[test]
    fn a_sensible_curve_validates() {
        assert_eq!(validate_curve(&[[45, 15], [60, 25], [92, 100]]), Ok(()));
        assert_eq!(validate_curve(&[[40, 100], [90, 100]]), Ok(()));
    }

    #[test]
    fn a_dipping_curve_is_refused() {
        let problem = curve_problem(&[[50, 60], [70, 20]]).expect("refused");
        assert!(problem.contains("fall"), "{problem}");
        assert!(problem.contains("must not slow down"), "{problem}");
    }

    #[test]
    fn a_curve_that_does_not_rise_is_refused() {
        assert_eq!(
            curve_problem(&[[70, 20], [50, 40]]).as_deref(),
            Some("temperatures must rise: 70 °C then 50 °C")
        );
        assert_eq!(
            curve_problem(&[[70, 20], [70, 40]]).as_deref(),
            Some("temperatures must rise: 70 °C then 70 °C")
        );
    }

    #[test]
    fn point_count_is_bounded() {
        assert_eq!(
            curve_problem(&[[50, 20]]).as_deref(),
            Some("a curve needs 2..5 points (got 1)")
        );
        let six: Vec<Point> = (0..6).map(|i| [40 + i, 10 * i]).collect();
        assert_eq!(
            curve_problem(&six).as_deref(),
            Some("a curve needs 2..5 points (got 6)")
        );
    }

    #[test]
    fn out_of_range_values_are_refused_by_name() {
        assert_eq!(
            curve_problem(&[[50, 20], [200, 40]]).as_deref(),
            Some("temperature 200 is outside 0..120 °C")
        );
        assert_eq!(
            curve_problem(&[[50, 20], [70, 140]]).as_deref(),
            Some("duty 140 is outside 0..100 %")
        );
    }

    #[test]
    fn points_parse() {
        assert_eq!(parse_point("55:30"), Ok([55, 30]));
        assert_eq!(parse_point(" 55 : 30 "), Ok([55, 30]));
        // A negative temperature parses, so that validation can name it the way
        // the daemon does.
        assert_eq!(parse_point("-5:30"), Ok([-5, 30]));
        for bad in ["55", "abc", "55:", ":30"] {
            let error = parse_point(bad).expect_err("refused");
            assert!(error.message().contains("is not a T:D point"), "{bad}");
        }
    }

    #[test]
    fn modes_parse() {
        assert_eq!(parse_mode("auto"), Ok((FanMode::Auto, None)));
        assert_eq!(parse_mode("MAX"), Ok((FanMode::Turbo, None)));
        assert_eq!(parse_mode("MaxQ"), Ok((FanMode::MaxQ, None)));
        assert_eq!(parse_mode("manual:60"), Ok((FanMode::Manual, Some(60))));
        assert_eq!(parse_mode("manual 60"), Ok((FanMode::Manual, Some(60))));
        assert_eq!(parse_mode("manual"), Ok((FanMode::Manual, None)));
    }

    #[test]
    fn manual_bounds_are_enforced() {
        assert_eq!(
            parse_mode("manual:200").expect_err("refused").message(),
            "manual duty must be 0..100 (got 200)"
        );
        assert_eq!(
            parse_mode("manual:abc").expect_err("refused").message(),
            "manual mode needs a percentage, e.g. manual:60"
        );
    }

    #[test]
    fn an_unknown_mode_names_itself() {
        assert!(
            parse_mode("ludicrous")
                .expect_err("refused")
                .message()
                .contains("'ludicrous'")
        );
    }

    #[test]
    fn the_presets_are_always_legal_curves() {
        for name in CURVE_MODES {
            let points = preset(name).expect("preset");
            assert_eq!(validate_curve(points), Ok(()), "{name}");
        }
    }

    #[test]
    fn labels_read_like_the_cli() {
        assert_eq!(mode_label("manual", Some(60), None), "manual 60%");
        assert_eq!(mode_label("turbo", None, None), "turbo (100%)");
        assert_eq!(
            mode_label("custom", None, Some(&[[50, 25], [95, 100]])),
            "custom (curve 50:25 95:100)"
        );
        assert_eq!(mode_label("auto", None, None), "auto");
    }

    #[test]
    fn every_mode_round_trips_through_its_cli_word() {
        for mode in FanMode::ALL {
            assert_eq!(FanMode::from_status(mode.as_cli_arg()), Some(mode));
        }
        assert_eq!(FanMode::from_status("manual"), Some(FanMode::Manual));
        assert!(FanMode::from_status("ludicrous").is_none());
    }

    #[test]
    fn only_curve_modes_are_driven_by_the_daemon() {
        assert!(!FanMode::Auto.is_curve());
        assert!(FanMode::Custom.is_curve());
        assert!(!FanMode::Manual.is_curve());
    }

    /// The panel has no chip for a pinned duty, so its chip list is shorter
    /// than the daemon's mode list. That is the difference between "what the
    /// CLI accepts" and "what the UI offers", and it is deliberate.
    #[test]
    fn the_panel_offers_five_of_the_six_modes() {
        assert_eq!(FanMode::ALL.len(), 6);
        assert_eq!(FanMode::UI_MODES.len(), 5);
        assert!(!FanMode::UI_MODES.contains(&FanMode::Manual));
    }

    #[test]
    fn curve_formats_as_cli_args() {
        assert_eq!(format_curve(&[[50, 25], [95, 100]]), "50:25 95:100");
    }

    /// The panel's only way of looking at the fans is `g5fan status --cached
    /// --json`, so this snapshot is the contract with the CLI.
    const SAMPLE: &str = r#"{
        "mode": "silent",
        "backend": "kernel",
        "driver": true,
        "manual_duty": null,
        "curve": [[45, 15], [60, 25], [72, 40], [82, 65], [92, 100]],
        "custom_curve": [[50, 25], [65, 40], [75, 60], [85, 80], [95, 100]],
        "presets": {
            "silent": [[45, 15]], "maxq": [[48, 12]], "custom": [[50, 25]]
        },
        "fans": [
            {"label": "CPU", "duty_pct": 20, "rpm": 2218, "tacho": 972},
            {"label": "GPU", "duty_pct": 15, "rpm": 1737, "tacho": 1241}
        ],
        "cpu_temp_c": 52,
        "gpu_temp_c": 44,
        "ceiling_c": 100.0,
        "daemon": true,
        "age_s": 1.1,
        "stale": false
    }"#;

    #[test]
    fn parses_a_fan_status_snapshot() {
        let s: RawFanStatus = serde_json::from_str(SAMPLE).expect("parse");
        assert_eq!(s.mode, "silent");
        assert_eq!(s.backend, "kernel");
        assert!(s.driver && s.daemon && !s.stale);
        assert_eq!(s.manual_duty, None);
        assert_eq!(s.curve.as_ref().map(Vec::len), Some(5));
        assert_eq!(s.custom_curve.len(), 5);
        assert_eq!(s.presets["custom"], vec![[50, 25]]);
        assert_eq!(s.fans.len(), 2);
        assert_eq!(s.fans[0].label, "CPU");
        assert_eq!(s.fans[0].duty_pct, Some(20));
        assert_eq!(s.fans[0].rpm, Some(2218));
        assert_eq!(s.fans[0].tacho, Some(972));
        assert_eq!(s.cpu_temp_c, Some(52));
        assert_eq!(s.gpu_temp_c, Some(44));
        assert_eq!(s.age_s, 1.1);
    }

    /// A fan the EC cannot report, and a machine whose EC gives no
    /// temperature, must come through as nulls rather than as a parse error.
    #[test]
    fn tolerates_missing_readings() {
        let json = r#"{
            "mode": "auto", "backend": "ec", "driver": false,
            "manual_duty": null, "curve": null,
            "custom_curve": [[50, 25], [95, 100]],
            "presets": {},
            "fans": [{"label": "CPU", "duty_pct": null,
                      "rpm": null, "tacho": null}],
            "cpu_temp_c": null, "gpu_temp_c": null, "ceiling_c": 90.0,
            "daemon": false, "age_s": 0.0, "stale": true
        }"#;
        let s: RawFanStatus = serde_json::from_str(json).expect("parse");
        assert_eq!(s.fans[0].duty_pct, None);
        assert_eq!(s.fans[0].rpm, None);
        assert_eq!(s.cpu_temp_c, None);
        assert!(s.curve.is_none() && s.stale && !s.driver);
    }
}
