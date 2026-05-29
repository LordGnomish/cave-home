//! A synchronised group of speakers and the group-volume arithmetic.
//!
//! Modelled from the public Snapcast control-protocol description: a group has
//! an id, an optional name, the id of the stream it is playing, an ordered list
//! of member client ids, and a group mute flag. Every client belongs to exactly
//! one group (the [`crate::topology`] layer enforces that invariant). Snapcast
//! source was NOT read.
//!
//! Group volume is *derived*, not stored: a group's effective volume is the
//! average of its unmuted members, and setting a group volume spreads the change
//! across members proportionally — the standard Snapcast behaviour, here
//! implemented as pure arithmetic and unit-tested for the proportional spread
//! and clamping.

use crate::client::Volume;

/// A group of speakers that play in sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    id: String,
    name: String,
    stream_id: String,
    members: Vec<String>,
    muted: bool,
}

impl Group {
    /// Create a group bound to a stream with an initial membership.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        stream_id: impl Into<String>,
        members: Vec<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            stream_id: stream_id.into(),
            members,
            muted: false,
        }
    }

    /// The stable group id (control-plane internal).
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The household-facing group name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The id of the stream this group is playing.
    #[must_use]
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }

    /// The member client ids, in order.
    #[must_use]
    pub fn members(&self) -> &[String] {
        &self.members
    }

    /// Whether the whole group is muted.
    #[must_use]
    pub const fn muted(&self) -> bool {
        self.muted
    }

    /// Whether the given client is a member.
    #[must_use]
    pub fn contains(&self, client_id: &str) -> bool {
        self.members.iter().any(|m| m == client_id)
    }

    pub(crate) fn set_stream(&mut self, stream_id: impl Into<String>) {
        self.stream_id = stream_id.into();
    }

    pub(crate) const fn set_muted(&mut self, m: bool) {
        self.muted = m;
    }

    pub(crate) fn add_member(&mut self, client_id: impl Into<String>) {
        let id = client_id.into();
        if !self.members.contains(&id) {
            self.members.push(id);
        }
    }

    pub(crate) fn remove_member(&mut self, client_id: &str) -> bool {
        let before = self.members.len();
        self.members.retain(|m| m != client_id);
        self.members.len() != before
    }
}

/// The effective group volume: the average volume of its *unmuted* members.
///
/// Rounded to the nearest percent. Returns 0 for a group with no audible
/// members (all muted or empty) — there is nothing to average.
#[must_use]
pub fn effective_volume(member_volumes: &[(Volume, bool)]) -> u8 {
    let mut sum: u32 = 0;
    let mut count: u32 = 0;
    for (vol, muted) in member_volumes {
        if !*muted {
            sum += u32::from(vol.percent());
            count += 1;
        }
    }
    if count == 0 {
        return 0;
    }
    // Round to nearest: (sum + count/2) / count.
    #[allow(clippy::cast_possible_truncation)]
    {
        ((sum + count / 2) / count) as u8
    }
}

/// Spread a new target group volume across members proportionally to their
/// current volumes — the standard Snapcast group-volume behaviour.
///
/// The ratio is `target / current_group_volume`; each member's new volume is
/// its current volume times that ratio, clamped to `0..=100`. Two important
/// edge cases, both handled here and tested:
/// - if the current group volume is 0 (silent or empty), there is no ratio to
///   scale by, so every member is set flat to `target` (the only sensible way
///   to bring a silent group up to a level);
/// - clamping is per-member, so raising a group whose loudest member is already
///   near 100 compresses (the loud ones clamp, the quiet ones still rise).
///
/// Muted members keep their stored volume (they are excluded from the average
/// but their underlying level is preserved for when they unmute).
#[must_use]
pub fn spread_group_volume(member_volumes: &[(Volume, bool)], target: Volume) -> Vec<Volume> {
    let current = effective_volume(member_volumes);
    member_volumes
        .iter()
        .map(|(vol, _muted)| {
            if current == 0 {
                target
            } else {
                let scaled = i32::from(vol.percent()) * i32::from(target.percent())
                    / i32::from(current);
                Volume::clamped(scaled)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    fn v(p: u8) -> Volume {
        Volume::new(p).expect("vol")
    }

    #[test]
    fn effective_volume_averages_unmuted() {
        let members = [(v(40), false), (v(80), false)];
        assert_eq!(effective_volume(&members), 60);
    }

    #[test]
    fn effective_volume_excludes_muted_and_rounds() {
        // 50 and 51 unmuted, 100 muted -> average of 50,51 = 50.5 -> 51.
        let members = [(v(50), false), (v(51), false), (v(100), true)];
        assert_eq!(effective_volume(&members), 51);
    }

    #[test]
    fn effective_volume_zero_when_no_audible_members() {
        assert_eq!(effective_volume(&[]), 0);
        assert_eq!(effective_volume(&[(v(80), true)]), 0);
    }

    #[test]
    fn spread_scales_proportionally() {
        // Group at 60 (avg of 40,80) raised to 90 -> ratio 1.5 -> 60, 120->clamp 100.
        let members = [(v(40), false), (v(80), false)];
        let out = spread_group_volume(&members, v(90));
        assert_eq!(out[0].percent(), 60);
        assert_eq!(out[1].percent(), 100); // 80*1.5=120 clamped
    }

    #[test]
    fn spread_lowering_preserves_ratio() {
        // Group at 60 lowered to 30 -> ratio 0.5 -> 20, 40.
        let members = [(v(40), false), (v(80), false)];
        let out = spread_group_volume(&members, v(30));
        assert_eq!(out[0].percent(), 20);
        assert_eq!(out[1].percent(), 40);
    }

    #[test]
    fn spread_from_silent_group_sets_flat() {
        // No ratio exists from 0; bring all members to the target.
        let members = [(v(0), false), (v(0), false)];
        let out = spread_group_volume(&members, v(45));
        assert_eq!(out[0].percent(), 45);
        assert_eq!(out[1].percent(), 45);
    }

    #[test]
    fn membership_helpers() {
        let mut g = Group::new("g1", "Downstairs", "spotify", vec!["c1".into()]);
        assert!(g.contains("c1"));
        g.add_member("c2");
        g.add_member("c2"); // idempotent
        assert_eq!(g.members().len(), 2);
        assert!(g.remove_member("c1"));
        assert!(!g.remove_member("c1")); // already gone
        assert!(!g.contains("c1"));
    }
}
