//! Cover device classes and their capability/feature model.
//!
//! A cover's *device class* is what kind of thing it is (a garage door, a
//! blind, an awning…); its *features* are what it can do (set an exact
//! position? tilt its slats? stop mid-travel?). The two are related but not
//! identical — a cheap garage opener is a `Garage` that only knows Open / Close
//! / Stop, while a motorised venetian blind is a `Blind` that supports position
//! *and* tilt. The engine consults the feature set to reject commands a given
//! cover physically cannot honour.

/// What kind of cover this is. Mirrors the Home Assistant `cover` device
/// classes (Apache-2.0 semantics), named in household language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceClass {
    /// A garage door.
    Garage,
    /// A roller / venetian / roman blind.
    Blind,
    /// A roller or pleated shade.
    Shade,
    /// A retractable awning (e.g. over a terrace).
    Awning,
    /// A curtain on a motorised track.
    Curtain,
    /// A roller shutter (rolladen).
    Shutter,
    /// A driveway / garden gate.
    Gate,
    /// A powered door.
    Door,
    /// A powered (e.g. roof / skylight) window.
    Window,
}

/// What a particular cover can do.
///
/// Every cover is assumed to support Open / Close (the bare minimum of the
/// domain). The three flags below are the *optional* capabilities the engine
/// must check before accepting the corresponding command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    /// Can be driven to an exact position (not just fully open / closed).
    pub set_position: bool,
    /// Has tiltable slats with their own 0..=100 axis.
    pub tilt: bool,
    /// Can be halted mid-travel by a Stop command.
    pub stop: bool,
}

impl Features {
    /// The minimal feature set: Open / Close only, no position, tilt or stop.
    /// A bare contact-closure garage relay looks like this.
    #[must_use]
    pub const fn minimal() -> Self {
        Self { set_position: false, tilt: false, stop: false }
    }

    /// Position-capable and stoppable, but no tilt — a roller shade / shutter.
    #[must_use]
    pub const fn positionable() -> Self {
        Self { set_position: true, tilt: false, stop: true }
    }

    /// The full feature set: position, tilt and stop — a motorised venetian
    /// blind.
    #[must_use]
    pub const fn full() -> Self {
        Self { set_position: true, tilt: true, stop: true }
    }
}

impl DeviceClass {
    /// The set of features a typical example of this class ships with. A caller
    /// that knows better (e.g. from device discovery) can override per device;
    /// this is the sensible default.
    #[must_use]
    pub const fn default_features(self) -> Features {
        match self {
            // Most consumer garage openers are open/close/stop with no position
            // feedback.
            Self::Garage | Self::Gate => Features { set_position: false, tilt: false, stop: true },
            // Venetian-style blinds tilt; treat the blind class as fully
            // featured by default.
            Self::Blind => Features::full(),
            // Shades, shutters, curtains, doors and windows position and stop
            // but do not tilt.
            Self::Shade
            | Self::Awning
            | Self::Curtain
            | Self::Shutter
            | Self::Door
            | Self::Window => Features::positionable(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_supports_nothing_optional() {
        let f = Features::minimal();
        assert!(!f.set_position);
        assert!(!f.tilt);
        assert!(!f.stop);
    }

    #[test]
    fn full_supports_everything() {
        let f = Features::full();
        assert!(f.set_position);
        assert!(f.tilt);
        assert!(f.stop);
    }

    #[test]
    fn garage_default_is_open_close_stop_no_position() {
        let f = DeviceClass::Garage.default_features();
        assert!(!f.set_position);
        assert!(!f.tilt);
        assert!(f.stop);
    }

    #[test]
    fn blind_default_supports_tilt() {
        assert!(DeviceClass::Blind.default_features().tilt);
    }

    #[test]
    fn shade_class_positions_but_does_not_tilt() {
        let f = DeviceClass::Shade.default_features();
        assert!(f.set_position);
        assert!(!f.tilt);
        assert!(f.stop);
    }
}
