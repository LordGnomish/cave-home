//! The TV power model.
//!
//! A display is one of three power states. Most control commands only make
//! sense while the TV is fully [`PowerState::On`]; the [`crate::machine`] module
//! gates them accordingly. [`PowerState::Standby`] is the low-power "screen off
//! but listening" state a TV rests in between an explicit power-off and the next
//! wake — the household still says "the TV is off" for both, but the model keeps
//! them apart so a wake can be a fast resume rather than a cold boot.

/// Whether the display is on, fully off, or resting in standby.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    /// Fully off — the screen is dark and only a power-on command is accepted.
    Off,
    /// On and showing a picture — the full command set is available.
    On,
    /// Low-power standby: listening for a wake, but not showing a picture.
    Standby,
}

impl PowerState {
    /// Whether the household would describe the TV as "on" (showing a picture).
    ///
    /// Standby and off both read as "off" to a person in the room.
    #[must_use]
    pub const fn is_on(self) -> bool {
        matches!(self, Self::On)
    }

    /// Whether the TV is awake enough that media commands could run.
    ///
    /// Only [`PowerState::On`] qualifies; standby must wake first.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::On)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_on_is_active() {
        assert!(PowerState::On.is_active());
        assert!(!PowerState::Off.is_active());
        assert!(!PowerState::Standby.is_active());
    }

    #[test]
    fn standby_and_off_read_as_off_to_a_person() {
        assert!(!PowerState::Off.is_on());
        assert!(!PowerState::Standby.is_on());
        assert!(PowerState::On.is_on());
    }
}
