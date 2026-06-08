//! The snapshot / clip request model.
//!
//! When the door rings or sees motion, cave-home wants a picture of who is
//! there. This module decides *what* to ask the camera pillar for — a single
//! still snapshot or a short video clip — and *why*. It does **not** capture
//! anything: the actual frame grab over the camera's stream is the camera
//! pillar's job and is a Phase-1b adapter (see `parity.manifest.toml`,
//! ADR-018). This crate only models the request, keeping the doorbell pillar
//! decoupled from the camera transport entirely.

use crate::event::DoorbellEvent;

/// What kind of media to request from the camera pillar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    /// A single still image — enough to see who is at the door.
    Snapshot,
    /// A short video clip — captures movement, useful for a visitor passing by
    /// or to review what happened around a missed call.
    Clip,
}

/// Why a media request was raised. Drives the camera pillar's framing and the
/// visitor-log thumbnail caption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaReason {
    /// The doorbell button was pressed — grab a still of the caller.
    ButtonPress,
    /// Motion was seen at the door — grab a short clip of what moved.
    Motion,
}

/// A request for the camera pillar to capture media of the front door.
///
/// Carries the *when* (a caller-supplied tick) so the camera pillar and the
/// visitor log can line the capture up with the call it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaRequest {
    /// Snapshot vs short clip.
    pub kind: MediaKind,
    /// Why the request was raised.
    pub reason: MediaReason,
    /// The tick the triggering event occurred at.
    pub at: crate::event::Tick,
}

impl MediaRequest {
    /// Decide the media request for a door event at tick `at`, or `None` if the
    /// event does not warrant a capture (household actions and the timeout
    /// signal do not).
    ///
    /// A button press asks for a [`MediaKind::Snapshot`] — you want a clear
    /// face of whoever rang. Motion asks for a [`MediaKind::Clip`] — movement
    /// is better understood from a few seconds of video than a single frame.
    #[must_use]
    pub const fn for_event(event: DoorbellEvent, at: crate::event::Tick) -> Option<Self> {
        match event {
            DoorbellEvent::ButtonPressed => Some(Self {
                kind: MediaKind::Snapshot,
                reason: MediaReason::ButtonPress,
                at,
            }),
            DoorbellEvent::MotionDetected => Some(Self {
                kind: MediaKind::Clip,
                reason: MediaReason::Motion,
                at,
            }),
            DoorbellEvent::CallAnswered
            | DoorbellEvent::CallDeclined
            | DoorbellEvent::CallEnded
            | DoorbellEvent::VisitorTimeout => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_press_requests_a_snapshot() {
        let req = MediaRequest::for_event(DoorbellEvent::ButtonPressed, 42)
            .expect("press should request media");
        assert_eq!(req.kind, MediaKind::Snapshot);
        assert_eq!(req.reason, MediaReason::ButtonPress);
        assert_eq!(req.at, 42);
    }

    #[test]
    fn motion_requests_a_clip() {
        let req = MediaRequest::for_event(DoorbellEvent::MotionDetected, 7)
            .expect("motion should request media");
        assert_eq!(req.kind, MediaKind::Clip);
        assert_eq!(req.reason, MediaReason::Motion);
        assert_eq!(req.at, 7);
    }

    #[test]
    fn household_actions_request_no_media() {
        for ev in [
            DoorbellEvent::CallAnswered,
            DoorbellEvent::CallDeclined,
            DoorbellEvent::CallEnded,
            DoorbellEvent::VisitorTimeout,
        ] {
            assert_eq!(MediaRequest::for_event(ev, 0), None, "{ev:?} should not capture");
        }
    }
}
