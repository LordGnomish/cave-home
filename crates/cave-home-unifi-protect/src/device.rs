//! The typed UniFi Protect device models.
//!
//! A UniFi Protect site is a set of devices behind one NVR: cameras (some of
//! which are doorbells), motion / contact sensors, smart lights, chimes, and
//! viewers (the TV-wall display devices). This module is the vendor-neutral,
//! transport-free shape of those devices — the part of the public Protect data
//! model the rest of cave-home reasons about. The bootstrap that *fills* these
//! structs from the NVR is the deferred Phase-1b transport (see the crate doc
//! and the parity manifest).
//!
//! Modelled from the public UniFi Protect device taxonomy and the HA
//! `unifiprotect` integration's device classes (Apache-2.0). No GPL source was
//! read.

/// A stable camera identifier. The NVR assigns each camera an opaque id; the
/// id is never shown to the household (Charter §6.3 — they see the friendly
/// name).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CameraId(String);

/// A stable motion / contact / environment sensor identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SensorId(String);

/// A stable smart-light identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LightId(String);

/// A stable chime identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChimeId(String);

/// A stable viewer (display device) identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewerId(String);

macro_rules! id_newtype {
    ($name:ident) => {
        impl $name {
            /// Wrap an opaque NVR id.
            #[must_use]
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            /// The opaque id as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_newtype!(CameraId);
id_newtype!(SensorId);
id_newtype!(LightId);
id_newtype!(ChimeId);
id_newtype!(ViewerId);

/// Whether a device is currently reachable.
///
/// The NVR reports a device as connected or not; cave-home keeps this simple
/// binary because the household only ever needs "the front camera is offline".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// The device is connected and reporting.
    Online,
    /// The device is unreachable.
    Offline,
}

impl DeviceState {
    /// True if the device is online.
    #[must_use]
    pub const fn is_online(self) -> bool {
        matches!(self, Self::Online)
    }
}

/// The recording mode a camera is set to on the NVR.
///
/// This is the camera's *configured* mode (what the NVR is told to do); the
/// moment-to-moment "should we be writing video right now" answer is computed
/// by [`crate::recording::should_record`], which folds in the live detection
/// and schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingMode {
    /// Never record.
    Never,
    /// Always record (continuous).
    Always,
    /// Record when something is detected.
    Detections,
    /// Record only during the camera's recording schedule.
    Schedule,
}

/// A UniFi Protect camera (which may also be a doorbell).
///
/// Feature flags mirror the capabilities a Protect camera advertises in its
/// bootstrap: whether it carries a microphone, a speaker (two-way talk), and
/// whether it is a doorbell (has a button + chime). `is_doorbell` is what the
/// [`crate::event::RingEvent`] path keys on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectCamera {
    /// Stable NVR id.
    pub id: CameraId,
    /// Household-set name, e.g. "Front door".
    pub name: String,
    /// Hardware MAC address as the NVR reports it.
    pub mac: String,
    /// Reachability.
    pub state: DeviceState,
    /// Configured recording mode.
    pub recording_mode: RecordingMode,
    /// Has a microphone.
    pub has_mic: bool,
    /// Has a speaker (two-way talk).
    pub has_speaker: bool,
    /// Is a doorbell (button + chime).
    pub is_doorbell: bool,
    /// The smart-detect types this camera's hardware can recognise.
    pub features: Vec<crate::detect::SmartDetectType>,
}

impl ProtectCamera {
    /// A plain camera: online, recording on detections, no audio, not a
    /// doorbell, no smart-detect features. Builder methods layer on capabilities.
    #[must_use]
    pub fn new(id: CameraId, name: impl Into<String>, mac: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            mac: mac.into(),
            state: DeviceState::Online,
            recording_mode: RecordingMode::Detections,
            has_mic: false,
            has_speaker: false,
            is_doorbell: false,
            features: Vec::new(),
        }
    }

    /// Mark this camera as a doorbell (a button + a two-way intercom).
    #[must_use]
    pub fn as_doorbell(mut self) -> Self {
        self.is_doorbell = true;
        self.has_mic = true;
        self.has_speaker = true;
        self
    }

    /// Set the configured recording mode.
    #[must_use]
    pub fn with_recording_mode(mut self, mode: RecordingMode) -> Self {
        self.recording_mode = mode;
        self
    }

    /// Set the device reachability state.
    #[must_use]
    pub fn with_state(mut self, state: DeviceState) -> Self {
        self.state = state;
        self
    }

    /// Add a smart-detect feature this camera supports.
    #[must_use]
    pub fn with_feature(mut self, feature: crate::detect::SmartDetectType) -> Self {
        if !self.features.contains(&feature) {
            self.features.push(feature);
        }
        self
    }

    /// Whether this camera can recognise the given smart-detect type.
    #[must_use]
    pub fn supports(&self, feature: crate::detect::SmartDetectType) -> bool {
        self.features.contains(&feature)
    }

    /// Whether the camera is online.
    #[must_use]
    pub fn is_online(&self) -> bool {
        self.state.is_online()
    }
}

/// What a [`ProtectSensor`] measures. Protect sensors are multi-function
/// (motion + contact + environment) but the household reasons about one role
/// at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorKind {
    /// Passive-infrared motion.
    Motion,
    /// Door / window open-close contact.
    Contact,
    /// Water-leak.
    Leak,
    /// Temperature.
    Temperature,
    /// Relative humidity.
    Humidity,
}

/// A UniFi Protect sensor device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectSensor {
    /// Stable NVR id.
    pub id: SensorId,
    /// Household-set name.
    pub name: String,
    /// Reachability.
    pub state: DeviceState,
    /// What the sensor measures.
    pub kind: SensorKind,
}

impl ProtectSensor {
    /// An online sensor of the given kind.
    #[must_use]
    pub fn new(id: SensorId, name: impl Into<String>, kind: SensorKind) -> Self {
        Self {
            id,
            name: name.into(),
            state: DeviceState::Online,
            kind,
        }
    }

    /// Whether the sensor is online.
    #[must_use]
    pub fn is_online(&self) -> bool {
        self.state.is_online()
    }
}

/// A UniFi Protect smart light (flood / path light with motion activation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectLight {
    /// Stable NVR id.
    pub id: LightId,
    /// Household-set name.
    pub name: String,
    /// Reachability.
    pub state: DeviceState,
    /// Whether the light is currently on.
    pub is_on: bool,
}

impl ProtectLight {
    /// An online light, switched off.
    #[must_use]
    pub fn new(id: LightId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            state: DeviceState::Online,
            is_on: false,
        }
    }
}

/// A UniFi Protect chime (the speaker that rings on a doorbell press).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectChime {
    /// Stable NVR id.
    pub id: ChimeId,
    /// Household-set name.
    pub name: String,
    /// Reachability.
    pub state: DeviceState,
    /// Ring volume, 0..=100.
    pub volume: u8,
}

impl ProtectChime {
    /// An online chime at the given volume (clamped to 0..=100).
    #[must_use]
    pub fn new(id: ChimeId, name: impl Into<String>, volume: u8) -> Self {
        Self {
            id,
            name: name.into(),
            state: DeviceState::Online,
            volume: volume.min(100),
        }
    }
}

/// A UniFi Protect viewer (a display device that shows a camera live view).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectViewer {
    /// Stable NVR id.
    pub id: ViewerId,
    /// Household-set name.
    pub name: String,
    /// Reachability.
    pub state: DeviceState,
    /// The camera this viewer is currently showing, if any.
    pub showing: Option<CameraId>,
}

impl ProtectViewer {
    /// An online viewer showing nothing yet.
    #[must_use]
    pub fn new(id: ViewerId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            state: DeviceState::Online,
            showing: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::SmartDetectType;

    #[test]
    fn camera_defaults_are_a_plain_online_camera() {
        let cam = ProtectCamera::new(CameraId::new("c1"), "Front door", "aa:bb");
        assert!(cam.is_online());
        assert_eq!(cam.recording_mode, RecordingMode::Detections);
        assert!(!cam.is_doorbell);
        assert!(!cam.has_mic);
        assert!(!cam.has_speaker);
        assert!(cam.features.is_empty());
    }

    #[test]
    fn doorbell_gains_mic_and_speaker() {
        let cam = ProtectCamera::new(CameraId::new("c1"), "Front door", "aa:bb").as_doorbell();
        assert!(cam.is_doorbell);
        assert!(cam.has_mic);
        assert!(cam.has_speaker);
    }

    #[test]
    fn camera_feature_set_is_deduplicated() {
        let cam = ProtectCamera::new(CameraId::new("c1"), "Drive", "aa:bb")
            .with_feature(SmartDetectType::Person)
            .with_feature(SmartDetectType::Person)
            .with_feature(SmartDetectType::Vehicle);
        assert_eq!(cam.features.len(), 2);
        assert!(cam.supports(SmartDetectType::Person));
        assert!(cam.supports(SmartDetectType::Vehicle));
        assert!(!cam.supports(SmartDetectType::Package));
    }

    #[test]
    fn offline_camera_reads_offline() {
        let cam = ProtectCamera::new(CameraId::new("c1"), "Garage", "aa:bb")
            .with_state(DeviceState::Offline);
        assert!(!cam.is_online());
    }

    #[test]
    fn id_newtypes_round_trip_and_display() {
        let id = CameraId::new("64xyz");
        assert_eq!(id.as_str(), "64xyz");
        assert_eq!(id.to_string(), "64xyz");
        assert_eq!(SensorId::new("s").as_str(), "s");
        assert_eq!(LightId::new("l").as_str(), "l");
        assert_eq!(ChimeId::new("h").as_str(), "h");
        assert_eq!(ViewerId::new("v").as_str(), "v");
    }

    #[test]
    fn sensor_carries_its_kind() {
        let s = ProtectSensor::new(SensorId::new("s1"), "Back door", SensorKind::Contact);
        assert_eq!(s.kind, SensorKind::Contact);
        assert!(s.is_online());
    }

    #[test]
    fn light_starts_off() {
        let l = ProtectLight::new(LightId::new("l1"), "Path light");
        assert!(!l.is_on);
    }

    #[test]
    fn chime_volume_is_clamped() {
        assert_eq!(ProtectChime::new(ChimeId::new("h1"), "Hall", 250).volume, 100);
        assert_eq!(ProtectChime::new(ChimeId::new("h1"), "Hall", 40).volume, 40);
    }

    #[test]
    fn viewer_shows_nothing_initially() {
        let v = ProtectViewer::new(ViewerId::new("v1"), "Kitchen TV");
        assert!(v.showing.is_none());
    }

    #[test]
    fn device_state_helpers() {
        assert!(DeviceState::Online.is_online());
        assert!(!DeviceState::Offline.is_online());
    }
}
