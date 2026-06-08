//! The chime model: which indoor tone a door event plays, and whether it
//! should sound at all once do-not-disturb and quiet hours are applied.
//!
//! This is pure policy. It plays no audio and reads no clock: the caller passes
//! in the current hour as a whole number 0..=23 and the engine returns a
//! [`ChimeDecision`]. The actual indoor-speaker output is a Phase-1b adapter
//! (see `parity.manifest.toml`, ADR-018).

use crate::event::DoorbellEvent;

/// The indoor tone played for a door event. Names are household-friendly; the
/// mapping to a real speaker sample is a Phase-1b adapter concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChimeTone {
    /// The familiar two-note "ding-dong" — the default for a button press.
    DingDong,
    /// A single soft note — used for a motion alert so it reads as gentler than
    /// a real press.
    SoftPing,
    /// No tone at all (the event is suppressed).
    Silent,
}

/// An hour of the day, 0..=23. Validated so quiet-hours arithmetic never has to
/// defend against out-of-range input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hour(u8);

/// Why an [`Hour`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HourError {
    /// The hour was not in 0..=23.
    OutOfRange,
}

impl core::fmt::Display for HourError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfRange => f.write_str("hour of day must be between 0 and 23"),
        }
    }
}

impl std::error::Error for HourError {}

impl Hour {
    /// Construct a validated hour.
    ///
    /// # Errors
    /// [`HourError::OutOfRange`] if `hour` is not in 0..=23.
    pub const fn new(hour: u8) -> Result<Self, HourError> {
        if hour <= 23 {
            Ok(Self(hour))
        } else {
            Err(HourError::OutOfRange)
        }
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// The chime policy a household configures.
///
/// `quiet_hours` is an optional `[start, end)` window of whole hours that may
/// wrap past midnight (e.g. `22..7` means 22:00 through 06:59). During quiet
/// hours a button press still chimes (you do not want to miss a real visitor)
/// but motion alerts are silenced. Do-not-disturb, when on, silences
/// everything regardless of the hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChimePolicy {
    /// Master switch: when `false` the door never chimes indoors.
    enabled: bool,
    /// When `true`, suppress every chime regardless of hour.
    do_not_disturb: bool,
    /// Per-event toggle for the gentler motion chime.
    chime_on_motion: bool,
    /// Optional wrap-capable quiet-hours window `[start, end)`.
    quiet_start: Option<Hour>,
    quiet_end: Option<Hour>,
}

impl Default for ChimePolicy {
    /// A sensible household default: chiming on, no do-not-disturb, motion
    /// chimes on, no quiet hours.
    fn default() -> Self {
        Self {
            enabled: true,
            do_not_disturb: false,
            chime_on_motion: true,
            quiet_start: None,
            quiet_end: None,
        }
    }
}

/// The outcome of evaluating the chime policy for one event at one hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChimeDecision {
    /// Whether to play a tone indoors at all.
    pub should_chime: bool,
    /// Which tone to play. [`ChimeTone::Silent`] iff `should_chime` is `false`.
    pub tone: ChimeTone,
    /// Why the decision came out the way it did (for the visitor log / debug
    /// surface — never shown raw to the household).
    pub reason: ChimeReason,
}

/// The single deciding factor behind a [`ChimeDecision`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChimeReason {
    /// Chiming is disabled at the master switch.
    ChimingDisabled,
    /// Do-not-disturb is on.
    DoNotDisturb,
    /// Inside the quiet-hours window (motion is hushed; presses still ring).
    QuietHours,
    /// The motion chime is switched off for this household.
    MotionChimeOff,
    /// This event does not produce a chime (a household action, not a door
    /// signal).
    NotAChimingEvent,
    /// A normal audible chime.
    Audible,
}

impl ChimePolicy {
    /// Build a policy.
    #[must_use]
    pub const fn new(
        enabled: bool,
        do_not_disturb: bool,
        chime_on_motion: bool,
        quiet_start: Option<Hour>,
        quiet_end: Option<Hour>,
    ) -> Self {
        Self { enabled, do_not_disturb, chime_on_motion, quiet_start, quiet_end }
    }

    /// Whether `hour` falls inside the configured quiet-hours window. A window
    /// whose start equals its end is treated as empty (never quiet), matching
    /// the `[start, end)` half-open convention. Windows where `start > end`
    /// wrap past midnight.
    #[must_use]
    pub fn in_quiet_hours(&self, hour: Hour) -> bool {
        match (self.quiet_start, self.quiet_end) {
            (Some(start), Some(end)) => {
                let (s, e, h) = (start.get(), end.get(), hour.get());
                if s == e {
                    false
                } else if s < e {
                    // Same-day window, e.g. 1..6.
                    h >= s && h < e
                } else {
                    // Wraps midnight, e.g. 22..7 -> [22,23] ∪ [0,6].
                    h >= s || h < e
                }
            }
            _ => false,
        }
    }

    /// Decide whether — and how — to chime for `event` at `hour`.
    ///
    /// Precedence (first match wins): master switch off → do-not-disturb →
    /// the event is not a chiming event → motion-specific gates (per-event
    /// toggle, then quiet hours) → audible.
    #[must_use]
    pub fn decide(&self, event: DoorbellEvent, hour: Hour) -> ChimeDecision {
        if !self.enabled {
            return Self::silent(ChimeReason::ChimingDisabled);
        }
        if self.do_not_disturb {
            return Self::silent(ChimeReason::DoNotDisturb);
        }
        match event {
            DoorbellEvent::ButtonPressed => ChimeDecision {
                should_chime: true,
                tone: ChimeTone::DingDong,
                reason: ChimeReason::Audible,
            },
            DoorbellEvent::MotionDetected => {
                if !self.chime_on_motion {
                    Self::silent(ChimeReason::MotionChimeOff)
                } else if self.in_quiet_hours(hour) {
                    // A real visitor pressing the button still rings at night;
                    // mere motion does not wake the house.
                    Self::silent(ChimeReason::QuietHours)
                } else {
                    ChimeDecision {
                        should_chime: true,
                        tone: ChimeTone::SoftPing,
                        reason: ChimeReason::Audible,
                    }
                }
            }
            // Household actions and the timeout signal are not chiming events.
            DoorbellEvent::CallAnswered
            | DoorbellEvent::CallDeclined
            | DoorbellEvent::CallEnded
            | DoorbellEvent::VisitorTimeout => Self::silent(ChimeReason::NotAChimingEvent),
        }
    }

    const fn silent(reason: ChimeReason) -> ChimeDecision {
        ChimeDecision { should_chime: false, tone: ChimeTone::Silent, reason }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hr(h: u8) -> Hour {
        Hour::new(h).expect("valid test hour")
    }

    #[test]
    fn hour_rejects_out_of_range() {
        assert!(Hour::new(0).is_ok());
        assert!(Hour::new(23).is_ok());
        assert_eq!(Hour::new(24), Err(HourError::OutOfRange));
        assert_eq!(Hour::new(255), Err(HourError::OutOfRange));
    }

    #[test]
    fn press_chimes_ding_dong_by_default() {
        let p = ChimePolicy::default();
        let d = p.decide(DoorbellEvent::ButtonPressed, hr(14));
        assert!(d.should_chime);
        assert_eq!(d.tone, ChimeTone::DingDong);
        assert_eq!(d.reason, ChimeReason::Audible);
    }

    #[test]
    fn motion_chimes_soft_ping_by_default() {
        let p = ChimePolicy::default();
        let d = p.decide(DoorbellEvent::MotionDetected, hr(14));
        assert!(d.should_chime);
        assert_eq!(d.tone, ChimeTone::SoftPing);
    }

    #[test]
    fn master_switch_off_silences_everything() {
        let p = ChimePolicy::new(false, false, true, None, None);
        let d = p.decide(DoorbellEvent::ButtonPressed, hr(14));
        assert!(!d.should_chime);
        assert_eq!(d.tone, ChimeTone::Silent);
        assert_eq!(d.reason, ChimeReason::ChimingDisabled);
    }

    #[test]
    fn do_not_disturb_silences_even_a_press() {
        let p = ChimePolicy::new(true, true, true, None, None);
        let d = p.decide(DoorbellEvent::ButtonPressed, hr(14));
        assert!(!d.should_chime);
        assert_eq!(d.reason, ChimeReason::DoNotDisturb);
    }

    #[test]
    fn motion_chime_can_be_turned_off_per_event() {
        let p = ChimePolicy::new(true, false, false, None, None);
        let motion = p.decide(DoorbellEvent::MotionDetected, hr(14));
        assert!(!motion.should_chime);
        assert_eq!(motion.reason, ChimeReason::MotionChimeOff);
        // A press is unaffected by the motion toggle.
        let press = p.decide(DoorbellEvent::ButtonPressed, hr(14));
        assert!(press.should_chime);
    }

    #[test]
    fn quiet_hours_same_day_window_hushes_motion_but_not_press() {
        // Quiet 1..6 (01:00..05:59).
        let p = ChimePolicy::new(true, false, true, Some(hr(1)), Some(hr(6)));
        assert!(p.in_quiet_hours(hr(1)));
        assert!(p.in_quiet_hours(hr(5)));
        assert!(!p.in_quiet_hours(hr(6)), "end is exclusive");
        assert!(!p.in_quiet_hours(hr(0)));

        let motion = p.decide(DoorbellEvent::MotionDetected, hr(3));
        assert!(!motion.should_chime);
        assert_eq!(motion.reason, ChimeReason::QuietHours);

        let press = p.decide(DoorbellEvent::ButtonPressed, hr(3));
        assert!(press.should_chime, "a real visitor still rings at night");
    }

    #[test]
    fn quiet_hours_wrapping_midnight() {
        // Quiet 22..7 (22:00 through 06:59), wrapping midnight.
        let p = ChimePolicy::new(true, false, true, Some(hr(22)), Some(hr(7)));
        assert!(p.in_quiet_hours(hr(22)));
        assert!(p.in_quiet_hours(hr(23)));
        assert!(p.in_quiet_hours(hr(0)));
        assert!(p.in_quiet_hours(hr(6)));
        assert!(!p.in_quiet_hours(hr(7)), "end is exclusive");
        assert!(!p.in_quiet_hours(hr(12)));
        assert!(!p.in_quiet_hours(hr(21)));
    }

    #[test]
    fn quiet_hours_boundary_just_outside_window_chimes_motion() {
        let p = ChimePolicy::new(true, false, true, Some(hr(22)), Some(hr(7)));
        // 07:00 is the first hour back out of quiet hours.
        let motion = p.decide(DoorbellEvent::MotionDetected, hr(7));
        assert!(motion.should_chime);
        assert_eq!(motion.tone, ChimeTone::SoftPing);
    }

    #[test]
    fn empty_quiet_window_is_never_quiet() {
        // start == end -> empty window.
        let p = ChimePolicy::new(true, false, true, Some(hr(3)), Some(hr(3)));
        assert!(!p.in_quiet_hours(hr(3)));
        assert!(!p.in_quiet_hours(hr(0)));
    }

    #[test]
    fn household_actions_never_chime() {
        let p = ChimePolicy::default();
        for ev in [
            DoorbellEvent::CallAnswered,
            DoorbellEvent::CallDeclined,
            DoorbellEvent::CallEnded,
            DoorbellEvent::VisitorTimeout,
        ] {
            let d = p.decide(ev, hr(14));
            assert!(!d.should_chime);
            assert_eq!(d.reason, ChimeReason::NotAChimingEvent);
        }
    }
}
