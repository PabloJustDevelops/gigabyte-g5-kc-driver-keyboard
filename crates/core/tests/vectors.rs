//! The shared vectors: the same file `tests/test_g5fan.py` reads.
//!
//! `crates/core/testdata/curve_vectors.json` is generated from the daemon's own
//! implementation, and both sides assert against it. Before this existed, each
//! suite carried its own hand-copied expectations and a comment claiming they
//! matched — which is how the panel came to draw 11 % where the daemon asks for
//! 10 % (see the `tie` curve: 10.5 rounds half to even).

use g5core::fan::{self, Point};
use g5core::units;
use serde_json::Value;

const VECTORS: &str = include_str!("../testdata/curve_vectors.json");

fn vectors() -> Value {
    serde_json::from_str(VECTORS).expect("the shared vector file parses")
}

/// `[40, 10]` → `[40, 10]`.
fn point(value: &Value) -> Point {
    let pair = value.as_array().expect("a [temp, duty] pair");
    [i32_of(&pair[0]), i32_of(&pair[1])]
}

/// `[[40, 10], [60, 30]]` → `[[40, 10], [60, 30]]`.
fn points(value: &Value) -> Vec<Point> {
    value
        .as_array()
        .expect("a list of points")
        .iter()
        .map(point)
        .collect()
}

fn i32_of(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("an integer")).expect("fits in i32")
}

fn u32_of(value: &Value) -> u32 {
    u32::try_from(value.as_i64().expect("an integer")).expect("fits in u32")
}

fn u8_of(value: &Value) -> u8 {
    u8::try_from(value.as_i64().expect("an integer")).expect("fits in u8")
}

#[test]
fn the_curve_table_is_the_one_we_think_it_is() {
    let vectors = vectors();
    let curves = vectors["curves"].as_object().expect("curves");
    assert!(curves.contains_key("ramp4"), "{curves:?}");
    // The presets in the code and in the vectors are the same numbers: if a
    // preset is edited in one place only, this is what says so.
    for name in fan::CURVE_MODES {
        assert_eq!(
            fan::preset(name).expect("preset"),
            points(&curves[name]).as_slice(),
            "{name}"
        );
    }
}

#[test]
fn every_degree_of_every_curve_matches_the_daemon() {
    let vectors = vectors();
    let curves = vectors["curves"].as_object().expect("curves");
    let low = i32_of(&vectors["duty_sweep_temps"][0]);
    let high = i32_of(&vectors["duty_sweep_temps"][1]);
    let mut compared = 0;
    for (name, expected) in vectors["duty"].as_object().expect("duty") {
        let curve = points(&curves[name]);
        let expected = expected.as_array().expect("sweep");
        assert_eq!(expected.len(), (high - low + 1) as usize, "{name}");
        for (temp, want) in (low..=high).zip(expected) {
            assert_eq!(
                fan::duty_for(&curve, temp),
                u32_of(want),
                "{name} at {temp} °C"
            );
            compared += 1;
        }
    }
    // Six curves × 121 degrees: enough that an off-by-one in the sweep cannot
    // pass as "the vectors agree".
    assert_eq!(compared, 6 * 121);
}

/// Agreement with the daemon is not the same as being sane: a ramp that dips in
/// the middle would be faithfully reproduced on both sides.
#[test]
fn no_curve_in_the_vectors_ever_asks_a_fan_to_slow_down() {
    let vectors = vectors();
    for (name, expected) in vectors["duty"].as_object().expect("duty") {
        let duties: Vec<u32> = expected
            .as_array()
            .expect("sweep")
            .iter()
            .map(u32_of)
            .collect();
        assert!(
            duties.windows(2).all(|w| w[0] <= w[1]),
            "{name} falls: {duties:?}"
        );
    }
}

#[test]
fn the_roundings_tie_goes_to_even_like_python() {
    // The panel's old implementation rounded it away from zero and drew 11 %.
    let vectors = vectors();
    let curve = points(&vectors["curves"]["tie"]);
    assert_eq!(fan::duty_for(&curve, 41), 10);
    assert_eq!(fan::duty_for(&curve, 42), 11);
}

#[test]
fn duty_percentages_match_the_daemon() {
    for case in vectors()["pct_to_byte"].as_array().expect("pct").clone() {
        let pct = i32_of(&case["pct"]);
        assert_eq!(units::pct_to_byte(pct), u8_of(&case["byte"]), "{pct} %");
    }
}

#[test]
fn duty_readings_match_the_daemon() {
    for case in vectors()["byte_to_pct"].as_array().expect("byte").clone() {
        let byte = u8_of(&case["byte"]);
        assert_eq!(
            units::byte_to_pct(Some(byte)),
            Some(u8_of(&case["pct"])),
            "{byte}"
        );
    }
    assert_eq!(units::byte_to_pct(None), None);
}

#[test]
fn tacho_periods_match_the_daemon() {
    for case in vectors()["rpm_from_tach"].as_array().expect("tach").clone() {
        let period = case["period"].as_u64();
        assert_eq!(
            units::rpm_from_tach(period.map(|p| p as u32)),
            case["rpm"].as_u64().map(|r| r as u32),
            "{period:?}"
        );
    }
}

#[test]
fn the_valid_curves_validate() {
    for curve in vectors()["valid_curves"].as_array().expect("valid").clone() {
        let points = points(&curve);
        assert_eq!(fan::validate_curve(&points), Ok(()), "{points:?}");
    }
}

#[test]
fn the_invalid_curves_are_refused_in_the_daemons_own_words() {
    for case in vectors()["invalid_curves"]
        .as_array()
        .expect("invalid")
        .clone()
    {
        let points = points(&case["points"]);
        let want = case["message"].as_str().expect("message");
        assert_eq!(
            fan::curve_problem(&points).as_deref(),
            Some(want),
            "{points:?}"
        );
    }
}

#[test]
fn points_parse_and_junk_is_refused() {
    let vectors = vectors();
    for case in vectors["points"].as_array().expect("points").clone() {
        let text = case["text"].as_str().expect("text");
        assert_eq!(fan::parse_point(text), Ok(point(&case["point"])), "{text}");
    }
    for case in vectors["bad_points"].as_array().expect("bad").clone() {
        let text = case.as_str().expect("text");
        let error = fan::parse_point(text).expect_err("refused");
        assert!(error.message().contains("is not a T:D point"), "{text}");
    }
}
