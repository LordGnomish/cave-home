//! Colour model for WLED lights.
//!
//! WLED's JSON API expresses a segment's colour as an array of RGB (or RGBW)
//! byte triplets/quadruplets, plus a master brightness byte (`bri`, 0..=255).
//! This module is the colour brain: the [`Rgb`] / [`Rgbw`] types, HSV↔RGB
//! conversion (so the household "make it a warmer red" gestures map to bytes),
//! and a Kelvin colour-temperature → RGB approximation for the "warm white" /
//! "cool white" presets.
//!
//! All maths is integer-clamped and total — no value can produce a panic, a
//! `NaN`, or an out-of-range byte. Implemented from the public WLED JSON API
//! colour description; WLED firmware source was not read (ADR-014 clean-room).

// Every float→integer cast in this module is preceded by an explicit
// `.clamp(0.0, 255.0)` (or clamp into 0..=1, then ×255), so truncation and
// sign-loss are intended and cannot misbehave. The clamp is the safety; the
// cast is the representation.
// The HSV↔RGB conversions use the conventional single-letter channel/temporary
// names (r/g/b/h/s/v/p/q/t) from the textbook algorithm; longer names would
// hurt, not help, readability here.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::many_single_char_names
)]

/// A single packed RGB colour, one byte per channel, exactly as a WLED segment
/// colour slot carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgb {
    /// Red channel, 0..=255.
    pub r: u8,
    /// Green channel, 0..=255.
    pub g: u8,
    /// Blue channel, 0..=255.
    pub b: u8,
}

impl Rgb {
    /// Pure black / off.
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };
    /// Full white (all channels max).
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255 };

    /// Construct from three channel bytes.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// `true` if every channel is zero (the light shows nothing).
    #[must_use]
    pub const fn is_black(self) -> bool {
        self.r == 0 && self.g == 0 && self.b == 0
    }
}

/// An RGBW colour: RGB plus a dedicated white channel, used by SK6812-RGBW
/// strips where a real white LED gives a cleaner white than mixing R+G+B.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgbw {
    /// Red channel, 0..=255.
    pub r: u8,
    /// Green channel, 0..=255.
    pub g: u8,
    /// Blue channel, 0..=255.
    pub b: u8,
    /// Dedicated white channel, 0..=255.
    pub w: u8,
}

impl Rgbw {
    /// Construct from four channel bytes.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, w: u8) -> Self {
        Self { r, g, b, w }
    }

    /// The RGB part, dropping the white channel.
    #[must_use]
    pub const fn rgb(self) -> Rgb {
        Rgb::new(self.r, self.g, self.b)
    }
}

impl From<Rgb> for Rgbw {
    fn from(c: Rgb) -> Self {
        Self::new(c.r, c.g, c.b, 0)
    }
}

/// A hue/saturation/value colour.
///
/// Hue is 0..360 degrees (wrapping), saturation and value are 0.0..=1.0. This
/// is the gesture-friendly space — "a bit warmer", "less saturated" — that the
/// UI works in before committing to the byte-packed [`Rgb`] WLED wants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsv {
    /// Hue in degrees, normalised to 0.0..360.0.
    pub h: f64,
    /// Saturation, clamped to 0.0..=1.0.
    pub s: f64,
    /// Value (brightness of the colour itself), clamped to 0.0..=1.0.
    pub v: f64,
}

impl Hsv {
    /// Construct an HSV colour, normalising hue into 0..360 and clamping S/V
    /// into 0.0..=1.0 so downstream maths is always well-defined.
    #[must_use]
    pub fn new(h: f64, s: f64, v: f64) -> Self {
        let h = if h.is_finite() {
            h.rem_euclid(360.0)
        } else {
            0.0
        };
        Self {
            h,
            s: clamp_unit(s),
            v: clamp_unit(v),
        }
    }
}

/// Clamp a possibly-NaN float into the closed unit interval.
const fn clamp_unit(x: f64) -> f64 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(0.0, 1.0)
    }
}

/// Round a unit-interval float to a 0..=255 byte.
fn unit_to_byte(x: f64) -> u8 {
    let scaled = (clamp_unit(x) * 255.0).round();
    // round() of a clamped 0..=1 value is always in 0..=255.
    scaled as u8
}

impl From<Hsv> for Rgb {
    /// Convert HSV → RGB using the standard sextant decomposition.
    fn from(hsv: Hsv) -> Self {
        let Hsv { h, s, v } = hsv;
        if s <= 0.0 {
            let g = unit_to_byte(v);
            return Self::new(g, g, g);
        }
        let sector = h / 60.0;
        let i = sector.floor();
        let f = sector - i;
        let p = v * (1.0 - s);
        let q = v * s.mul_add(-f, 1.0);
        let t = v * s.mul_add(-(1.0 - f), 1.0);
        // i is in 0..6 because h is normalised to 0..360.
        let (rf, gf, bf) = match i as i64 % 6 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };
        Self::new(unit_to_byte(rf), unit_to_byte(gf), unit_to_byte(bf))
    }
}

impl From<Rgb> for Hsv {
    /// Convert RGB → HSV. The inverse of [`From<Hsv> for Rgb`] up to byte
    /// rounding.
    fn from(rgb: Rgb) -> Self {
        let r = f64::from(rgb.r) / 255.0;
        let g = f64::from(rgb.g) / 255.0;
        let b = f64::from(rgb.b) / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let v = max;
        let s = if max <= 0.0 { 0.0 } else { delta / max };

        let h = if delta <= 0.0 {
            0.0
        } else if (max - r).abs() < f64::EPSILON {
            60.0 * (((g - b) / delta).rem_euclid(6.0))
        } else if (max - g).abs() < f64::EPSILON {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };

        Self::new(h, s, v)
    }
}

/// Approximate the RGB rendering of a black-body colour temperature in Kelvin.
///
/// This is the "warm white" (≈2700 K) ↔ "daylight / cool white" (≈6500 K)
/// control the household actually reaches for. Uses Tanner Helland's published
/// polynomial approximation (valid roughly 1000 K..=40000 K); the input is
/// clamped to that range so the result is always a sensible white-ish colour.
#[must_use]
pub fn kelvin_to_rgb(kelvin: u32) -> Rgb {
    let k = f64::from(kelvin.clamp(1000, 40000)) / 100.0;

    let red = if k <= 66.0 {
        255.0
    } else {
        329.698_727_446 * (k - 60.0).powf(-0.133_204_759_2)
    };

    let green = if k <= 66.0 {
        99.470_802_586_1f64.mul_add(k.ln(), -161.119_568_166_1)
    } else {
        288.122_169_528_3 * (k - 60.0).powf(-0.075_514_849_2)
    };

    let blue = if k >= 66.0 {
        255.0
    } else if k <= 19.0 {
        0.0
    } else {
        138.517_731_223_1f64.mul_add((k - 10.0).ln(), -305.044_792_730_7)
    };

    Rgb::new(clamp_to_byte(red), clamp_to_byte(green), clamp_to_byte(blue))
}

/// Clamp a possibly-out-of-range / NaN float to a 0..=255 byte.
fn clamp_to_byte(x: f64) -> u8 {
    if x.is_nan() {
        0
    } else {
        x.clamp(0.0, 255.0).round() as u8
    }
}

#[cfg(test)]
mod tests {
    // Tests legitimately use expect/unwrap on known-good inputs and the
    // `let mut s = Default; s.field = ..` setup shape; these patterns are fine
    // in test scaffolding even though clippy::pedantic flags them in shipped code.
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::field_reassign_with_default,
        clippy::uninlined_format_args,
        clippy::float_cmp
    )]
    use super::*;

    #[test]
    fn rgb_constants_and_helpers() {
        assert!(Rgb::BLACK.is_black());
        assert!(!Rgb::WHITE.is_black());
        assert_eq!(Rgb::WHITE, Rgb::new(255, 255, 255));
    }

    #[test]
    fn rgbw_drops_and_lifts_white() {
        let w = Rgbw::new(10, 20, 30, 200);
        assert_eq!(w.rgb(), Rgb::new(10, 20, 30));
        let lifted: Rgbw = Rgb::new(1, 2, 3).into();
        assert_eq!(lifted, Rgbw::new(1, 2, 3, 0));
    }

    #[test]
    fn hsv_normalises_hue_and_clamps() {
        let a = Hsv::new(400.0, 2.0, -1.0);
        assert!((a.h - 40.0).abs() < 1e-9);
        assert_eq!(a.s, 1.0);
        assert_eq!(a.v, 0.0);
        // NaN inputs degrade gracefully rather than poisoning the colour.
        let b = Hsv::new(f64::NAN, f64::NAN, f64::NAN);
        assert_eq!((b.h, b.s, b.v), (0.0, 0.0, 0.0));
    }

    #[test]
    fn hsv_to_rgb_known_primaries() {
        // Pure red / green / blue at full value+saturation.
        assert_eq!(Rgb::from(Hsv::new(0.0, 1.0, 1.0)), Rgb::new(255, 0, 0));
        assert_eq!(Rgb::from(Hsv::new(120.0, 1.0, 1.0)), Rgb::new(0, 255, 0));
        assert_eq!(Rgb::from(Hsv::new(240.0, 1.0, 1.0)), Rgb::new(0, 0, 255));
    }

    #[test]
    fn hsv_to_rgb_secondaries_and_white() {
        assert_eq!(Rgb::from(Hsv::new(60.0, 1.0, 1.0)), Rgb::new(255, 255, 0)); // yellow
        assert_eq!(Rgb::from(Hsv::new(180.0, 1.0, 1.0)), Rgb::new(0, 255, 255)); // cyan
        assert_eq!(Rgb::from(Hsv::new(300.0, 1.0, 1.0)), Rgb::new(255, 0, 255)); // magenta
        assert_eq!(Rgb::from(Hsv::new(0.0, 0.0, 1.0)), Rgb::WHITE); // white
        assert_eq!(Rgb::from(Hsv::new(0.0, 0.0, 0.0)), Rgb::BLACK); // off
    }

    #[test]
    fn rgb_to_hsv_known_values() {
        let red: Hsv = Rgb::new(255, 0, 0).into();
        assert!((red.h - 0.0).abs() < 1e-9);
        assert!((red.s - 1.0).abs() < 1e-9);
        assert!((red.v - 1.0).abs() < 1e-9);

        let green: Hsv = Rgb::new(0, 255, 0).into();
        assert!((green.h - 120.0).abs() < 1e-9);

        let blue: Hsv = Rgb::new(0, 0, 255).into();
        assert!((blue.h - 240.0).abs() < 1e-9);

        let gray: Hsv = Rgb::new(128, 128, 128).into();
        assert!(gray.s.abs() < 1e-9, "grey has zero saturation");
    }

    #[test]
    fn hsv_rgb_round_trip_is_stable() {
        // Convert RGB -> HSV -> RGB and require it to land back within ±1 byte.
        for c in [
            Rgb::new(255, 0, 0),
            Rgb::new(0, 255, 0),
            Rgb::new(0, 0, 255),
            Rgb::new(123, 200, 47),
            Rgb::new(17, 17, 240),
            Rgb::new(200, 100, 50),
            Rgb::WHITE,
            Rgb::BLACK,
        ] {
            let hsv: Hsv = c.into();
            let back: Rgb = hsv.into();
            assert!(
                (i32::from(back.r) - i32::from(c.r)).abs() <= 1
                    && (i32::from(back.g) - i32::from(c.g)).abs() <= 1
                    && (i32::from(back.b) - i32::from(c.b)).abs() <= 1,
                "round trip drifted: {c:?} -> {hsv:?} -> {back:?}"
            );
        }
    }

    #[test]
    fn kelvin_warm_is_reddish_cool_is_bluish() {
        let warm = kelvin_to_rgb(2700); // candle / warm white
        let cool = kelvin_to_rgb(6500); // daylight
        assert!(warm.r >= warm.b, "warm white should not be blue-dominant");
        assert!(
            cool.b >= warm.b,
            "cooler temperature should carry more blue than warmer"
        );
        // Both are recognisably white-ish: every channel meaningfully lit.
        assert!(warm.r > 200);
        assert!(cool.r > 200 && cool.g > 200 && cool.b > 200);
    }

    #[test]
    fn kelvin_clamps_extreme_inputs() {
        // Below/above the valid range must still yield a valid byte colour,
        // never a panic or a wild value.
        let lo = kelvin_to_rgb(0);
        let hi = kelvin_to_rgb(1_000_000);
        assert_eq!(lo, kelvin_to_rgb(1000));
        assert_eq!(hi, kelvin_to_rgb(40000));
    }

    #[test]
    fn kelvin_red_channel_is_always_full_in_warm_range() {
        for k in [1000, 1500, 2000, 2700, 3500, 5000, 6600] {
            assert_eq!(kelvin_to_rgb(k).r, 255, "red saturates across warm whites at {k}K");
        }
    }
}
