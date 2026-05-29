//! Presence / device-tracker model — "is this person home?".
//!
//! Ports HA `unifi`'s `device_tracker` semantics as pure logic: a tracked
//! client is **home** while the network has seen it recently, and flips to
//! **away** once it has been silent longer than a consider-home timeout. This
//! is the input to presence automations ("turn the heating down when everyone
//! leaves").
//!
//! The crate reads no clock: the caller supplies `now` and the `timeout`, both
//! on the same monotonic tick scale as [`crate::client::NetworkClient::last_seen`].

use crate::client::NetworkClient;

/// Whether a tracked client is considered present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// Seen within the timeout — treat the owner as home.
    Home,
    /// Silent longer than the timeout — treat the owner as away.
    Away,
}

impl Presence {
    #[must_use]
    pub const fn is_home(self) -> bool {
        matches!(self, Self::Home)
    }
}

/// Derive presence for a client from its last-seen tick.
///
/// `now` and `timeout` share the client's tick scale. The client is [`Home`]
/// while `now - last_seen <= timeout`, and [`Away`] once the gap exceeds the
/// timeout. A `now` earlier than `last_seen` (clock went backwards / a future
/// stamp) is treated as just-seen, i.e. [`Home`] — we never report a present
/// device as away on a clock glitch.
///
/// A blocked client is reported [`Away`] regardless of last-seen: cave-home
/// must not light up "home" for a device it has deliberately cut off.
///
/// [`Home`]: Presence::Home
/// [`Away`]: Presence::Away
#[must_use]
pub fn presence_of(client: &NetworkClient, now: u64, timeout: u64) -> Presence {
    if client.is_blocked() {
        return Presence::Away;
    }
    let last = client.last_seen();
    if now <= last {
        return Presence::Home;
    }
    if now - last <= timeout {
        Presence::Home
    } else {
        Presence::Away
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client_seen_at(tick: u64) -> NetworkClient {
        NetworkClient::new("aa:bb", "Phone")
            .wireless("Home", "ap-1")
            .last_seen_at(tick)
    }

    #[test]
    fn recently_seen_is_home() {
        let c = client_seen_at(100);
        assert_eq!(presence_of(&c, 120, 300), Presence::Home);
        assert!(presence_of(&c, 120, 300).is_home());
    }

    #[test]
    fn exactly_at_timeout_is_still_home() {
        let c = client_seen_at(100);
        // now - last == timeout -> boundary is inclusive (still home).
        assert_eq!(presence_of(&c, 400, 300), Presence::Home);
    }

    #[test]
    fn past_timeout_is_away() {
        let c = client_seen_at(100);
        assert_eq!(presence_of(&c, 401, 300), Presence::Away);
    }

    #[test]
    fn future_now_is_treated_as_just_seen() {
        let c = client_seen_at(500);
        assert_eq!(presence_of(&c, 100, 300), Presence::Home);
    }

    #[test]
    fn blocked_client_is_always_away() {
        let c = client_seen_at(100).blocked();
        // Even though it was just seen, a blocked client is not "home".
        assert_eq!(presence_of(&c, 101, 300), Presence::Away);
    }
}
