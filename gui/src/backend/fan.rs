// Fan status, as `g5fan status --json` reports it. The field names below are
// a contract with the CLI (see `status_data` in src/g5fan.py and its key-set
// test in tests/test_g5fan.py); if the CLI renames one, these tests break
// here instead of the panel quietly drawing an empty fan list.

use serde::{Deserialize, Serialize};

/// The five modes the Windows Control Center exposes (its oem.ini calls them
/// 0:Auto 1:Max 3:Silent 5:MAXQ 6:Custom — "Max" is labelled Turbo in the UI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FanMode {
    Auto,
    Turbo,
    Silent,
    MaxQ,
    Custom,
}

impl FanMode {
    pub const ALL: [FanMode; 5] = [
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
            _ => None,
        }
    }
}

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
    pub curve: Option<Vec<[u32; 2]>>,
    /// the saved custom curve, whether or not it is in play
    pub custom_curve: Vec<[u32; 2]>,
    /// the built-in presets, so the panel never hard-codes a copy
    pub presets: std::collections::BTreeMap<String, Vec<[u32; 2]>>,
    pub fans: Vec<RawFanReading>,
    pub cpu_temp_c: Option<i32>,
    pub gpu_temp_c: Option<i32>,
    pub ceiling_c: f64,
    pub daemon: bool,
    pub age_s: f64,
    pub stale: bool,
}

/// First thing wrong with a curve, or None. Mirrors validate_curve in
/// src/g5fan.py so the panel refuses exactly what the CLI refuses.
pub fn curve_problem(curve: &[[u32; 2]]) -> Option<String> {
    if curve.len() < 2 {
        return Some("A curve needs at least two points.".into());
    }
    for w in curve.windows(2) {
        if w[1][0] <= w[0][0] {
            return Some(format!(
                "temperatures must rise: {} °C then {} °C",
                w[0][0], w[1][0]
            ));
        }
        if w[1][1] < w[0][1] {
            return Some(format!(
                "duty falls from {}% at {} °C to {}% at {} °C — a fan must not slow down as it gets hotter",
                w[0][1], w[0][0], w[1][1], w[1][0]
            ));
        }
    }
    None
}

/// Format the curve the way `g5fan curve set` takes it: `T:D` pairs.
pub fn format_curve(curve: &[[u32; 2]]) -> String {
    curve
        .iter()
        .map(|p| format!("{}:{}", p[0], p[1]))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panel's only way of looking at the fans is `g5fan status --json
    /// --cached`, so this snapshot is the contract with the CLI.
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

    #[test]
    fn curve_validation_matches_the_cli() {
        assert!(curve_problem(&[[50, 25], [65, 40], [95, 100]]).is_none());

        assert_eq!(
            curve_problem(&[[65, 40], [50, 25]]).as_deref(),
            Some("temperatures must rise: 65 °C then 50 °C")
        );
        assert!(
            curve_problem(&[[50, 40], [65, 25]])
                .unwrap()
                .contains("must not slow down")
        );
        assert_eq!(
            curve_problem(&[[50, 25]]).as_deref(),
            Some("A curve needs at least two points.")
        );
    }

    #[test]
    fn curve_formats_as_cli_args() {
        assert_eq!(format_curve(&[[50, 25], [95, 100]]), "50:25 95:100");
    }

    #[test]
    fn fan_mode_round_trips() {
        for mode in FanMode::ALL {
            assert_eq!(FanMode::from_status(mode.as_cli_arg()), Some(mode));
        }
        assert!(FanMode::from_status("manual").is_none());
        assert!(!FanMode::Auto.is_curve());
        assert!(FanMode::Custom.is_curve());
    }
}
