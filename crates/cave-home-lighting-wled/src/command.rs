//! The validated control layer.
//!
//! Every household action — dim the lights, turn them off, paint a segment a
//! colour, pick the "party" effect, run the nightlight — is a [`Command`].
//! Applying a command is *pure*: it validates its arguments against the
//! documented WLED bounds and returns a brand-new [`State`], leaving the input
//! untouched. Out-of-range arguments are rejected with a [`CommandError`]
//! rather than silently clamped, so a buggy caller is caught, not masked.
//!
//! A separate [`headline`] builder renders any state into the one-line,
//! grandma-friendly EN/DE/TR sentence the Portal and voice replies speak.

use crate::color::Rgb;
use crate::effect::{effect_name, MAX_EFFECT_ID, MAX_PALETTE_ID};
use crate::label::{brightness_percent, colour_word, Lang};
use crate::segment::Segment;
use crate::state::State;

/// Why a command was refused. Carries enough context to explain the rejection
/// without leaking the offending raw value into user-facing text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    /// The named segment index does not exist in the state.
    NoSuchSegment,
    /// The effect id is outside the supported range.
    EffectOutOfRange,
    /// The palette id is outside the supported range.
    PaletteOutOfRange,
    /// The nightlight duration was zero (nothing to time).
    EmptyDuration,
}

impl CommandError {
    /// A short, jargon-free explanation in the requested language.
    #[must_use]
    pub const fn message(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::NoSuchSegment, Lang::En) => "That part of the strip is not set up.",
            (Self::NoSuchSegment, Lang::De) => "Dieser Teil des Lichtbands ist nicht eingerichtet.",
            (Self::NoSuchSegment, Lang::Tr) => "Şeridin o bölümü ayarlı değil.",
            (Self::EffectOutOfRange, Lang::En) => "That effect is not available.",
            (Self::EffectOutOfRange, Lang::De) => "Dieser Effekt ist nicht verfügbar.",
            (Self::EffectOutOfRange, Lang::Tr) => "Bu efekt mevcut değil.",
            (Self::PaletteOutOfRange, Lang::En) => "That colour set is not available.",
            (Self::PaletteOutOfRange, Lang::De) => "Diese Farbpalette ist nicht verfügbar.",
            (Self::PaletteOutOfRange, Lang::Tr) => "Bu renk paleti mevcut değil.",
            (Self::EmptyDuration, Lang::En) => "Set a time longer than zero.",
            (Self::EmptyDuration, Lang::De) => "Eine Zeit länger als null einstellen.",
            (Self::EmptyDuration, Lang::Tr) => "Sıfırdan uzun bir süre seçin.",
        }
    }
}

/// A household lighting action. Brightness is always 0..=255 (a `u8`, so it
/// cannot itself be out of range); other fields are validated on apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Set the master brightness (0..=255).
    SetBrightness(u8),
    /// Flip the master on/off.
    Toggle,
    /// Turn the master on or off explicitly.
    SetPower(bool),
    /// Paint a segment's primary colour.
    SetSegmentColor {
        /// Target segment index.
        segment: u16,
        /// New primary colour.
        color: Rgb,
    },
    /// Select an effect on a segment.
    SetEffect {
        /// Target segment index.
        segment: u16,
        /// WLED effect id.
        effect: u8,
    },
    /// Select a palette on a segment.
    SetPalette {
        /// Target segment index.
        segment: u16,
        /// WLED palette id.
        palette: u8,
    },
    /// Apply a stored preset by id.
    ApplyPreset(u16),
    /// Start the nightlight fade with a duration (minutes) and target
    /// brightness.
    StartNightlight {
        /// Fade duration in minutes (must be > 0).
        duration_min: u16,
        /// Brightness to settle at.
        target_brightness: u8,
    },
}

impl Command {
    /// Apply this command to `state`, returning a new validated state.
    ///
    /// The input state is borrowed and never mutated.
    ///
    /// # Errors
    /// Returns a [`CommandError`] if an argument is out of the documented WLED
    /// range (unknown segment, effect/palette id too large, zero duration).
    pub fn apply(self, state: &State) -> Result<State, CommandError> {
        let mut next = state.clone();
        match self {
            Self::SetBrightness(bri) => {
                next.brightness = bri;
                // Setting a non-zero brightness implies the user wants light on.
                if bri > 0 {
                    next.on = true;
                }
            }
            Self::Toggle => next.on = !next.on,
            Self::SetPower(on) => next.on = on,
            Self::SetSegmentColor { segment, color } => {
                let seg = find_segment_mut(&mut next, segment)?;
                seg.colors[0] = color;
                seg.on = true;
            }
            Self::SetEffect { segment, effect } => {
                if effect > MAX_EFFECT_ID {
                    return Err(CommandError::EffectOutOfRange);
                }
                let seg = find_segment_mut(&mut next, segment)?;
                seg.effect = effect;
            }
            Self::SetPalette { segment, palette } => {
                if palette > MAX_PALETTE_ID {
                    return Err(CommandError::PaletteOutOfRange);
                }
                let seg = find_segment_mut(&mut next, segment)?;
                seg.palette = palette;
            }
            Self::ApplyPreset(id) => {
                next.preset = Some(id);
                next.on = true;
            }
            Self::StartNightlight {
                duration_min,
                target_brightness,
            } => {
                if duration_min == 0 {
                    return Err(CommandError::EmptyDuration);
                }
                next.nightlight.on = true;
                next.nightlight.duration_min = duration_min;
                next.nightlight.target_brightness = target_brightness;
                next.on = true;
            }
        }
        Ok(next)
    }
}

fn find_segment_mut(state: &mut State, id: u16) -> Result<&mut Segment, CommandError> {
    state
        .segments
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or(CommandError::NoSuchSegment)
}

/// Build the one-line, grandma-friendly headline for a state.
///
/// `room` is the household name of the light (e.g. "Living-room"); the caller
/// owns localisation of that label. The colour/effect/brightness wording is
/// localised here.
#[must_use]
pub fn headline(room: &str, state: &State, lang: Lang) -> String {
    if !state.on || state.brightness == 0 {
        return match lang {
            Lang::En => format!("{room} lights off"),
            Lang::De => format!("{room}-Licht aus"),
            Lang::Tr => format!("{room} ışıkları kapalı"),
        };
    }

    let pct = brightness_percent(state.brightness);
    let first = state.segments.first();
    let rgb = first.map_or(Rgb::WHITE, Segment::primary);
    let colour = colour_word(rgb, lang);

    // If an interesting effect is running on the first segment, lead with it.
    let effect_id = first.map_or(0, |s| s.effect);
    if effect_id != 0 {
        let fx = effect_name(effect_id, lang);
        return match lang {
            Lang::En => format!("{room} lights: {fx} at {pct}%"),
            Lang::De => format!("{room}-Licht: {fx} bei {pct}%"),
            Lang::Tr => format!("{room} ışıkları: %{pct} {fx}"),
        };
    }

    match lang {
        Lang::En => format!("{room} lights are {colour} at {pct}%"),
        Lang::De => format!("{room}-Licht ist {colour} bei {pct}%"),
        Lang::Tr => format!("{room} ışıkları %{pct} {colour}"),
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
    use crate::state::Nightlight;

    fn state_with_segments() -> State {
        let mut s = State::default();
        s.brightness = 153; // ~60%
        s.segments = vec![
            Segment::solid(0, 0, 30, Rgb::new(255, 0, 0)).expect("seg"),
            Segment::solid(1, 30, 60, Rgb::new(0, 0, 255)).expect("seg"),
        ];
        s
    }

    #[test]
    fn set_brightness_turns_on_and_is_pure() {
        let s = {
            let mut s = State::default();
            s.on = false;
            s
        };
        let next = Command::SetBrightness(200).apply(&s).expect("apply");
        assert_eq!(next.brightness, 200);
        assert!(next.on, "non-zero brightness implies on");
        assert!(!s.on, "input state must not be mutated");
    }

    #[test]
    fn brightness_zero_does_not_force_on() {
        let next = Command::SetBrightness(0).apply(&State::default()).expect("apply");
        assert_eq!(next.brightness, 0);
    }

    #[test]
    fn toggle_and_setpower() {
        let s = State::default(); // on by default
        let off = Command::Toggle.apply(&s).expect("apply");
        assert!(!off.on);
        let on = Command::SetPower(true).apply(&off).expect("apply");
        assert!(on.on);
    }

    #[test]
    fn set_segment_color_targets_by_id() {
        let s = state_with_segments();
        let next = Command::SetSegmentColor {
            segment: 1,
            color: Rgb::new(0, 255, 0),
        }
        .apply(&s)
        .expect("apply");
        assert_eq!(next.segments[1].primary(), Rgb::new(0, 255, 0));
        assert_eq!(next.segments[0].primary(), Rgb::new(255, 0, 0), "other segment untouched");
    }

    #[test]
    fn unknown_segment_is_rejected() {
        let s = state_with_segments();
        let err = Command::SetSegmentColor {
            segment: 99,
            color: Rgb::WHITE,
        }
        .apply(&s)
        .unwrap_err();
        assert_eq!(err, CommandError::NoSuchSegment);
    }

    #[test]
    fn effect_bounds_are_enforced() {
        let s = state_with_segments();
        // In range: accepted.
        let ok = Command::SetEffect { segment: 0, effect: 73 }.apply(&s).expect("apply");
        assert_eq!(ok.segments[0].effect, 73);
        // Out of range: rejected, not clamped.
        let err = Command::SetEffect {
            segment: 0,
            effect: 255,
        }
        .apply(&s)
        .unwrap_err();
        assert_eq!(err, CommandError::EffectOutOfRange);
    }

    #[test]
    fn palette_bounds_are_enforced() {
        let s = state_with_segments();
        assert!(Command::SetPalette { segment: 0, palette: 6 }.apply(&s).is_ok());
        let err = Command::SetPalette {
            segment: 0,
            palette: 200,
        }
        .apply(&s)
        .unwrap_err();
        assert_eq!(err, CommandError::PaletteOutOfRange);
    }

    #[test]
    fn apply_preset_sets_id_and_power() {
        let mut s = State::default();
        s.on = false;
        let next = Command::ApplyPreset(4).apply(&s).expect("apply");
        assert_eq!(next.preset, Some(4));
        assert!(next.on);
    }

    #[test]
    fn nightlight_requires_nonzero_duration() {
        let s = State::default();
        let err = Command::StartNightlight {
            duration_min: 0,
            target_brightness: 0,
        }
        .apply(&s)
        .unwrap_err();
        assert_eq!(err, CommandError::EmptyDuration);

        let ok = Command::StartNightlight {
            duration_min: 30,
            target_brightness: 5,
        }
        .apply(&s)
        .expect("apply");
        assert_eq!(
            ok.nightlight,
            Nightlight {
                on: true,
                duration_min: 30,
                target_brightness: 5,
                mode: 1,
            }
        );
    }

    #[test]
    fn headline_off_when_powered_down() {
        let mut s = State::default();
        s.on = false;
        assert_eq!(headline("Living-room", &s, Lang::En), "Living-room lights off");
        assert_eq!(headline("Wohnzimmer", &s, Lang::De), "Wohnzimmer-Licht aus");
        assert_eq!(headline("Oturma odası", &s, Lang::Tr), "Oturma odası ışıkları kapalı");
    }

    #[test]
    fn headline_describes_colour_and_brightness() {
        let mut s = State::default();
        s.brightness = 153;
        s.segments = vec![Segment::solid(0, 0, 30, Rgb::new(255, 220, 180)).expect("seg")];
        let line = headline("Living-room", &s, Lang::En);
        assert_eq!(line, "Living-room lights are warm white at 60%");
    }

    #[test]
    fn headline_leads_with_effect_when_running() {
        let mut s = State::default();
        s.brightness = 255;
        let mut seg = Segment::solid(0, 0, 30, Rgb::new(255, 0, 0)).expect("seg");
        seg.effect = 73; // party
        s.segments = vec![seg];
        let line = headline("Kids' room", &s, Lang::En);
        assert!(line.contains("Party"), "effect headline: {line}");
        assert!(line.contains("100%"));
    }

    #[test]
    fn error_messages_are_localised_and_nonempty() {
        for e in [
            CommandError::NoSuchSegment,
            CommandError::EffectOutOfRange,
            CommandError::PaletteOutOfRange,
            CommandError::EmptyDuration,
        ] {
            for l in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!e.message(l).is_empty());
            }
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the UI must never surface protocol/wire terms. These
        // are checked case-insensitively as whole-ish tokens; e.g. the German
        // word "Farbpalette" legitimately contains "pal" but is not jargon.
        const BANNED: &[&str] = &[
            "json", "ddp", "e1.31", "drgb", "udp", "websocket", "mqtt", " fx",
            "fx ", "segment 0", "byte", "0x", "preset slot", "entity_id",
            "wled",
        ];
        let mut s = State::default();
        s.brightness = 153;
        s.segments = vec![Segment::solid(0, 0, 30, Rgb::new(0, 0, 255)).expect("seg")];
        let mut texts = Vec::new();
        for l in [Lang::En, Lang::De, Lang::Tr] {
            texts.push(headline("Living-room", &s, l));
            for e in [
                CommandError::NoSuchSegment,
                CommandError::EffectOutOfRange,
                CommandError::PaletteOutOfRange,
                CommandError::EmptyDuration,
            ] {
                texts.push(e.message(l).to_string());
            }
        }
        for t in &texts {
            let lower = t.to_lowercase();
            for b in BANNED {
                assert!(!lower.contains(b), "UI string leaks jargon {b:?}: {t}");
            }
        }
    }
}
