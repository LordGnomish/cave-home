//! The music-library media model — the typed vocabulary cave-home reasons about
//! (ADR-020).
//!
//! Everything downstream — the playback [`crate::queue`], the per-room
//! [`crate::player`], the [`crate::library`] search — works off these types
//! alone. A music-provider adapter (a local file scan, a streaming service;
//! all deferred to Phase 1b, see the parity manifest) maps its catalog onto
//! this model and then reuses the engine unchanged. No type here touches the
//! network or the disk.

/// Where a piece of music came from.
///
/// A [`ProviderId`] names the *source* of a track without binding to any wire
/// protocol or account: it is just enough to disambiguate two tracks with the
/// same title. The actual fetch adapters are Phase 1b (ADR-020) and never
/// appear in this engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderId {
    /// Music that lives on the home's own storage.
    Local,
    /// A streaming service the household has linked (the specific service is an
    /// adapter detail; the engine only needs to know "not local").
    Streaming,
}

impl ProviderId {
    /// Short, end-user-facing label (Charter §6.3 — household words only).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Local => "Your music",
            Self::Streaming => "Online music",
        }
    }
}

/// A stable identifier for a library item.
///
/// The engine never parses or interprets an id; it only compares them for
/// equality and uses them to order a [`Playlist`] / [`crate::queue::Queue`]. A
/// provider adapter chooses how to mint ids (a file path hash, a service track
/// key); to the engine they are opaque strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrackId(String);

impl TrackId {
    /// Wrap an opaque id string.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The underlying id string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for TrackId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One playable song.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Stable, engine-opaque identifier.
    pub id: TrackId,
    /// Song title, as a person would read it.
    pub title: String,
    /// Performing artist name.
    pub artist: String,
    /// Album name.
    pub album: String,
    /// Length in whole seconds.
    pub duration_secs: u32,
    /// Which source this track came from.
    pub provider: ProviderId,
}

impl Track {
    /// Construct a track. Pure data assembly — no validation needed beyond the
    /// type system (a zero-length track is a legal, if odd, library entry).
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        artist: impl Into<String>,
        album: impl Into<String>,
        duration_secs: u32,
        provider: ProviderId,
    ) -> Self {
        Self {
            id: TrackId::new(id),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            duration_secs,
            provider,
        }
    }
}

/// A performing artist (a library grouping, not a playback unit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artist {
    pub name: String,
}

impl Artist {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// An album: a named collection of track ids in disc order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Album {
    pub title: String,
    pub artist: String,
    pub tracks: Vec<TrackId>,
}

impl Album {
    #[must_use]
    pub fn new(title: impl Into<String>, artist: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            artist: artist.into(),
            tracks: Vec::new(),
        }
    }

    /// Append a track id to the album order.
    #[must_use]
    pub fn with_track(mut self, id: impl Into<String>) -> Self {
        self.tracks.push(TrackId::new(id));
        self
    }
}

/// An ordered, named list of track ids — the thing a person calls "my Morning
/// playlist".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<TrackId>,
}

impl Playlist {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tracks: Vec::new(),
        }
    }

    /// Append a track id to the playlist order (builder style).
    #[must_use]
    pub fn with_track(mut self, id: impl Into<String>) -> Self {
        self.tracks.push(TrackId::new(id));
        self
    }

    /// How many songs the playlist holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Whether the playlist has no songs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }
}

/// A thing the household can ask to play. The engine resolves a [`MediaItem`]
/// into a flat list of track ids before enqueueing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaItem {
    /// One song.
    Track(TrackId),
    /// A whole album, in disc order.
    Album(Album),
    /// A named playlist, in its saved order.
    Playlist(Playlist),
}

impl MediaItem {
    /// Flatten this item into the ordered track ids it expands to.
    #[must_use]
    pub fn track_ids(&self) -> Vec<TrackId> {
        match self {
            Self::Track(id) => vec![id.clone()],
            Self::Album(a) => a.tracks.clone(),
            Self::Playlist(p) => p.tracks.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_carries_its_fields() {
        let t = Track::new("t1", "Clair de Lune", "Debussy", "Suite", 300, ProviderId::Local);
        assert_eq!(t.id.as_str(), "t1");
        assert_eq!(t.title, "Clair de Lune");
        assert_eq!(t.duration_secs, 300);
        assert_eq!(t.provider, ProviderId::Local);
    }

    #[test]
    fn media_item_track_flattens_to_one_id() {
        let item = MediaItem::Track(TrackId::new("solo"));
        assert_eq!(item.track_ids(), vec![TrackId::new("solo")]);
    }

    #[test]
    fn media_item_playlist_preserves_order() {
        let pl = Playlist::new("Morning")
            .with_track("a")
            .with_track("b")
            .with_track("c");
        assert_eq!(pl.len(), 3);
        let item = MediaItem::Playlist(pl);
        assert_eq!(
            item.track_ids(),
            vec![TrackId::new("a"), TrackId::new("b"), TrackId::new("c")]
        );
    }

    #[test]
    fn media_item_album_flattens_to_disc_order() {
        let album = Album::new("Suite", "Debussy")
            .with_track("mvt1")
            .with_track("mvt2");
        let item = MediaItem::Album(album);
        assert_eq!(item.track_ids(), vec![TrackId::new("mvt1"), TrackId::new("mvt2")]);
    }

    #[test]
    fn empty_playlist_reports_empty() {
        assert!(Playlist::new("New").is_empty());
    }

    #[test]
    fn provider_labels_are_household_words() {
        assert_eq!(ProviderId::Local.label(), "Your music");
        assert_eq!(ProviderId::Streaming.label(), "Online music");
    }
}
