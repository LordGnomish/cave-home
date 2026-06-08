//! The server topology — the `Server.GetStatus` tree: streams, clients and
//! groups, with the invariants that hold them together.
//!
//! Modelled from the public Snapcast control-protocol description. A server
//! status is a set of audio [`Stream`]s, a set of [`crate::client::Client`]
//! speakers, and a set of [`crate::group::Group`]s. The load-bearing invariant
//! (enforced here, not assumed) is that **every client belongs to exactly one
//! group**. Snapcast source was NOT read.

use crate::client::Client;
use crate::group::Group;

/// What an audio stream is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamStatus {
    /// Audio is flowing.
    Playing,
    /// Connected but silent (paused / no source data).
    Idle,
}

impl StreamStatus {
    /// The Snapcast wire token for this status.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Idle => "idle",
        }
    }

    /// Parse a wire token; unknown tokens are treated as idle (fail safe).
    #[must_use]
    pub fn from_wire(s: &str) -> Self {
        if s == "playing" {
            Self::Playing
        } else {
            Self::Idle
        }
    }
}

/// An audio source the groups can play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stream {
    id: String,
    status: StreamStatus,
    codec: String,
    sample_format: String,
}

impl Stream {
    /// Create a stream with a codec and sample-format descriptor
    /// (e.g. codec `"flac"`, sample-format `"48000:16:2"`).
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        status: StreamStatus,
        codec: impl Into<String>,
        sample_format: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            status,
            codec: codec.into(),
            sample_format: sample_format.into(),
        }
    }

    /// The stable stream id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Whether the stream is playing or idle.
    #[must_use]
    pub const fn status(&self) -> StreamStatus {
        self.status
    }

    /// The codec name.
    #[must_use]
    pub fn codec(&self) -> &str {
        &self.codec
    }

    /// The sample-format descriptor.
    #[must_use]
    pub fn sample_format(&self) -> &str {
        &self.sample_format
    }
}

/// The whole control-plane state: streams, speakers and groups.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Topology {
    streams: Vec<Stream>,
    clients: Vec<Client>,
    groups: Vec<Group>,
}

impl Topology {
    /// An empty topology.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a stream (replacing any existing stream with the same id).
    pub fn add_stream(&mut self, stream: Stream) {
        if let Some(slot) = self.streams.iter_mut().find(|s| s.id() == stream.id()) {
            *slot = stream;
        } else {
            self.streams.push(stream);
        }
    }

    /// All streams.
    #[must_use]
    pub fn streams(&self) -> &[Stream] {
        &self.streams
    }

    /// Look up a stream by id.
    #[must_use]
    pub fn stream(&self, id: &str) -> Option<&Stream> {
        self.streams.iter().find(|s| s.id() == id)
    }

    /// All speakers.
    #[must_use]
    pub fn clients(&self) -> &[Client] {
        &self.clients
    }

    /// Look up a speaker by id.
    #[must_use]
    pub fn client(&self, id: &str) -> Option<&Client> {
        self.clients.iter().find(|c| c.id() == id)
    }

    /// All groups.
    #[must_use]
    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// Look up a group by id.
    #[must_use]
    pub fn group(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.id() == id)
    }

    /// The group that owns a given client, if any.
    #[must_use]
    pub fn group_of(&self, client_id: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.contains(client_id))
    }

    /// Add a speaker that joins an existing group.
    ///
    /// # Errors
    /// - [`TopologyError::DuplicateClient`] if the client id already exists.
    /// - [`TopologyError::UnknownGroup`] if `group_id` is not a known group.
    pub fn add_client_to_group(
        &mut self,
        client: Client,
        group_id: &str,
    ) -> Result<(), TopologyError> {
        if self.client(client.id()).is_some() {
            return Err(TopologyError::DuplicateClient);
        }
        let id = client.id().to_string();
        let Some(group) = self.groups.iter_mut().find(|g| g.id() == group_id) else {
            return Err(TopologyError::UnknownGroup);
        };
        group.add_member(id);
        self.clients.push(client);
        Ok(())
    }

    /// Register a group along with the speakers that belong only to it. The
    /// group's member list is taken as authoritative for those clients.
    ///
    /// # Errors
    /// - [`TopologyError::DuplicateGroup`] if the group id already exists.
    /// - [`TopologyError::DuplicateClient`] if any supplied client already
    ///   exists in another group (the one-group invariant).
    pub fn add_group(
        &mut self,
        group: Group,
        clients: Vec<Client>,
    ) -> Result<(), TopologyError> {
        if self.group(group.id()).is_some() {
            return Err(TopologyError::DuplicateGroup);
        }
        for c in &clients {
            if self.client(c.id()).is_some() {
                return Err(TopologyError::DuplicateClient);
            }
        }
        self.clients.extend(clients);
        self.groups.push(group);
        Ok(())
    }

    /// Assert the one-group invariant: every client is in exactly one group and
    /// every group member is a known client. Used by tests and by the control
    /// layer after structural changes.
    #[must_use]
    pub fn invariant_holds(&self) -> bool {
        // Every client appears in exactly one group.
        for c in &self.clients {
            let count = self.groups.iter().filter(|g| g.contains(c.id())).count();
            if count != 1 {
                return false;
            }
        }
        // Every group member is a known client.
        for g in &self.groups {
            for m in g.members() {
                if self.client(m).is_none() {
                    return false;
                }
            }
        }
        true
    }

    // --- crate-internal mutable accessors for the control layer ---

    pub(crate) fn client_mut(&mut self, id: &str) -> Option<&mut Client> {
        self.clients.iter_mut().find(|c| c.id() == id)
    }

    pub(crate) fn group_mut(&mut self, id: &str) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id() == id)
    }

    pub(crate) fn remove_group(&mut self, id: &str) -> Option<Group> {
        let idx = self.groups.iter().position(|g| g.id() == id)?;
        Some(self.groups.remove(idx))
    }

    pub(crate) fn push_group(&mut self, group: Group) {
        self.groups.push(group);
    }
}

/// Why a structural topology change was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyError {
    /// A client with that id already exists.
    DuplicateClient,
    /// A group with that id already exists.
    DuplicateGroup,
    /// The referenced group does not exist.
    UnknownGroup,
}

impl core::fmt::Display for TopologyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DuplicateClient => f.write_str("a speaker with that id already exists"),
            Self::DuplicateGroup => f.write_str("a group with that id already exists"),
            Self::UnknownGroup => f.write_str("no such group"),
        }
    }
}

impl std::error::Error for TopologyError {}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::client::Volume;

    fn client(id: &str, name: &str) -> Client {
        Client::new(id, name, Volume::new(50).expect("vol"))
    }

    #[test]
    fn stream_status_wire_round_trip() {
        assert_eq!(StreamStatus::from_wire("playing"), StreamStatus::Playing);
        assert_eq!(StreamStatus::from_wire("idle"), StreamStatus::Idle);
        assert_eq!(StreamStatus::from_wire("???"), StreamStatus::Idle);
        assert_eq!(StreamStatus::Playing.as_wire(), "playing");
    }

    #[test]
    fn build_topology_and_check_invariant() {
        let mut t = Topology::new();
        t.add_stream(Stream::new("spotify", StreamStatus::Playing, "flac", "48000:16:2"));
        t.add_group(
            Group::new("g1", "Kitchen", "spotify", vec!["c1".into()]),
            vec![client("c1", "Kitchen")],
        )
        .expect("add group");
        assert!(t.invariant_holds());
        assert_eq!(t.group_of("c1").map(Group::id), Some("g1"));
    }

    #[test]
    fn duplicate_group_rejected() {
        let mut t = Topology::new();
        t.add_group(
            Group::new("g1", "Kitchen", "s", vec![]),
            vec![],
        )
        .expect("first");
        assert_eq!(
            t.add_group(Group::new("g1", "Other", "s", vec![]), vec![]),
            Err(TopologyError::DuplicateGroup)
        );
    }

    #[test]
    fn add_client_to_unknown_group_rejected() {
        let mut t = Topology::new();
        assert_eq!(
            t.add_client_to_group(client("c1", "Kitchen"), "nope"),
            Err(TopologyError::UnknownGroup)
        );
    }

    #[test]
    fn duplicate_client_rejected() {
        let mut t = Topology::new();
        t.add_group(Group::new("g1", "K", "s", vec![]), vec![])
            .expect("group");
        t.add_client_to_group(client("c1", "Kitchen"), "g1")
            .expect("first");
        assert_eq!(
            t.add_client_to_group(client("c1", "Dup"), "g1"),
            Err(TopologyError::DuplicateClient)
        );
    }

    #[test]
    fn stream_replace_by_id() {
        let mut t = Topology::new();
        t.add_stream(Stream::new("s", StreamStatus::Idle, "pcm", "44100:16:2"));
        t.add_stream(Stream::new("s", StreamStatus::Playing, "flac", "48000:16:2"));
        assert_eq!(t.streams().len(), 1);
        assert_eq!(t.stream("s").map(Stream::status), Some(StreamStatus::Playing));
    }
}
