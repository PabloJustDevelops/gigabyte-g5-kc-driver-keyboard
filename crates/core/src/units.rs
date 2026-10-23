//! Duty percentages ↔ EC bytes, and tacho periods ↔ rpm.
//!
//! The arithmetic is not symmetric and not obvious, which is why it lives in one
//! place instead of two. Two traps, both real:
//!
//! * The duty byte is `pct * 255 / 100` rounded, and the daemon's implementation
//!   is Python, whose `round()` is round-half-to-**even**: 30 % is 76, not 77.
//!   Rust's `f64::round` rounds half away from zero, so it has to be
//!   `round_ties_even` — and it has to be `f64`, because the panel's old `f32`
//!   copy could lose the tie entirely.
//! * The tacho register holds a *period*, not a speed: rpm is
//!   `2_156_220 / period`, which is easy to get wrong by a factor of ~23.

/// The standard Clevo tacho constant: rpm = this / period.
pub const RPM_CONST: u32 = 2_156_220;

/// A stopped fan reports either 0 or an 0xFFxx sentinel.
pub const TACH_STOPPED: u32 = 0xFF00;

/// Duty percentage to the byte the EC takes, clamped to 0..100 %.
///
/// Round-half-to-even, deliberately: see the module comment.
pub fn pct_to_byte(pct: i32) -> u8 {
    let clamped = f64::from(pct.clamp(0, 100));
    (clamped * 255.0 / 100.0).round_ties_even() as u8
}

/// The EC's duty byte back to a percentage, or `None` when there is no reading.
pub fn byte_to_pct(raw: Option<u8>) -> Option<u8> {
    // No `u8` value can land exactly on a tie here (it would need
    // 2·raw·100 = 255·(2k+1), impossible for an integer raw), but the rounding
    // mode is spelled out anyway so the two sides cannot drift apart.
    raw.map(|b| (f64::from(b) * 100.0 / 255.0).round_ties_even() as u8)
}

/// Tacho period to rpm: 0 for a stopped fan, `None` when there is no reading.
pub fn rpm_from_tach(period: Option<u32>) -> Option<u32> {
    let period = period?;
    if period == 0 || period >= TACH_STOPPED {
        return Some(0);
    }
    Some(RPM_CONST / period)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pct_to_byte_covers_the_ends() {
        assert_eq!(pct_to_byte(0), 0);
        assert_eq!(pct_to_byte(100), 255);
    }

    #[test]
    fn pct_to_byte_clamps() {
        assert_eq!(pct_to_byte(-20), 0);
        assert_eq!(pct_to_byte(1000), 255);
    }

    #[test]
    fn pct_to_byte_never_goes_backwards() {
        let levels: Vec<u8> = (0..=100).map(pct_to_byte).collect();
        assert!(levels.windows(2).all(|w| w[0] <= w[1]), "{levels:?}");
    }

    /// 30 % is 76.5 in bytes. Python's `round()` gives 76 — half to even — and
    /// the panel's old `f32` copy rounded it away from zero, to 77.
    #[test]
    fn pct_to_byte_rounds_half_to_even_like_python() {
        assert_eq!(pct_to_byte(30), 76);
        assert_eq!(pct_to_byte(50), 128);
    }

    #[test]
    fn byte_to_pct_reads_back() {
        assert_eq!(byte_to_pct(Some(255)), Some(100));
        assert_eq!(byte_to_pct(Some(0)), Some(0));
        assert_eq!(byte_to_pct(None), None);
    }

    #[test]
    fn tacho_is_a_period() {
        // What a live G5 KC reports for its CPU fan at a 35 % duty.
        assert_eq!(rpm_from_tach(Some(972)), Some(2218));
        assert_eq!(rpm_from_tach(Some(1241)), Some(1737));
    }

    #[test]
    fn a_stopped_fan_is_zero_not_a_huge_number() {
        assert_eq!(rpm_from_tach(Some(0)), Some(0));
        assert_eq!(rpm_from_tach(Some(TACH_STOPPED)), Some(0));
        assert_eq!(rpm_from_tach(Some(0xFFF0)), Some(0));
    }

    #[test]
    fn no_reading_is_none() {
        assert_eq!(rpm_from_tach(None), None);
    }

    #[test]
    fn a_faster_fan_has_a_shorter_period() {
        assert!(rpm_from_tach(Some(500)) > rpm_from_tach(Some(1500)));
    }
}
