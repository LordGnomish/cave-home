//! The WLED device `state` object.
//!
//! This is the typed mirror of the WLED JSON API `state` object: the master
//! on/off (`on`), master brightness (`bri`), crossfade transition (`transition`,
//! in 100 ms units), the active preset id (`ps`), the segment list (`seg`), and
//! the nightlight block (`nl`). It round-trips the documented JSON shape via the
//! std-only [`crate::json`] model.
//!
//! Implemented from the public WLED JSON API state description; firmware source
//! was not read (ADR-014 clean-room).

// Every JSON integer is `.clamp()`ed into the destination's range before the
// `as` cast, so the truncation/sign-loss the cast lints warn about is exactly
// the clamping behaviour we want for untrusted device input.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use crate::color::Rgb;
use crate::json::Json;
use crate::segment::Segment;
use std::collections::BTreeMap;

/// The nightlight: a timed fade the household triggers at bedtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nightlight {
    /// Whether the nightlight timer is running (`on`).
    pub on: bool,
    /// Duration of the fade in minutes (`dur`).
    pub duration_min: u16,
    /// Brightness to settle at when the timer elapses, 0..=255 (`tbri`).
    pub target_brightness: u8,
    /// Nightlight mode (`mode`): 0 instant, 1 fade, 2 colour fade, 3 sunrise.
    pub mode: u8,
}

impl Default for Nightlight {
    fn default() -> Self {
        Self {
            on: false,
            duration_min: 60,
            target_brightness: 0,
            mode: 1,
        }
    }
}

/// The full WLED device state cave-home reasons about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// Master on/off for the whole device (`on`).
    pub on: bool,
    /// Master brightness, 0..=255 (`bri`).
    pub brightness: u8,
    /// Crossfade transition time in 100 ms units (`transition`).
    pub transition: u16,
    /// Active preset id, or `None` if no preset is applied (`ps`, -1 → None).
    pub preset: Option<u16>,
    /// The segment list (`seg`).
    pub segments: Vec<Segment>,
    /// The nightlight block (`nl`).
    pub nightlight: Nightlight,
}

impl Default for State {
    fn default() -> Self {
        Self {
            on: true,
            brightness: 128,
            transition: 7,
            preset: None,
            segments: Vec::new(),
            nightlight: Nightlight::default(),
        }
    }
}

impl State {
    /// Encode a colour as a WLED `col` slot: a 3-element integer array.
    fn color_json(c: Rgb) -> Json {
        Json::Arr(vec![
            Json::Int(i64::from(c.r)),
            Json::Int(i64::from(c.g)),
            Json::Int(i64::from(c.b)),
        ])
    }

    /// Decode a WLED `col` slot. Missing channels default to 0; out-of-range
    /// values are clamped into a byte.
    fn color_from_json(v: &Json) -> Rgb {
        let arr = v.as_arr().unwrap_or(&[]);
        let ch = |i: usize| -> u8 {
            arr.get(i)
                .and_then(Json::as_int)
                .unwrap_or(0)
                .clamp(0, 255) as u8
        };
        Rgb::new(ch(0), ch(1), ch(2))
    }

    fn segment_json(seg: &Segment) -> Json {
        let mut m = BTreeMap::new();
        m.insert("id".to_string(), Json::Int(i64::from(seg.id)));
        m.insert("start".to_string(), Json::Int(i64::from(seg.start)));
        m.insert("stop".to_string(), Json::Int(i64::from(seg.stop)));
        m.insert(
            "col".to_string(),
            Json::Arr(seg.colors.iter().map(|c| Self::color_json(*c)).collect()),
        );
        m.insert("fx".to_string(), Json::Int(i64::from(seg.effect)));
        m.insert("pal".to_string(), Json::Int(i64::from(seg.palette)));
        m.insert("sx".to_string(), Json::Int(i64::from(seg.speed)));
        m.insert("ix".to_string(), Json::Int(i64::from(seg.intensity)));
        m.insert("bri".to_string(), Json::Int(i64::from(seg.brightness)));
        m.insert("on".to_string(), Json::Bool(seg.on));
        m.insert("rev".to_string(), Json::Bool(seg.reversed));
        m.insert("mir".to_string(), Json::Bool(seg.mirror));
        Json::Obj(m)
    }

    fn segment_from_json(v: &Json) -> Segment {
        let int = |k: &str, default: i64| v.get(k).and_then(Json::as_int).unwrap_or(default);
        let byte = |k: &str, default: i64| int(k, default).clamp(0, 255) as u8;
        let u16v = |k: &str, default: i64| int(k, default).clamp(0, i64::from(u16::MAX)) as u16;
        let flag = |k: &str, default: bool| v.get(k).and_then(Json::as_bool).unwrap_or(default);

        let mut colors = [Rgb::BLACK; 3];
        if let Some(cols) = v.get("col").and_then(Json::as_arr) {
            for (i, slot) in cols.iter().take(3).enumerate() {
                colors[i] = Self::color_from_json(slot);
            }
        }

        Segment {
            id: u16v("id", 0),
            start: u16v("start", 0),
            stop: u16v("stop", 0),
            colors,
            effect: byte("fx", 0),
            palette: byte("pal", 0),
            speed: byte("sx", 128),
            intensity: byte("ix", 128),
            brightness: byte("bri", 255),
            on: flag("on", true),
            reversed: flag("rev", false),
            mirror: flag("mir", false),
        }
    }

    fn nightlight_json(nl: Nightlight) -> Json {
        let mut m = BTreeMap::new();
        m.insert("on".to_string(), Json::Bool(nl.on));
        m.insert("dur".to_string(), Json::Int(i64::from(nl.duration_min)));
        m.insert("tbri".to_string(), Json::Int(i64::from(nl.target_brightness)));
        m.insert("mode".to_string(), Json::Int(i64::from(nl.mode)));
        Json::Obj(m)
    }

    fn nightlight_from_json(v: &Json) -> Nightlight {
        Nightlight {
            on: v.get("on").and_then(Json::as_bool).unwrap_or(false),
            duration_min: v
                .get("dur")
                .and_then(Json::as_int)
                .unwrap_or(60)
                .clamp(0, i64::from(u16::MAX)) as u16,
            target_brightness: v
                .get("tbri")
                .and_then(Json::as_int)
                .unwrap_or(0)
                .clamp(0, 255) as u8,
            mode: v.get("mode").and_then(Json::as_int).unwrap_or(1).clamp(0, 255) as u8,
        }
    }

    /// Serialize this state to the WLED JSON `state` object string.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut m = BTreeMap::new();
        m.insert("on".to_string(), Json::Bool(self.on));
        m.insert("bri".to_string(), Json::Int(i64::from(self.brightness)));
        m.insert("transition".to_string(), Json::Int(i64::from(self.transition)));
        // WLED uses -1 to mean "no preset".
        m.insert(
            "ps".to_string(),
            Json::Int(self.preset.map_or(-1, i64::from)),
        );
        m.insert("nl".to_string(), Self::nightlight_json(self.nightlight));
        m.insert(
            "seg".to_string(),
            Json::Arr(self.segments.iter().map(Self::segment_json).collect()),
        );
        Json::Obj(m).to_string()
    }

    /// Parse a WLED JSON `state` object string into a [`State`].
    ///
    /// Missing fields take their documented defaults; this is intentionally
    /// lenient (the device may omit unchanged fields).
    ///
    /// # Errors
    /// Returns an error message if the input is not valid JSON or is not an
    /// object.
    pub fn from_json(input: &str) -> Result<Self, String> {
        let v = Json::parse(input)?;
        if !matches!(v, Json::Obj(_)) {
            return Err("WLED state must be a JSON object".to_string());
        }
        let preset = match v.get("ps").and_then(Json::as_int) {
            Some(p) if p >= 0 => Some(p.clamp(0, i64::from(u16::MAX)) as u16),
            _ => None,
        };
        let segments = v
            .get("seg")
            .and_then(Json::as_arr)
            .map(|arr| arr.iter().map(Self::segment_from_json).collect())
            .unwrap_or_default();
        let nightlight = v
            .get("nl")
            .map(Self::nightlight_from_json)
            .unwrap_or_default();

        Ok(Self {
            on: v.get("on").and_then(Json::as_bool).unwrap_or(true),
            brightness: v
                .get("bri")
                .and_then(Json::as_int)
                .unwrap_or(128)
                .clamp(0, 255) as u8,
            transition: v
                .get("transition")
                .and_then(Json::as_int)
                .unwrap_or(7)
                .clamp(0, i64::from(u16::MAX)) as u16,
            preset,
            segments,
            nightlight,
        })
    }

    /// The primary colour of the first segment, if any — the "what colour are
    /// the lights" answer for the headline.
    #[must_use]
    pub fn primary_color(&self) -> Option<Rgb> {
        self.segments.first().map(Segment::primary)
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

    fn sample() -> State {
        let mut s = State::default();
        s.on = true;
        s.brightness = 153;
        s.transition = 10;
        s.preset = Some(3);
        s.nightlight = Nightlight {
            on: true,
            duration_min: 30,
            target_brightness: 0,
            mode: 1,
        };
        s.segments = vec![
            Segment::solid(0, 0, 30, Rgb::new(255, 100, 0)).expect("seg"),
            Segment::solid(1, 30, 60, Rgb::new(0, 0, 255)).expect("seg"),
        ];
        s
    }

    #[test]
    fn default_state_is_sane() {
        let s = State::default();
        assert!(s.on);
        assert_eq!(s.brightness, 128);
        assert_eq!(s.preset, None);
        assert!(s.segments.is_empty());
        assert!(!s.nightlight.on);
    }

    #[test]
    fn json_round_trips() {
        let s = sample();
        let encoded = s.to_json();
        let decoded = State::from_json(&encoded).expect("decode");
        assert_eq!(s, decoded, "state must survive JSON round-trip");
    }

    #[test]
    fn preset_none_encodes_as_minus_one() {
        let mut s = State::default();
        s.preset = None;
        let json = s.to_json();
        assert!(json.contains("\"ps\":-1"), "no-preset encodes as -1: {json}");
        let back = State::from_json(&json).expect("decode");
        assert_eq!(back.preset, None);
    }

    #[test]
    fn parses_partial_state_with_defaults() {
        // The device may send just the fields that changed.
        let s = State::from_json("{\"on\":false,\"bri\":10}").expect("decode");
        assert!(!s.on);
        assert_eq!(s.brightness, 10);
        assert_eq!(s.transition, 7); // default
        assert!(s.segments.is_empty());
    }

    #[test]
    fn rejects_non_object_and_garbage() {
        assert!(State::from_json("[1,2,3]").is_err());
        assert!(State::from_json("not json").is_err());
        assert!(State::from_json("").is_err());
    }

    #[test]
    fn clamps_out_of_range_json_values() {
        // A device claiming bri=9999 must not overflow our u8.
        let s = State::from_json("{\"bri\":9999,\"seg\":[{\"col\":[[999,-5,40]]}]}")
            .expect("decode");
        assert_eq!(s.brightness, 255);
        assert_eq!(s.segments[0].colors[0], Rgb::new(255, 0, 40));
    }

    #[test]
    fn nightlight_round_trips() {
        let mut s = State::default();
        s.nightlight = Nightlight {
            on: true,
            duration_min: 45,
            target_brightness: 5,
            mode: 3,
        };
        let back = State::from_json(&s.to_json()).expect("decode");
        assert_eq!(back.nightlight, s.nightlight);
    }

    #[test]
    fn primary_color_reads_first_segment() {
        let s = sample();
        assert_eq!(s.primary_color(), Some(Rgb::new(255, 100, 0)));
        assert_eq!(State::default().primary_color(), None);
    }
}
