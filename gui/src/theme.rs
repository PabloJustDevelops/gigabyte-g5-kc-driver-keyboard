// Colour palette, ported 1:1 from the web panel's styles.css so the native
// app looks like the same product. Neutral near-black chrome; the only
// saturated accent on screen is the live keyboard colour.

use gpui_kit::Rgba;

/// `rgb()` from gpui is not const, and these are needed in const position;
/// this is the same conversion, const-evaluable.
const fn rgb(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.,
        g: ((hex >> 8) & 0xff) as f32 / 255.,
        b: (hex & 0xff) as f32 / 255.,
        a: 1.,
    }
}

pub const BG: Rgba = rgb(0x0b0d10);
pub const PANEL: Rgba = rgb(0x14171c);
pub const PANEL2: Rgba = rgb(0x191d24);
pub const FIELD: Rgba = rgb(0x101318);
pub const LINE: Rgba = rgb(0x272b33);
pub const LINE_STRONG: Rgba = rgb(0x3a3f4a);
pub const TEXT: Rgba = rgb(0xeceff4);
pub const MUTED: Rgba = rgb(0xa2aab6);
pub const FAINT: Rgba = rgb(0x858e9d);
pub const OK: Rgba = rgb(0x3ddc84);
pub const WARN: Rgba = rgb(0xffb454);
pub const DANGER: Rgba = rgb(0xff5c6c);
/// Resting accent (the LED default); the live accent follows the glow.
pub const ACCENT: Rgba = rgb(0x00aaff);

/// Mix two colours by `t` (0 = a, 1 = b). Used to tint keycaps with the glow.
pub fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0., 1.);
    Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.,
    }
}

/// Rgba from `RRGGBB` hex text, or None. Same shape as the CLI accepts.
pub fn parse_hex(text: &str) -> Option<Rgba> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(gpui_kit::rgb(v))
}

pub fn hex_of(c: Rgba) -> String {
    let byte = |f: f32| (f.clamp(0., 1.) * 255.).round() as u8;
    format!("{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let c = parse_hex("00aaff").expect("valid hex");
        assert_eq!(hex_of(c), "00aaff");
        assert!(parse_hex("00aaf").is_none());
        assert!(parse_hex("00aaZZ").is_none());
        assert!(parse_hex("#ff0066").is_some(), "leading # is accepted");
    }
}
