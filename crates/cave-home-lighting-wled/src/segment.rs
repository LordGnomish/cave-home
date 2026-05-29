//! The WLED segment model.
//!
//! A WLED strip is divided into *segments* (`seg` array in the JSON state): a
//! contiguous run of LEDs that share a colour, effect and palette. Each segment
//! carries up to three colour slots (primary / secondary / tertiary, the `col`
//! array), an effect id (`fx`), palette id (`pal`), effect speed (`sx`) and
//! intensity (`ix`), its own brightness (`bri`), an on/off flag (`on`), and the
//! `rev` / `mir` orientation flags.
//!
//! This is a typed model with explicit JSON in/out (see [`crate::state`]); it
//! is implemented from the public WLED JSON API segment description — firmware
//! source was not read (ADR-014 clean-room).

use crate::color::Rgb;

/// One WLED segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Segment index within the strip (`id`).
    pub id: u16,
    /// First LED of the segment, inclusive (`start`).
    pub start: u16,
    /// One past the last LED of the segment (`stop`).
    pub stop: u16,
    /// The three colour slots: primary, secondary, tertiary (`col`).
    pub colors: [Rgb; 3],
    /// Active effect id (`fx`).
    pub effect: u8,
    /// Active palette id (`pal`).
    pub palette: u8,
    /// Effect speed, 0..=255 (`sx`).
    pub speed: u8,
    /// Effect intensity, 0..=255 (`ix`).
    pub intensity: u8,
    /// Per-segment brightness, 0..=255 (`bri`).
    pub brightness: u8,
    /// Whether the segment is lit (`on`).
    pub on: bool,
    /// Play the effect reversed along the segment (`rev`).
    pub reversed: bool,
    /// Mirror the effect about the segment centre (`mir`).
    pub mirror: bool,
}

impl Default for Segment {
    fn default() -> Self {
        Self {
            id: 0,
            start: 0,
            stop: 0,
            colors: [Rgb::WHITE, Rgb::BLACK, Rgb::BLACK],
            effect: 0,
            palette: 0,
            speed: 128,
            intensity: 128,
            brightness: 255,
            on: true,
            reversed: false,
            mirror: false,
        }
    }
}

impl Segment {
    /// Create a segment spanning `[start, stop)` with the given id and a single
    /// solid primary colour. Returns `None` if the LED range is empty or
    /// reversed (`stop <= start`).
    #[must_use]
    pub fn solid(id: u16, start: u16, stop: u16, color: Rgb) -> Option<Self> {
        if stop <= start {
            return None;
        }
        Some(Self {
            id,
            start,
            stop,
            colors: [color, Rgb::BLACK, Rgb::BLACK],
            ..Self::default()
        })
    }

    /// The primary colour slot.
    #[must_use]
    pub const fn primary(&self) -> Rgb {
        self.colors[0]
    }

    /// Number of LEDs the segment covers.
    #[must_use]
    pub const fn len(&self) -> u16 {
        self.stop.saturating_sub(self.start)
    }

    /// `true` if the segment covers no LEDs.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
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
    fn solid_constructor_validates_range() {
        let s = Segment::solid(0, 0, 30, Rgb::new(255, 0, 0)).expect("valid range");
        assert_eq!(s.len(), 30);
        assert_eq!(s.primary(), Rgb::new(255, 0, 0));
        assert!(!s.is_empty());
        assert!(s.on);
        // Empty / reversed ranges are rejected.
        assert!(Segment::solid(0, 30, 30, Rgb::WHITE).is_none());
        assert!(Segment::solid(0, 40, 10, Rgb::WHITE).is_none());
    }

    #[test]
    fn default_segment_is_sane() {
        let s = Segment::default();
        assert!(s.on);
        assert_eq!(s.brightness, 255);
        assert_eq!(s.effect, 0);
        assert_eq!(s.colors[1], Rgb::BLACK);
        assert!(s.is_empty(), "default has no LED span until configured");
    }

    #[test]
    fn len_saturates_on_bad_bounds() {
        let mut s = Segment::default();
        s.start = 50;
        s.stop = 10;
        assert_eq!(s.len(), 0, "len never underflows");
    }
}
