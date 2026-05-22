// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Call / call-event / call-control surfaces.

use serde::{Deserialize, Serialize};

use crate::phone::PhoneId;

/// Unique call identifier (Talk hub call GUID).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallId(String);

impl CallId {
    /// Construct from any string.
    #[must_use]
    pub fn new<S: Into<String>>(raw: S) -> Self {
        Self(raw.into())
    }
    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// An incoming call from outside the Talk hub or another extension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingCall {
    /// Call GUID.
    pub id: CallId,
    /// Calling-party extension (E.164 or SIP user).
    pub from_extension: String,
    /// Destination phone on the Talk hub.
    pub to_phone: PhoneId,
    /// Optional display name (caller-ID).
    pub from_display_name: Option<String>,
}

/// Kind of call event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CallEventKind {
    /// New incoming call.
    Incoming,
    /// User answered the call.
    Answered,
    /// User declined the call.
    Declined,
    /// Call completed normally.
    Ended,
    /// Call rang through to voicemail / timed out.
    Missed,
    /// Call was transferred to another phone.
    Transferred,
}

impl CallEventKind {
    /// Wire-form string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incoming => "incoming",
            Self::Answered => "answered",
            Self::Declined => "declined",
            Self::Ended => "ended",
            Self::Missed => "missed",
            Self::Transferred => "transferred",
        }
    }

    /// Parse the wire form.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "incoming" => Self::Incoming,
            "answered" => Self::Answered,
            "declined" => Self::Declined,
            "ended" => Self::Ended,
            "missed" => Self::Missed,
            "transferred" => Self::Transferred,
            _ => return None,
        })
    }

    /// Iterate every variant.
    #[must_use]
    pub fn all() -> [Self; 6] {
        [
            Self::Incoming,
            Self::Answered,
            Self::Declined,
            Self::Ended,
            Self::Missed,
            Self::Transferred,
        ]
    }
}

/// A call lifecycle event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallEvent {
    /// Call this event refers to.
    pub call: CallId,
    /// Phone this event affects.
    pub phone: PhoneId,
    /// Event kind.
    pub kind: CallEventKind,
}

/// Verbs the portal / automation can issue against an active call.
///
/// The Phase 1 set covers the four buttons grandma's intercom tile
/// shows: answer, decline, transfer, end. Voicemail / hold / forward
/// are Phase 2 tickets (Ubiquiti API stability dependent).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallControlVerb {
    /// Accept the incoming call.
    Answer,
    /// Reject the incoming call.
    Decline,
    /// Hand off to another phone (with `target` extension).
    Transfer,
    /// Hang up the call.
    End,
}

impl CallControlVerb {
    /// Wire-form string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Decline => "decline",
            Self::Transfer => "transfer",
            Self::End => "end",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_kind_round_trip() {
        for v in CallEventKind::all() {
            assert_eq!(CallEventKind::parse(v.as_str()), Some(v));
        }
    }

    #[test]
    fn control_verb_strings() {
        assert_eq!(CallControlVerb::Answer.as_str(), "answer");
        assert_eq!(CallControlVerb::Decline.as_str(), "decline");
    }
}
