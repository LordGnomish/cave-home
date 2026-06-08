//! Per-panel timing and policy configuration.
//!
//! The alarm panel's behaviour is shaped by a handful of household-set numbers,
//! mirroring the HA `alarm_control_panel` configuration knobs:
//!
//! - **exit delay** — how long, after an arm command, the household has to
//!   leave before the watch begins (the panel sits in `Arming`).
//! - **entry delay** — how long, after a watched sensor trips, the household
//!   has to disarm before the alarm sounds (the panel sits in `Pending`).
//! - **trigger time** — how long the alarm sounds once triggered before it
//!   settles back to its prior armed state.
//! - **arm-requires-code** — whether arming needs a code (disarming always
//!   does).
//! - **home/night instant** — whether the home and night modes skip the entry
//!   delay (an instant interior/perimeter zone) and sound immediately.
//!
//! Times are whole seconds. The caller supplies elapsed time as integer
//! seconds — this crate deliberately depends on no clock or time crate, so the
//! pure state machine is fully testable and deterministic.

/// Elapsed/duration time in whole seconds. The panel never reads a clock; the
/// caller (an adapter, a test) advances time explicitly.
pub type Seconds = u32;

/// Why a [`PanelConfig`] was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// The trigger time was zero — an alarm that sounds for no time is not an
    /// alarm. Safety: refuse the silent-alarm misconfiguration up front.
    ZeroTriggerTime,
    /// A delay exceeded the sane upper bound (`MAX_DELAY`), which usually means
    /// a units mistake (minutes entered as seconds, etc.).
    DelayTooLong,
}

impl core::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ZeroTriggerTime => f.write_str("trigger time must be greater than zero"),
            Self::DelayTooLong => f.write_str("a configured delay is implausibly long"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Validated timing + policy configuration for one alarm panel.
///
/// Construct with [`PanelConfig::new`] (validated) — the fields are private so
/// an out-of-range or silent-alarm configuration can never reach the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelConfig {
    exit_delay: Seconds,
    entry_delay: Seconds,
    trigger_time: Seconds,
    arm_requires_code: bool,
    home_instant: bool,
    night_instant: bool,
    /// If `true`, the alarm stays in `Triggered` after the siren time elapses
    /// (a human must disarm it). If `false`, it auto-returns to the prior armed
    /// state once the siren time is up.
    stay_triggered: bool,
}

impl PanelConfig {
    /// The longest delay we accept for any single phase. One hour is already
    /// far beyond any real exit/entry/siren window; longer is a units error.
    pub const MAX_DELAY: Seconds = 3600;

    /// Validate and construct a panel configuration.
    ///
    /// # Errors
    /// Returns [`ConfigError::ZeroTriggerTime`] if `trigger_time` is zero (a
    /// safety guard against a silent alarm) and [`ConfigError::DelayTooLong`]
    /// if any of the three durations exceeds [`PanelConfig::MAX_DELAY`].
    pub fn new(
        exit_delay: Seconds,
        entry_delay: Seconds,
        trigger_time: Seconds,
        arm_requires_code: bool,
        home_instant: bool,
        night_instant: bool,
        stay_triggered: bool,
    ) -> Result<Self, ConfigError> {
        if trigger_time == 0 {
            return Err(ConfigError::ZeroTriggerTime);
        }
        if exit_delay > Self::MAX_DELAY
            || entry_delay > Self::MAX_DELAY
            || trigger_time > Self::MAX_DELAY
        {
            return Err(ConfigError::DelayTooLong);
        }
        Ok(Self {
            exit_delay,
            entry_delay,
            trigger_time,
            arm_requires_code,
            home_instant,
            night_instant,
            stay_triggered,
        })
    }

    /// A sensible residential default: 60 s to leave, 30 s to disarm on return,
    /// 4-minute siren, code required to arm and disarm, home/night modes use
    /// the entry delay (not instant), and the alarm auto-clears to its armed
    /// state after the siren time.
    #[must_use]
    pub fn residential_default() -> Self {
        // These literals are all within range and trigger_time != 0, so this
        // construction cannot fail; fall back to the strictest config if the
        // invariant is ever broken, so the panel still works.
        Self::new(60, 30, 240, true, false, false, false).unwrap_or(Self {
            exit_delay: 0,
            entry_delay: 0,
            trigger_time: 1,
            arm_requires_code: true,
            home_instant: true,
            night_instant: true,
            stay_triggered: true,
        })
    }

    #[must_use]
    pub const fn exit_delay(&self) -> Seconds {
        self.exit_delay
    }

    #[must_use]
    pub const fn entry_delay(&self) -> Seconds {
        self.entry_delay
    }

    #[must_use]
    pub const fn trigger_time(&self) -> Seconds {
        self.trigger_time
    }

    #[must_use]
    pub const fn arm_requires_code(&self) -> bool {
        self.arm_requires_code
    }

    /// Whether a sensor trip while in the given armed state should sound the
    /// alarm immediately (instant zone) rather than running the entry delay.
    #[must_use]
    pub const fn is_instant_for(&self, instant_home: bool, instant_night: bool) -> bool {
        (instant_home && self.home_instant) || (instant_night && self.night_instant)
    }

    #[must_use]
    pub const fn home_instant(&self) -> bool {
        self.home_instant
    }

    #[must_use]
    pub const fn night_instant(&self) -> bool {
        self.night_instant
    }

    #[must_use]
    pub const fn stay_triggered(&self) -> bool {
        self.stay_triggered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_trigger_time() {
        assert_eq!(
            PanelConfig::new(60, 30, 0, true, false, false, false),
            Err(ConfigError::ZeroTriggerTime)
        );
    }

    #[test]
    fn rejects_implausibly_long_delay() {
        assert_eq!(
            PanelConfig::new(PanelConfig::MAX_DELAY + 1, 30, 240, true, false, false, false),
            Err(ConfigError::DelayTooLong)
        );
        assert_eq!(
            PanelConfig::new(60, PanelConfig::MAX_DELAY + 1, 240, true, false, false, false),
            Err(ConfigError::DelayTooLong)
        );
        assert_eq!(
            PanelConfig::new(60, 30, PanelConfig::MAX_DELAY + 1, true, false, false, false),
            Err(ConfigError::DelayTooLong)
        );
    }

    #[test]
    fn accepts_zero_exit_and_entry_delay() {
        // A 0-second exit/entry delay (arm/trip is instant) is legitimate.
        let cfg = PanelConfig::new(0, 0, 240, false, false, false, false)
            .expect("zero exit/entry with positive trigger is valid");
        assert_eq!(cfg.exit_delay(), 0);
        assert_eq!(cfg.entry_delay(), 0);
    }

    #[test]
    fn residential_default_is_sane() {
        let cfg = PanelConfig::residential_default();
        assert_eq!(cfg.exit_delay(), 60);
        assert_eq!(cfg.entry_delay(), 30);
        assert_eq!(cfg.trigger_time(), 240);
        assert!(cfg.arm_requires_code());
        assert!(!cfg.stay_triggered());
    }

    #[test]
    fn instant_zone_predicate() {
        let cfg = PanelConfig::new(60, 30, 240, true, true, false, false).expect("valid");
        assert!(cfg.is_instant_for(true, false), "home instant zone fires instantly");
        assert!(!cfg.is_instant_for(false, true), "night instant disabled here");
        assert!(!cfg.is_instant_for(false, false));
    }
}
