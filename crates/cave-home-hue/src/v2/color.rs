// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4 homeassistant/util/color.py
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/models/light.py (gamut handling)
//! Hue colour science — CIE 1931 `xy` ⇄ sRGB ⇄ HSV, mirek ⇄ kelvin, and
//! gamut-triangle clamping.
//!
//! Line-by-line port of Home Assistant's `homeassistant/util/color.py`
//! colour-space helpers (Apache-2.0), with the Philips Hue gamut A/B/C
//! triangles from the Hue developer docs.
//!
//! The Hue CLIP API speaks `xy` ([`ColorPoint`]) plus a `mirek` colour
//! temperature; grandma-friendly callers want RGB / HSV. These conversions are
//! the bridge between the two. Everything here is pure `f64` arithmetic — no
//! IO, no `unsafe`, deterministic — so it is exercised entirely by unit tests.

// Colour maths is inherently lossy `f64 -> u8` rounding; the pedantic cast
// lints fire on every channel clamp and are noise here.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::suboptimal_flops,
    clippy::imprecise_flops,
    clippy::cast_lossless
)]

use crate::v2::models::feature::ColorPoint;

/// A CIE 1931 `xy` chromaticity coordinate.
/// Source: `aiohue` light gamut handling / HA `util.color.XYPoint`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XyPoint {
    pub x: f64,
    pub y: f64,
}

/// A lamp's reachable colour triangle (three `xy` corners).
/// Source: HA `util.color.GamutType`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gamut {
    pub red: XyPoint,
    pub green: XyPoint,
    pub blue: XyPoint,
}

/// Philips Hue **Gamut A** (early `LivingColors` / `LightStrips`).
pub const GAMUT_A: Gamut = Gamut {
    red: XyPoint { x: 0.704, y: 0.296 },
    green: XyPoint { x: 0.2151, y: 0.7106 },
    blue: XyPoint { x: 0.138, y: 0.080 },
};

/// Philips Hue **Gamut B** (first-gen colour bulbs).
pub const GAMUT_B: Gamut = Gamut {
    red: XyPoint { x: 0.675, y: 0.322 },
    green: XyPoint { x: 0.409, y: 0.518 },
    blue: XyPoint { x: 0.167, y: 0.040 },
};

/// Philips Hue **Gamut C** (current colour + extended-colour bulbs / `Lightstrip` Plus).
pub const GAMUT_C: Gamut = Gamut {
    red: XyPoint { x: 0.6915, y: 0.3038 },
    green: XyPoint { x: 0.1700, y: 0.7000 },
    blue: XyPoint { x: 0.1532, y: 0.0475 },
};

impl Default for Gamut {
    /// Gamut C is the modern default when a lamp reports no gamut.
    fn default() -> Self {
        GAMUT_C
    }
}

impl From<XyPoint> for ColorPoint {
    fn from(p: XyPoint) -> Self {
        Self {
            x: p.x as f32,
            y: p.y as f32,
        }
    }
}

impl From<ColorPoint> for XyPoint {
    fn from(p: ColorPoint) -> Self {
        Self {
            x: f64::from(p.x),
            y: f64::from(p.y),
        }
    }
}

/// `mired`/`mirek` colour temperature → kelvin.
/// Source: HA `util.color.color_temperature_mired_to_kelvin` (`floor(1e6/m)`).
#[must_use]
pub const fn mirek_to_kelvin(mirek: u16) -> u32 {
    if mirek == 0 {
        return 0;
    }
    1_000_000 / mirek as u32
}

/// Kelvin → `mired`/`mirek`.
/// Source: HA `util.color.color_temperature_kelvin_to_mired` (`floor(1e6/k)`).
#[must_use]
pub const fn kelvin_to_mired(kelvin: u32) -> u16 {
    if kelvin == 0 {
        return 0;
    }
    (1_000_000 / kelvin) as u16
}

/// sRGB gamma-expansion of one 0..=1 channel. Source: HA `color_RGB_to_xy_brightness`.
fn gamma_expand(c: f64) -> f64 {
    if c > 0.040_45 {
        ((c + 0.055) / 1.055).powf(2.4)
    } else {
        c / 12.92
    }
}

/// sRGB gamma-compression of one linear channel. Source: HA `color_xy_brightness_to_RGB`.
fn gamma_compress(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB → CIE `xy` + brightness (0..=255).
/// Source: HA `util.color.color_RGB_to_xy_brightness` (Wide-RGB D65 matrix).
#[must_use]
pub fn rgb_to_xy(r: u8, g: u8, b: u8) -> (f64, f64, u8) {
    if u16::from(r) + u16::from(g) + u16::from(b) == 0 {
        return (0.0, 0.0, 0);
    }
    let rl = gamma_expand(f64::from(r) / 255.0);
    let gl = gamma_expand(f64::from(g) / 255.0);
    let bl = gamma_expand(f64::from(b) / 255.0);

    let big_x = rl * 0.664_511 + gl * 0.154_324 + bl * 0.162_028;
    let big_y = rl * 0.283_881 + gl * 0.668_433 + bl * 0.047_685;
    let big_z = rl * 0.000_088 + gl * 0.072_310 + bl * 0.986_039;

    let sum = big_x + big_y + big_z;
    if sum == 0.0 {
        return (0.0, 0.0, 0);
    }
    let x = big_x / sum;
    let y = big_y / sum;
    let bri = (big_y.min(1.0) * 255.0).round() as u8;
    (round3(x), round3(y), bri)
}

/// sRGB → CIE `xy`, clamped into a lamp's reachable [`Gamut`].
#[must_use]
pub fn rgb_to_xy_in_gamut(r: u8, g: u8, b: u8, gamut: &Gamut) -> (f64, f64, u8) {
    let (x, y, bri) = rgb_to_xy(r, g, b);
    let p = gamut.clamp(XyPoint { x, y });
    (round3(p.x), round3(p.y), bri)
}

/// CIE `xy` + brightness (0..=255) → sRGB.
/// Source: HA `util.color.color_xy_brightness_to_RGB`.
#[must_use]
pub fn xy_to_rgb(x: f64, y: f64, brightness: u8) -> (u8, u8, u8) {
    let bri = f64::from(brightness) / 255.0;
    if bri == 0.0 {
        return (0, 0, 0);
    }
    let vy = if y == 0.0 { 1e-11 } else { y };
    let big_y = bri;
    let big_x = (big_y / vy) * x;
    let big_z = (big_y / vy) * (1.0 - x - vy);

    let mut rl = big_x * 1.656_492 - big_y * 0.354_851 - big_z * 0.255_038;
    let mut gl = -big_x * 0.707_196 + big_y * 1.655_397 + big_z * 0.036_152;
    let mut bl = big_x * 0.051_713 - big_y * 0.121_364 + big_z * 1.011_530;

    rl = gamma_compress(rl).max(0.0);
    gl = gamma_compress(gl).max(0.0);
    bl = gamma_compress(bl).max(0.0);

    let max_c = rl.max(gl).max(bl);
    if max_c > 1.0 {
        rl /= max_c;
        gl /= max_c;
        bl /= max_c;
    }
    (
        (rl * 255.0).round() as u8,
        (gl * 255.0).round() as u8,
        (bl * 255.0).round() as u8,
    )
}

/// sRGB → HSV. Hue in `[0,360)`, saturation + value in `[0,100]`.
/// Source: standard HSV (HA delegates to `colorsys.rgb_to_hsv`).
#[must_use]
pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let rf = f64::from(r) / 255.0;
    let gf = f64::from(g) / 255.0;
    let bf = f64::from(b) / 255.0;
    let max_c = rf.max(gf).max(bf);
    let min_c = rf.min(gf).min(bf);
    let delta = max_c - min_c;

    let mut h = if delta == 0.0 {
        0.0
    } else if (max_c - rf).abs() < f64::EPSILON {
        60.0 * (((gf - bf) / delta).rem_euclid(6.0))
    } else if (max_c - gf).abs() < f64::EPSILON {
        60.0 * ((bf - rf) / delta + 2.0)
    } else {
        60.0 * ((rf - gf) / delta + 4.0)
    };
    if h < 0.0 {
        h += 360.0;
    }
    let s = if max_c == 0.0 { 0.0 } else { delta / max_c };
    (h, s * 100.0, max_c * 100.0)
}

/// HSV (`h` in `[0,360)`, `s`/`v` in `[0,100]`) → sRGB.
/// Source: standard HSV (HA delegates to `colorsys.hsv_to_rgb`).
#[must_use]
pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let s = (s / 100.0).clamp(0.0, 1.0);
    let v = (v / 100.0).clamp(0.0, 1.0);
    let c = v * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp as u8 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (
        ((r1 + m) * 255.0).round() as u8,
        ((g1 + m) * 255.0).round() as u8,
        ((b1 + m) * 255.0).round() as u8,
    )
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn cross(a: XyPoint, b: XyPoint) -> f64 {
    a.x * b.y - a.y * b.x
}

fn distance(a: XyPoint, b: XyPoint) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

/// Closest point to `p` on segment `a`–`b`. Source: HA `get_closest_point_to_line`.
fn closest_on_segment(a: XyPoint, b: XyPoint, p: XyPoint) -> XyPoint {
    let ap = XyPoint {
        x: p.x - a.x,
        y: p.y - a.y,
    };
    let ab = XyPoint {
        x: b.x - a.x,
        y: b.y - a.y,
    };
    let ab2 = ab.x * ab.x + ab.y * ab.y;
    if ab2 == 0.0 {
        return a;
    }
    let t = ((ap.x * ab.x + ap.y * ab.y) / ab2).clamp(0.0, 1.0);
    XyPoint {
        x: a.x + ab.x * t,
        y: a.y + ab.y * t,
    }
}

impl Gamut {
    /// Is `p` inside (or on the edge of) this triangle?
    /// Source: HA `util.color.check_point_in_lamps_reach` (barycentric test).
    #[must_use]
    pub fn contains(&self, p: XyPoint) -> bool {
        const EPS: f64 = 1e-9;
        let v1 = XyPoint {
            x: self.green.x - self.red.x,
            y: self.green.y - self.red.y,
        };
        let v2 = XyPoint {
            x: self.blue.x - self.red.x,
            y: self.blue.y - self.red.y,
        };
        let q = XyPoint {
            x: p.x - self.red.x,
            y: p.y - self.red.y,
        };
        let denom = cross(v1, v2);
        if denom == 0.0 {
            return false;
        }
        let s = cross(q, v2) / denom;
        let t = cross(v1, q) / denom;
        s >= -EPS && t >= -EPS && s + t <= 1.0 + EPS
    }

    /// Clamp `p` to the nearest reachable point inside this triangle.
    /// Source: HA `util.color.get_closest_point_to_point`.
    #[must_use]
    pub fn clamp(&self, p: XyPoint) -> XyPoint {
        if self.contains(p) {
            return p;
        }
        let p_ab = closest_on_segment(self.red, self.green, p);
        let p_ac = closest_on_segment(self.blue, self.red, p);
        let p_bc = closest_on_segment(self.green, self.blue, p);
        let d_ab = distance(p, p_ab);
        let d_ac = distance(p, p_ac);
        let d_bc = distance(p, p_bc);

        let mut best = p_ab;
        let mut lowest = d_ab;
        if d_ac < lowest {
            lowest = d_ac;
            best = p_ac;
        }
        if d_bc < lowest {
            best = p_bc;
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn mirek_kelvin_round_trips() {
        // 153 mirek == 6535 K (Hue's coolest), 500 mirek == 2000 K (warmest).
        assert_eq!(mirek_to_kelvin(153), 6535);
        assert_eq!(mirek_to_kelvin(500), 2000);
        assert_eq!(kelvin_to_mired(6535), 153);
        assert_eq!(kelvin_to_mired(2000), 500);
        // round trip within rounding
        let m = kelvin_to_mired(4000);
        assert!((mirek_to_kelvin(m) as i64 - 4000).abs() <= 20);
    }

    #[test]
    fn pure_red_maps_near_gamut_c_red() {
        let (x, y, bri) = rgb_to_xy(255, 0, 0);
        // Wide-gamut red sits near (0.70, 0.30).
        assert!(approx(x, 0.70, 0.03), "x={x}");
        assert!(approx(y, 0.30, 0.03), "y={y}");
        assert!(bri > 0);
    }

    #[test]
    fn white_maps_near_d65() {
        let (x, y, _bri) = rgb_to_xy(255, 255, 255);
        assert!(approx(x, 0.3227, 0.02), "x={x}");
        assert!(approx(y, 0.3290, 0.02), "y={y}");
    }

    #[test]
    fn xy_to_rgb_recovers_dominant_channel() {
        // Gamut-C green corner -> green should dominate.
        let (r, g, b) = xy_to_rgb(0.17, 0.7, 254);
        assert!(g > r && g > b, "rgb=({r},{g},{b})");
        // Gamut-C blue corner -> blue should dominate.
        let (r, g, b) = xy_to_rgb(0.1532, 0.0475, 254);
        assert!(b > r && b > g, "rgb=({r},{g},{b})");
    }

    #[test]
    fn rgb_xy_round_trip_keeps_hue() {
        let (x, y, bri) = rgb_to_xy(10, 200, 60);
        let (r, g, b) = xy_to_rgb(x, y, bri);
        // green still dominant after a full round trip
        assert!(g > r && g > b, "rgb=({r},{g},{b})");
    }

    #[test]
    fn rgb_to_hsv_primaries() {
        let (h, s, v) = rgb_to_hsv(255, 0, 0);
        assert!(approx(h, 0.0, 0.5), "h={h}");
        assert!(approx(s, 100.0, 0.5), "s={s}");
        assert!(approx(v, 100.0, 0.5), "v={v}");

        let (h, _s, _v) = rgb_to_hsv(0, 255, 0);
        assert!(approx(h, 120.0, 0.5), "h={h}");

        let (h, _s, _v) = rgb_to_hsv(0, 0, 255);
        assert!(approx(h, 240.0, 0.5), "h={h}");
    }

    #[test]
    fn hsv_to_rgb_primaries() {
        assert_eq!(hsv_to_rgb(0.0, 100.0, 100.0), (255, 0, 0));
        assert_eq!(hsv_to_rgb(120.0, 100.0, 100.0), (0, 255, 0));
        assert_eq!(hsv_to_rgb(240.0, 100.0, 100.0), (0, 0, 255));
        assert_eq!(hsv_to_rgb(0.0, 0.0, 100.0), (255, 255, 255));
    }

    #[test]
    fn gamut_c_contains_its_own_corners_and_centre() {
        assert!(GAMUT_C.contains(GAMUT_C.red));
        assert!(GAMUT_C.contains(GAMUT_C.green));
        assert!(GAMUT_C.contains(GAMUT_C.blue));
        // a point in the middle of the triangle
        assert!(GAMUT_C.contains(XyPoint { x: 0.33, y: 0.33 }));
    }

    #[test]
    fn clamp_pulls_outside_point_into_gamut() {
        // Way outside the triangle (saturated spectral green beyond the corner).
        let outside = XyPoint { x: 0.0, y: 1.0 };
        assert!(!GAMUT_C.contains(outside));
        let clamped = GAMUT_C.clamp(outside);
        assert!(
            GAMUT_C.contains(clamped),
            "clamped=({},{}) not in gamut",
            clamped.x,
            clamped.y
        );
    }

    #[test]
    fn clamp_leaves_inside_point_untouched() {
        let inside = XyPoint { x: 0.33, y: 0.33 };
        let clamped = GAMUT_C.clamp(inside);
        assert!(approx(clamped.x, inside.x, 1e-9));
        assert!(approx(clamped.y, inside.y, 1e-9));
    }

    #[test]
    fn color_point_interop() {
        use crate::v2::models::feature::ColorPoint;
        let xy = XyPoint { x: 0.4, y: 0.5 };
        let cp: ColorPoint = xy.into();
        assert!(approx(f64::from(cp.x), 0.4, 1e-6));
        let back: XyPoint = cp.into();
        assert!(approx(back.y, 0.5, 1e-6));
    }
}
