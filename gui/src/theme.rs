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
pub const WHITE: Rgba = rgb(0xffffff);
pub const BLACK: Rgba = rgb(0x000000);

/// An `Hsla` for shadows and glows, which is what `BoxShadow` takes. The
/// shadows are the port of the web panel's `box-shadow` rules, so they belong
/// next to the palette rather than inlined at each call site.
pub fn hsla(c: Rgba, a: f32) -> gpui_kit::Hsla {
    gpui_kit::hsla(c.r, c.g, c.b, a.clamp(0., 1.))
}

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

/// IBM Plex, compiled into the binary. Bundling it is what makes the panel look
/// the same on every machine — the web build shipped these files, so a user
/// without Plex installed was silently getting substituted metrics. Three faces
/// is all the panel uses (regular, bold, mono regular); the full family is
/// ~40 weights and would cost ~15 MB for no visible gain.
///
/// SIL Open Font License 1.1; see `fonts/OFL.txt`.
const PLEX_SANS: &[u8] = include_bytes!("../fonts/IBMPlexSans-Regular.ttf");
const PLEX_SANS_BOLD: &[u8] = include_bytes!("../fonts/IBMPlexSans-Bold.ttf");
const PLEX_MONO: &[u8] = include_bytes!("../fonts/IBMPlexMono-Regular.ttf");

/// Families we would like, best first. The first one the machine actually
/// has wins, and the rest become the fallback chain. The bundled Plex is
/// registered with the app, so it is normally the first match; the list keeps
/// the panel legible if the fonts ever fail to register.
const UI_CANDIDATES: &[&str] = &[
    "IBM Plex Sans",
    "Inter",
    "Noto Sans",
    "DejaVu Sans",
    "Cantarell",
];

/// Monospace for readouts: hex, temperatures, duty percentages.
const MONO_CANDIDATES: &[&str] = &[
    "IBM Plex Mono",
    "JetBrains Mono",
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "Hack",
];

type FontChain = gpui_kit::FontFallbacks;

static UI_FONT: std::sync::OnceLock<(gpui_kit::SharedString, FontChain)> =
    std::sync::OnceLock::new();
static MONO_FONT: std::sync::OnceLock<(gpui_kit::SharedString, FontChain)> =
    std::sync::OnceLock::new();

/// Pick the best available family from `candidates` and chain the rest behind
/// it, so a glyph that is missing from the winner still renders.
fn resolve(cx: &gpui_kit::App, candidates: &[&str]) -> (gpui_kit::SharedString, FontChain) {
    let available = cx.text_system().all_font_names();
    let chain: Vec<String> = candidates
        .iter()
        .filter(|name| available.iter().any(|have| have == *name))
        .map(|name| (*name).to_string())
        .collect();
    match chain.split_first() {
        Some((first, rest)) => (
            gpui_kit::SharedString::from(first.clone()),
            FontChain::from_fonts(rest.to_vec()),
        ),
        // Nothing matched; let GPUI pick its default rather than render tofu.
        None => (
            gpui_kit::SharedString::from(candidates[0]),
            FontChain::from_fonts(candidates[1..].iter().map(|n| (*n).to_string()).collect()),
        ),
    }
}

/// The UI font. `apply_kit_theme` fills this in at startup; before that it is
/// an empty chain, which renders with GPUI's default family.
pub fn ui_font() -> gpui_kit::Font {
    let mut font = gpui_kit::Font::default();
    if let Some((family, fallbacks)) = UI_FONT.get() {
        font.family = family.clone();
        font.fallbacks = Some(fallbacks.clone());
    }
    font
}

/// The monospace font, for readouts that should line up in a column.
pub fn mono_font() -> gpui_kit::Font {
    let mut font = gpui_kit::Font::default();
    if let Some((family, fallbacks)) = MONO_FONT.get() {
        font.family = family.clone();
        font.fallbacks = Some(fallbacks.clone());
    }
    font
}

/// Pin the component library's theme to ours.
///
/// The kit ships its own palette (`gpui_component::theme`) and renders every
/// `Button`, `Slider` and `Input` from it. Left alone it follows the desktop's
/// light/dark setting, which on a light-themed desktop painted white buttons
/// inside our dark cards. We also repaint its slider track: the default is a
/// near-white bar that, at full width, is the loudest thing on the screen.
///
/// `main.rs` calls this once at startup; `tests/snapshot.rs` calls it too, so
/// snapshots match what ships.
pub fn apply_kit_theme(cx: &mut gpui_kit::App) {
    use gpui_kit::component::theme::{Theme, ThemeMode};

    Theme::change(ThemeMode::Dark, None, cx);
    // Register the bundled faces *before* asking which families exist, so the
    // resolution below picks them.
    cx.text_system()
        .add_fonts(
            [PLEX_SANS, PLEX_SANS_BOLD, PLEX_MONO]
                .into_iter()
                .map(std::borrow::Cow::Borrowed)
                .collect(),
        )
        .expect("register the bundled IBM Plex faces");
    let _ = UI_FONT.set(resolve(cx, UI_CANDIDATES));
    let _ = MONO_FONT.set(resolve(cx, MONO_CANDIDATES));
    // `slider_bar` is the *filled* part of the track, not the groove. Its dark
    // default is near-white, which turned every slider into a bright bar and
    // made the loudest thing on the Lighting view a 3 px line. The groove
    // already reads correctly; the fill should be the accent.
    let fill: gpui_kit::Hsla = ACCENT.into();
    let thumb: gpui_kit::Hsla = WHITE.into();
    Theme::update(cx, |theme| {
        theme.colors.slider_bar = fill;
        theme.colors.slider_thumb = thumb;
    });
}

/// Darken towards black — the web panel's `brightness(0.5)` off state, which
/// greyed the whole board instead of only zeroing the glow.
pub fn dim(c: Rgba, t: f32) -> Rgba {
    mix(c, BLACK, t)
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
