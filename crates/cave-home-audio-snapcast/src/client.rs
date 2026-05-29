//! A speaker (Snapcast "client") and its validated volume value object.
//!
//! Modelled from the public Snapcast control-protocol description: a client has
//! a stable id, a human name, a connected flag, a volume percent (0..=100) with
//! a separate mute flag, and a latency in milliseconds the user can trim to
//! line a speaker up with the others. Snapcast source was NOT read.

use crate::sync::LatencyMs;

/// A loudness percentage, constrained to `0..=100`.
///
/// Snapcast carries client volume as a 0..=100 percent plus an independent mute
/// flag (mute is *not* "volume 0"). Wrapping it in a type means every control
/// path is bounds-checked once, here, and the rest of the engine can never hold
/// an out-of-range volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Volume(u8);

impl Volume {
    /// Silent.
    pub const MIN: Self = Self(0);
    /// Full.
    pub const MAX: Self = Self(100);

    /// Construct a volume, rejecting anything above 100.
    ///
    /// # Errors
    /// Returns [`VolumeError::OutOfRange`] if `percent > 100`.
    pub const fn new(percent: u8) -> Result<Self, VolumeError> {
        if percent > 100 {
            Err(VolumeError::OutOfRange)
        } else {
            Ok(Self(percent))
        }
    }

    /// Construct by clamping into `0..=100` (for untrusted device reports).
    #[must_use]
    pub const fn clamped(percent: i32) -> Self {
        if percent < 0 {
            Self::MIN
        } else if percent > 100 {
            Self::MAX
        } else {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Self(percent as u8)
        }
    }

    /// The percentage value.
    #[must_use]
    pub const fn percent(self) -> u8 {
        self.0
    }
}

/// Why a [`Volume`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeError {
    /// The percentage exceeded 100.
    OutOfRange,
}

impl core::fmt::Display for VolumeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfRange => f.write_str("volume percentage must be 0..=100"),
        }
    }
}

impl std::error::Error for VolumeError {}

/// A single speaker on the multi-room network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    id: String,
    name: String,
    connected: bool,
    volume: Volume,
    muted: bool,
    latency: LatencyMs,
}

impl Client {
    /// Create a connected speaker with the given id, name and starting volume.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, volume: Volume) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            connected: true,
            volume,
            muted: false,
            latency: LatencyMs::ZERO,
        }
    }

    /// The stable client id (control-plane internal — never shown to the user).
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The household-facing speaker name ("Kitchen", "Bedroom").
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the speaker is currently reachable.
    #[must_use]
    pub const fn connected(&self) -> bool {
        self.connected
    }

    /// The speaker's volume.
    #[must_use]
    pub const fn volume(&self) -> Volume {
        self.volume
    }

    /// Whether the speaker is muted (independent of its volume).
    #[must_use]
    pub const fn muted(&self) -> bool {
        self.muted
    }

    /// The speaker's latency trim.
    #[must_use]
    pub const fn latency(&self) -> LatencyMs {
        self.latency
    }

    /// The volume actually heard: zero when muted, the set volume otherwise.
    #[must_use]
    pub const fn audible_volume(&self) -> u8 {
        if self.muted {
            0
        } else {
            self.volume.percent()
        }
    }

    // --- pure mutators: these return-by-&mut for use inside control ops; the
    // control layer only ever hands out copies via the topology API, so callers
    // outside the crate cannot mutate a live client behind the engine's back.

    pub(crate) fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub(crate) const fn set_volume(&mut self, v: Volume) {
        self.volume = v;
    }

    pub(crate) const fn set_muted(&mut self, m: bool) {
        self.muted = m;
    }

    pub(crate) const fn set_latency(&mut self, l: LatencyMs) {
        self.latency = l;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn volume_bounds() {
        assert_eq!(Volume::new(0), Ok(Volume::MIN));
        assert_eq!(Volume::new(100), Ok(Volume::MAX));
        assert_eq!(Volume::new(101), Err(VolumeError::OutOfRange));
        assert_eq!(Volume::new(255), Err(VolumeError::OutOfRange));
    }

    #[test]
    fn volume_clamps_untrusted() {
        assert_eq!(Volume::clamped(-5), Volume::MIN);
        assert_eq!(Volume::clamped(50).percent(), 50);
        assert_eq!(Volume::clamped(900), Volume::MAX);
    }

    #[test]
    fn mute_is_not_volume_zero() {
        let mut c = Client::new("c1", "Kitchen", Volume::new(70).expect("vol"));
        c.set_muted(true);
        // The set volume is preserved across a mute…
        assert_eq!(c.volume().percent(), 70);
        // …but nothing is heard while muted.
        assert_eq!(c.audible_volume(), 0);
        c.set_muted(false);
        assert_eq!(c.audible_volume(), 70);
    }

    #[test]
    fn new_client_starts_connected_unmuted_zero_latency() {
        let c = Client::new("c1", "Bedroom", Volume::new(40).expect("vol"));
        assert!(c.connected());
        assert!(!c.muted());
        assert_eq!(c.latency(), LatencyMs::ZERO);
        assert_eq!(c.name(), "Bedroom");
    }
}
