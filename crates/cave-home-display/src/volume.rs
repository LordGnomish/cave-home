//! The volume value object: a validated 0..=100 level, an optional parental
//! maximum cap, and mute state that remembers the level to restore on unmute.
//!
//! All arithmetic is saturating: there is no panic path, and every public
//! constructor either clamps into range or is total. The Home Assistant
//! `media_player` domain models volume as a 0.0..=1.0 float; cave-home uses an
//! integer percentage because that is what a household reads ("the sound is at
//! 30").

/// The highest a volume percentage can go.
pub const MAX_VOLUME: u8 = 100;

/// A TV's sound level, plus mute state and an optional parental maximum.
///
/// The level is always in `0..=cap`, where `cap` is the parental maximum if one
/// is set, otherwise [`MAX_VOLUME`]. Muting hides the level (the TV is silent)
/// but remembers it, so unmute restores exactly what the household last chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume {
    /// The chosen level, already clamped into `0..=effective_cap`.
    level: u8,
    /// `true` while muted; the TV is silent but `level` is preserved.
    muted: bool,
    /// Optional parental maximum (`0..=100`). `None` means no cap.
    cap: Option<u8>,
}

impl Volume {
    /// A new volume at `level`, clamped into range, not muted, no cap.
    ///
    /// Any value above [`MAX_VOLUME`] is clamped down to it.
    #[must_use]
    pub const fn new(level: u8) -> Self {
        Self {
            level: if level > MAX_VOLUME { MAX_VOLUME } else { level },
            muted: false,
            cap: None,
        }
    }

    /// A new volume with a parental maximum applied.
    ///
    /// Both `level` and `cap` are clamped into `0..=100`, and `level` is then
    /// clamped to `cap`.
    #[must_use]
    pub const fn with_cap(level: u8, cap: u8) -> Self {
        let v = Self::new(level);
        v.set_cap(Some(cap))
    }

    /// The current level as a percentage. Note this is the *set* level even when
    /// muted — use [`Self::audible_level`] for what is actually coming out.
    #[must_use]
    pub const fn level(self) -> u8 {
        self.level
    }

    /// Whether the sound is currently muted.
    #[must_use]
    pub const fn is_muted(self) -> bool {
        self.muted
    }

    /// The parental maximum, if one is set.
    #[must_use]
    pub const fn cap(self) -> Option<u8> {
        self.cap
    }

    /// The level the room actually hears: `0` while muted, otherwise the level.
    #[must_use]
    pub const fn audible_level(self) -> u8 {
        if self.muted { 0 } else { self.level }
    }

    /// The effective ceiling the level is held under: the cap if set, else 100.
    #[must_use]
    pub const fn effective_cap(self) -> u8 {
        match self.cap {
            Some(c) => c,
            None => MAX_VOLUME,
        }
    }

    /// Set the parental maximum (or clear it with `None`), re-clamping the level.
    ///
    /// The cap itself is clamped into `0..=100`; the current level is then pulled
    /// down to the new cap if it now exceeds it.
    #[must_use]
    pub const fn set_cap(self, cap: Option<u8>) -> Self {
        let cap = match cap {
            Some(c) if c > MAX_VOLUME => Some(MAX_VOLUME),
            other => other,
        };
        let mut next = self;
        next.cap = cap;
        let ceil = next.effective_cap();
        if next.level > ceil {
            next.level = ceil;
        }
        next
    }

    /// Set the level, clamped into `0..=effective_cap`.
    ///
    /// Setting a level above the parental cap silently clamps to the cap rather
    /// than rejecting — the household asked for "loud", they get "as loud as
    /// allowed". Setting a level does not change mute state.
    #[must_use]
    pub const fn set_level(self, level: u8) -> Self {
        let ceil = self.effective_cap();
        let level = if level > ceil { ceil } else { level };
        let mut next = self;
        next.level = level;
        next
    }

    /// Raise the level by `step`, saturating at the effective cap.
    #[must_use]
    pub const fn volume_up(self, step: u8) -> Self {
        let raised = self.level.saturating_add(step);
        self.set_level(raised)
    }

    /// Lower the level by `step`, saturating at zero.
    #[must_use]
    pub const fn volume_down(self, step: u8) -> Self {
        let lowered = self.level.saturating_sub(step);
        self.set_level(lowered)
    }

    /// Set mute on or off.
    ///
    /// The set level is preserved across muting, so [`Self::set_mute(false)`]
    /// restores exactly the prior level.
    #[must_use]
    pub const fn set_mute(self, muted: bool) -> Self {
        let mut next = self;
        next.muted = muted;
        next
    }

    /// Flip the mute state.
    #[must_use]
    pub const fn toggle_mute(self) -> Self {
        self.set_mute(!self.muted)
    }
}

impl Default for Volume {
    /// A sensible default: a quiet level, unmuted, no cap.
    fn default() -> Self {
        Self::new(20)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clamps_above_max() {
        assert_eq!(Volume::new(200).level(), MAX_VOLUME);
        assert_eq!(Volume::new(100).level(), 100);
        assert_eq!(Volume::new(0).level(), 0);
    }

    #[test]
    fn set_level_clamps_to_cap() {
        let v = Volume::with_cap(10, 40);
        assert_eq!(v.set_level(90).level(), 40, "above-cap request clamps to cap");
        assert_eq!(v.set_level(30).level(), 30, "below-cap request is exact");
        assert_eq!(v.set_level(40).level(), 40, "exactly at cap is allowed");
    }

    #[test]
    fn with_cap_pulls_initial_level_down() {
        assert_eq!(Volume::with_cap(80, 30).level(), 30);
        assert_eq!(Volume::with_cap(20, 30).level(), 20);
    }

    #[test]
    fn setting_a_cap_pulls_current_level_down() {
        let v = Volume::new(70).set_cap(Some(50));
        assert_eq!(v.level(), 50);
        assert_eq!(v.effective_cap(), 50);
    }

    #[test]
    fn clearing_cap_restores_full_ceiling_but_not_lost_level() {
        // Capping to 50 lowers a level of 70 to 50; clearing the cap does NOT
        // bring 70 back — that information is gone, which is the honest model.
        let v = Volume::new(70).set_cap(Some(50)).set_cap(None);
        assert_eq!(v.effective_cap(), MAX_VOLUME);
        assert_eq!(v.level(), 50);
    }

    #[test]
    fn cap_itself_is_clamped() {
        assert_eq!(Volume::with_cap(50, 200).effective_cap(), MAX_VOLUME);
    }

    #[test]
    fn volume_up_saturates_at_cap() {
        let v = Volume::with_cap(38, 40);
        assert_eq!(v.volume_up(5).level(), 40);
        assert_eq!(v.volume_up(1).level(), 39);
    }

    #[test]
    fn volume_down_saturates_at_zero() {
        let v = Volume::new(3);
        assert_eq!(v.volume_down(5).level(), 0);
        assert_eq!(v.volume_down(1).level(), 2);
    }

    #[test]
    fn volume_up_does_not_overflow_at_255() {
        let v = Volume::new(100);
        assert_eq!(v.volume_up(255).level(), 100);
    }

    #[test]
    fn mute_preserves_level_and_unmute_restores_it() {
        let v = Volume::new(45);
        let muted = v.set_mute(true);
        assert!(muted.is_muted());
        assert_eq!(muted.level(), 45, "set level is preserved while muted");
        assert_eq!(muted.audible_level(), 0, "but nothing is heard");
        let unmuted = muted.set_mute(false);
        assert!(!unmuted.is_muted());
        assert_eq!(unmuted.audible_level(), 45, "unmute restores exactly");
    }

    #[test]
    fn toggle_mute_round_trips() {
        let v = Volume::new(30);
        assert!(v.toggle_mute().is_muted());
        assert!(!v.toggle_mute().toggle_mute().is_muted());
    }

    #[test]
    fn audible_level_is_zero_only_while_muted() {
        assert_eq!(Volume::new(60).audible_level(), 60);
        assert_eq!(Volume::new(60).set_mute(true).audible_level(), 0);
    }
}
