//! The pure control layer — validated operations that transform a [`Topology`]
//! into a new state, plus the grandma-friendly headline of what is playing.
//!
//! Every operation is total and bounds-checked: it either applies and reports
//! success, or returns a [`ControlError`] without mutating anything observable
//! in a half-done way. Operations are modelled from the public Snapcast control
//! protocol (`Client.SetVolume`, `Client.SetLatency`, `Client.SetName`,
//! `Group.SetStream`, `Group.SetMute`, group create/dissolve, and moving a
//! client between groups). Snapcast source was NOT read.

use crate::client::Volume;
use crate::group::{self, Group};
use crate::label::{every_room, join_names, muted_word, playing_word, Lang};
use crate::sync::LatencyMs;
use crate::topology::{StreamStatus, Topology, TopologyError};

/// Why a control operation was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlError {
    /// No speaker with that id.
    UnknownClient,
    /// No group with that id.
    UnknownGroup,
    /// No stream with that id.
    UnknownStream,
    /// A structural change broke a topology rule.
    Topology(TopologyError),
    /// A group id collided with an existing one.
    DuplicateGroup,
    /// Dissolving the last group would orphan its speakers.
    CannotDissolveLastGroup,
}

impl core::fmt::Display for ControlError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownClient => f.write_str("no such speaker"),
            Self::UnknownGroup => f.write_str("no such group"),
            Self::UnknownStream => f.write_str("no such stream"),
            Self::Topology(e) => write!(f, "topology rule broken: {e}"),
            Self::DuplicateGroup => f.write_str("a group with that id already exists"),
            Self::CannotDissolveLastGroup => {
                f.write_str("cannot dissolve the only group a speaker belongs to")
            }
        }
    }
}

impl std::error::Error for ControlError {}

/// Set one speaker's volume.
///
/// # Errors
/// [`ControlError::UnknownClient`] if no such speaker.
pub fn set_client_volume(
    topo: &mut Topology,
    client_id: &str,
    volume: Volume,
) -> Result<(), ControlError> {
    let c = topo.client_mut(client_id).ok_or(ControlError::UnknownClient)?;
    c.set_volume(volume);
    Ok(())
}

/// Mute or unmute one speaker (independent of its volume).
///
/// # Errors
/// [`ControlError::UnknownClient`] if no such speaker.
pub fn set_client_mute(
    topo: &mut Topology,
    client_id: &str,
    muted: bool,
) -> Result<(), ControlError> {
    let c = topo.client_mut(client_id).ok_or(ControlError::UnknownClient)?;
    c.set_muted(muted);
    Ok(())
}

/// Trim one speaker's latency so it lines up with the others.
///
/// # Errors
/// [`ControlError::UnknownClient`] if no such speaker.
pub fn set_client_latency(
    topo: &mut Topology,
    client_id: &str,
    latency: LatencyMs,
) -> Result<(), ControlError> {
    let c = topo.client_mut(client_id).ok_or(ControlError::UnknownClient)?;
    c.set_latency(latency);
    Ok(())
}

/// Rename one speaker.
///
/// # Errors
/// [`ControlError::UnknownClient`] if no such speaker.
pub fn set_client_name(
    topo: &mut Topology,
    client_id: &str,
    name: &str,
) -> Result<(), ControlError> {
    let c = topo.client_mut(client_id).ok_or(ControlError::UnknownClient)?;
    c.set_name(name);
    Ok(())
}

/// Point a group at a different stream.
///
/// # Errors
/// - [`ControlError::UnknownGroup`] if no such group.
/// - [`ControlError::UnknownStream`] if no such stream.
pub fn set_group_stream(
    topo: &mut Topology,
    group_id: &str,
    stream_id: &str,
) -> Result<(), ControlError> {
    if topo.stream(stream_id).is_none() {
        return Err(ControlError::UnknownStream);
    }
    let g = topo.group_mut(group_id).ok_or(ControlError::UnknownGroup)?;
    g.set_stream(stream_id);
    Ok(())
}

/// Mute or unmute a whole group.
///
/// # Errors
/// [`ControlError::UnknownGroup`] if no such group.
pub fn set_group_mute(
    topo: &mut Topology,
    group_id: &str,
    muted: bool,
) -> Result<(), ControlError> {
    let g = topo.group_mut(group_id).ok_or(ControlError::UnknownGroup)?;
    g.set_muted(muted);
    Ok(())
}

/// Set a group's effective volume, spreading the change across its members.
///
/// The spread is proportional (the standard Snapcast group-volume behaviour).
/// Muted members keep their stored level but are excluded from the average.
///
/// # Errors
/// [`ControlError::UnknownGroup`] if no such group.
pub fn set_group_volume(
    topo: &mut Topology,
    group_id: &str,
    target: Volume,
) -> Result<(), ControlError> {
    let group = topo.group(group_id).ok_or(ControlError::UnknownGroup)?;
    let member_ids: Vec<String> = group.members().to_vec();
    let member_state: Vec<(Volume, bool)> = member_ids
        .iter()
        .filter_map(|id| topo.client(id).map(|c| (c.volume(), c.muted())))
        .collect();
    let new_volumes = group::spread_group_volume(&member_state, target);
    // member_state was built by filtering to known clients in member order; the
    // spread output is 1:1 with it, so zip applies each new level to its client.
    let known_ids: Vec<String> = member_ids
        .into_iter()
        .filter(|id| topo.client(id).is_some())
        .collect();
    for (id, vol) in known_ids.iter().zip(new_volumes) {
        if let Some(c) = topo.client_mut(id) {
            c.set_volume(vol);
        }
    }
    Ok(())
}

/// Move a speaker from whatever group it is in into another existing group,
/// preserving the "exactly one group" invariant.
///
/// # Errors
/// - [`ControlError::UnknownClient`] if no such speaker.
/// - [`ControlError::UnknownGroup`] if the destination group does not exist.
pub fn move_client_to_group(
    topo: &mut Topology,
    client_id: &str,
    dest_group_id: &str,
) -> Result<(), ControlError> {
    if topo.client(client_id).is_none() {
        return Err(ControlError::UnknownClient);
    }
    if topo.group(dest_group_id).is_none() {
        return Err(ControlError::UnknownGroup);
    }
    // Remove from any current group first (the invariant lets at most one match).
    let current = topo.group_of(client_id).map(|g| g.id().to_string());
    if let Some(cur) = current {
        if cur == dest_group_id {
            return Ok(()); // already there
        }
        if let Some(g) = topo.group_mut(&cur) {
            g.remove_member(client_id);
        }
    }
    if let Some(dest) = topo.group_mut(dest_group_id) {
        dest.add_member(client_id);
    }
    Ok(())
}

/// Create a new group bound to a stream by pulling the named speakers out of
/// their current groups (they may have been in different ones) and into the new
/// group. Preserves the one-group invariant.
///
/// # Errors
/// - [`ControlError::DuplicateGroup`] if the group id is taken.
/// - [`ControlError::UnknownStream`] if the stream does not exist.
/// - [`ControlError::UnknownClient`] if any member id is unknown.
pub fn create_group(
    topo: &mut Topology,
    group_id: &str,
    name: &str,
    stream_id: &str,
    member_ids: &[&str],
) -> Result<(), ControlError> {
    if topo.group(group_id).is_some() {
        return Err(ControlError::DuplicateGroup);
    }
    if topo.stream(stream_id).is_none() {
        return Err(ControlError::UnknownStream);
    }
    for id in member_ids {
        if topo.client(id).is_none() {
            return Err(ControlError::UnknownClient);
        }
    }
    // Detach members from their existing groups.
    for id in member_ids {
        let current = topo.group_of(id).map(|g| g.id().to_string());
        if let Some(cur) = current {
            if let Some(g) = topo.group_mut(&cur) {
                g.remove_member(id);
            }
        }
    }
    let members: Vec<String> = member_ids.iter().map(|s| (*s).to_string()).collect();
    topo.push_group(Group::new(group_id, name, stream_id, members));
    Ok(())
}

/// Dissolve a group, moving its speakers into another existing group so they are
/// never orphaned (the one-group invariant has no "groupless" state).
///
/// # Errors
/// - [`ControlError::UnknownGroup`] if either group id is unknown.
/// - [`ControlError::CannotDissolveLastGroup`] if `into_group_id` equals the
///   group being dissolved (there is nowhere for the speakers to go).
pub fn dissolve_group(
    topo: &mut Topology,
    group_id: &str,
    into_group_id: &str,
) -> Result<(), ControlError> {
    if group_id == into_group_id {
        return Err(ControlError::CannotDissolveLastGroup);
    }
    if topo.group(group_id).is_none() || topo.group(into_group_id).is_none() {
        return Err(ControlError::UnknownGroup);
    }
    let Some(removed) = topo.remove_group(group_id) else {
        return Err(ControlError::UnknownGroup);
    };
    if let Some(dest) = topo.group_mut(into_group_id) {
        for m in removed.members() {
            dest.add_member(m.clone());
        }
    }
    Ok(())
}

/// A one-line, household-facing summary of what the speakers are doing — the
/// string the Portal tile and a voice reply speak (Charter §6.3, ADR-007).
///
/// Examples (EN): "Kitchen and living room playing together", "Music in every
/// room", "Bedroom muted", "Nothing playing".
#[must_use]
pub fn headline(topo: &Topology, lang: Lang) -> String {
    // Names of speakers whose group is playing and that are not muted.
    let mut playing: Vec<&str> = Vec::new();
    let mut any_muted = false;
    for g in topo.groups() {
        let stream_playing = topo
            .stream(g.stream_id())
            .is_some_and(|s| s.status() == StreamStatus::Playing);
        for id in g.members() {
            if let Some(c) = topo.client(id) {
                if c.muted() || g.muted() {
                    any_muted = true;
                } else if stream_playing {
                    playing.push(c.name());
                }
            }
        }
    }

    if playing.is_empty() {
        if any_muted {
            return match lang {
                Lang::En => "Everything is muted".to_string(),
                Lang::De => "Alles ist stumm".to_string(),
                Lang::Tr => "Her şey sessizde".to_string(),
            };
        }
        return match lang {
            Lang::En => "Nothing playing".to_string(),
            Lang::De => "Nichts läuft".to_string(),
            Lang::Tr => "Hiçbir şey çalmıyor".to_string(),
        };
    }

    // Whole house: every connected speaker is playing.
    let total = topo.clients().iter().filter(|c| c.connected()).count();
    if playing.len() >= total && total > 1 {
        return match lang {
            Lang::En => format!("Music in {}", every_room(lang)),
            Lang::De => format!("Musik in {}", every_room(lang)),
            Lang::Tr => format!("{} müzik", capitalise(every_room(lang))),
        };
    }

    let names = join_names(&playing, lang);
    if playing.len() == 1 {
        format!("{names} {}", playing_word(lang))
    } else {
        match lang {
            Lang::En => format!("{names} playing together"),
            Lang::De => format!("{names} spielen zusammen"),
            Lang::Tr => format!("{names} birlikte çalıyor"),
        }
    }
}

/// A short status for a single speaker: "Bedroom playing" / "Bedroom muted".
#[must_use]
pub fn client_headline(topo: &Topology, client_id: &str, lang: Lang) -> Option<String> {
    let c = topo.client(client_id)?;
    let g = topo.group_of(client_id);
    let muted = c.muted() || g.is_some_and(crate::group::Group::muted);
    let playing = g
        .and_then(|g| topo.stream(g.stream_id()))
        .is_some_and(|s| s.status() == StreamStatus::Playing);
    let word = if muted {
        muted_word(lang)
    } else if playing {
        playing_word(lang)
    } else {
        match lang {
            Lang::En => "idle",
            Lang::De => "still",
            Lang::Tr => "boşta",
        }
    };
    Some(format!("{} {word}", c.name()))
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::client::Client;
    use crate::topology::Stream;

    fn vol(p: u8) -> Volume {
        Volume::new(p).expect("vol")
    }

    fn two_group_house() -> Topology {
        let mut t = Topology::new();
        t.add_stream(Stream::new("spotify", StreamStatus::Playing, "flac", "48000:16:2"));
        t.add_stream(Stream::new("radio", StreamStatus::Idle, "flac", "48000:16:2"));
        t.add_group(
            Group::new("g1", "Kitchen", "spotify", vec!["c1".into()]),
            vec![Client::new("c1", "Kitchen", vol(50))],
        )
        .expect("g1");
        t.add_group(
            Group::new("g2", "Living room", "radio", vec!["c2".into()]),
            vec![Client::new("c2", "Living room", vol(50))],
        )
        .expect("g2");
        t
    }

    #[test]
    fn set_volume_and_unknown_client_rejected() {
        let mut t = two_group_house();
        set_client_volume(&mut t, "c1", vol(80)).expect("ok");
        assert_eq!(t.client("c1").map(|c| c.volume().percent()), Some(80));
        assert_eq!(
            set_client_volume(&mut t, "ghost", vol(10)),
            Err(ControlError::UnknownClient)
        );
    }

    #[test]
    fn set_latency_clamps_via_value_object() {
        let mut t = two_group_house();
        set_client_latency(&mut t, "c1", LatencyMs::new(50_000)).expect("ok");
        assert_eq!(
            t.client("c1").map(|c| c.latency().millis()),
            Some(crate::sync::MAX_ABS_LATENCY_MS)
        );
    }

    #[test]
    fn set_group_stream_validates_stream() {
        let mut t = two_group_house();
        set_group_stream(&mut t, "g1", "radio").expect("ok");
        assert_eq!(t.group("g1").map(Group::stream_id), Some("radio"));
        assert_eq!(
            set_group_stream(&mut t, "g1", "nope"),
            Err(ControlError::UnknownStream)
        );
        assert_eq!(
            set_group_stream(&mut t, "nope", "radio"),
            Err(ControlError::UnknownGroup)
        );
    }

    #[test]
    fn group_volume_spreads_to_members() {
        let mut t = Topology::new();
        t.add_stream(Stream::new("s", StreamStatus::Playing, "flac", "48000:16:2"));
        t.add_group(
            Group::new("g1", "Downstairs", "s", vec!["c1".into(), "c2".into()]),
            vec![
                Client::new("c1", "Kitchen", vol(40)),
                Client::new("c2", "Living room", vol(80)),
            ],
        )
        .expect("g1");
        // Group avg 60 -> raise to 90 -> 60 and clamp(120)=100.
        set_group_volume(&mut t, "g1", vol(90)).expect("ok");
        assert_eq!(t.client("c1").map(|c| c.volume().percent()), Some(60));
        assert_eq!(t.client("c2").map(|c| c.volume().percent()), Some(100));
    }

    #[test]
    fn move_client_preserves_invariant() {
        let mut t = two_group_house();
        assert!(t.invariant_holds());
        move_client_to_group(&mut t, "c1", "g2").expect("move");
        assert!(t.invariant_holds());
        assert_eq!(t.group_of("c1").map(Group::id), Some("g2"));
        assert!(!t.group("g1").expect("g1").contains("c1"));
    }

    #[test]
    fn move_client_errors() {
        let mut t = two_group_house();
        assert_eq!(
            move_client_to_group(&mut t, "ghost", "g2"),
            Err(ControlError::UnknownClient)
        );
        assert_eq!(
            move_client_to_group(&mut t, "c1", "ghost"),
            Err(ControlError::UnknownGroup)
        );
    }

    #[test]
    fn create_group_pulls_members_and_keeps_invariant() {
        let mut t = two_group_house();
        create_group(&mut t, "g3", "Party", "spotify", &["c1", "c2"]).expect("create");
        assert!(t.invariant_holds());
        assert_eq!(t.group_of("c1").map(Group::id), Some("g3"));
        assert_eq!(t.group_of("c2").map(Group::id), Some("g3"));
        // Source groups now empty but still exist.
        assert_eq!(t.group("g1").map(|g| g.members().len()), Some(0));
    }

    #[test]
    fn create_group_rejects_dupes_and_unknowns() {
        let mut t = two_group_house();
        assert_eq!(
            create_group(&mut t, "g1", "x", "spotify", &[]),
            Err(ControlError::DuplicateGroup)
        );
        assert_eq!(
            create_group(&mut t, "g9", "x", "nope", &[]),
            Err(ControlError::UnknownStream)
        );
        assert_eq!(
            create_group(&mut t, "g9", "x", "spotify", &["ghost"]),
            Err(ControlError::UnknownClient)
        );
    }

    #[test]
    fn dissolve_group_rehomes_members() {
        let mut t = two_group_house();
        dissolve_group(&mut t, "g1", "g2").expect("dissolve");
        assert!(t.invariant_holds());
        assert!(t.group("g1").is_none());
        assert_eq!(t.group_of("c1").map(Group::id), Some("g2"));
        assert_eq!(t.group("g2").map(|g| g.members().len()), Some(2));
    }

    #[test]
    fn dissolve_into_self_rejected() {
        let mut t = two_group_house();
        assert_eq!(
            dissolve_group(&mut t, "g1", "g1"),
            Err(ControlError::CannotDissolveLastGroup)
        );
    }

    #[test]
    fn headline_playing_together() {
        let mut t = two_group_house();
        set_group_stream(&mut t, "g2", "spotify").expect("ok");
        // Both groups now on the playing stream.
        let h = headline(&t, Lang::En);
        // Two speakers, both playing => whole house => "every room".
        assert!(h.contains("every room"), "got {h}");
    }

    #[test]
    fn headline_one_speaker() {
        let t = two_group_house();
        // Only c1 (Kitchen) is on the playing stream; c2 is on idle radio.
        let h = headline(&t, Lang::En);
        assert!(h.contains("Kitchen") && h.contains("playing"), "got {h}");
    }

    #[test]
    fn headline_nothing_and_muted() {
        let mut t = two_group_house();
        set_group_stream(&mut t, "g1", "radio").expect("ok"); // both idle now
        assert_eq!(headline(&t, Lang::En), "Nothing playing");
        set_client_mute(&mut t, "c1", true).expect("ok");
        set_client_mute(&mut t, "c2", true).expect("ok");
        assert_eq!(headline(&t, Lang::En), "Everything is muted");
    }

    #[test]
    fn client_headline_states() {
        let mut t = two_group_house();
        assert_eq!(
            client_headline(&t, "c1", Lang::En).as_deref(),
            Some("Kitchen playing")
        );
        set_client_mute(&mut t, "c1", true).expect("ok");
        assert_eq!(
            client_headline(&t, "c1", Lang::En).as_deref(),
            Some("Kitchen muted")
        );
        assert_eq!(client_headline(&t, "ghost", Lang::En), None);
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-020: the household UI must never surface protocol
        // or control-plane terms.
        const BANNED: &[&str] = &[
            "JSON-RPC", "Snapcast", "PCM", "codec", "stream_id", "client_id",
            "group_id", "latency", "buffer", "MQTT", "entity_id", "Group.SetStream",
            "Client.SetVolume", "TCP",
        ];
        let mut t = two_group_house();
        set_group_stream(&mut t, "g2", "spotify").expect("ok");
        let mut phrases = vec![
            headline(&t, Lang::En),
            headline(&t, Lang::De),
            headline(&t, Lang::Tr),
        ];
        for cid in ["c1", "c2"] {
            for l in [Lang::En, Lang::De, Lang::Tr] {
                if let Some(h) = client_headline(&t, cid, l) {
                    phrases.push(h);
                }
            }
        }
        for p in phrases {
            for b in BANNED {
                assert!(!p.contains(b), "headline leaks jargon {b:?}: {p}");
            }
        }
    }
}
