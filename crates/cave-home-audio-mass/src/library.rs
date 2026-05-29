//! An in-memory music library with search/filter (ADR-020).
//!
//! The [`Library`] is the catalog the household browses and searches. It is
//! populated entirely from already-resolved [`Track`]s — a provider adapter (a
//! local file scan, a streaming catalog; all Phase 1b, see the parity manifest)
//! is what fills it from the outside world. The engine only searches and looks
//! up; it never reaches for the network or the disk.
//!
//! Search is a case-insensitive substring match over title / artist / album, so
//! "deb" finds "Debussy" and "MORNING" finds the "Morning Light" track.

use crate::media::{Track, TrackId};
use std::collections::HashMap;

/// Which fields a search should look in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchFields {
    pub title: bool,
    pub artist: bool,
    pub album: bool,
}

impl SearchFields {
    /// Search every field — the default a person expects from one search box.
    #[must_use]
    pub const fn all() -> Self {
        Self { title: true, artist: true, album: true }
    }

    /// Search only by artist (for "everything by …").
    #[must_use]
    pub const fn artist_only() -> Self {
        Self { title: false, artist: true, album: false }
    }
}

impl Default for SearchFields {
    fn default() -> Self {
        Self::all()
    }
}

/// An in-memory catalog of tracks, indexed by id.
#[derive(Debug, Clone, Default)]
pub struct Library {
    by_id: HashMap<TrackId, Track>,
    /// Insertion order, so search results are stable and predictable.
    order: Vec<TrackId>,
}

impl Library {
    /// An empty library.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add (or replace) a track. Replacing keeps the original catalog position.
    pub fn add(&mut self, track: Track) {
        let id = track.id.clone();
        if !self.by_id.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.by_id.insert(id, track);
    }

    /// How many tracks the library holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Whether the library is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Look up a track by id.
    #[must_use]
    pub fn get(&self, id: &TrackId) -> Option<&Track> {
        self.by_id.get(id)
    }

    /// Every track, in catalog (insertion) order.
    #[must_use]
    pub fn tracks(&self) -> Vec<&Track> {
        self.order.iter().filter_map(|id| self.by_id.get(id)).collect()
    }

    /// Case-insensitive substring search across the chosen fields.
    ///
    /// An empty query matches nothing (a search box with nothing typed shows no
    /// results, rather than the whole library). Results come back in catalog
    /// order.
    #[must_use]
    pub fn search(&self, query: &str, fields: SearchFields) -> Vec<&Track> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.tracks()
            .into_iter()
            .filter(|t| {
                (fields.title && t.title.to_lowercase().contains(&needle))
                    || (fields.artist && t.artist.to_lowercase().contains(&needle))
                    || (fields.album && t.album.to_lowercase().contains(&needle))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::ProviderId;

    fn sample() -> Library {
        let mut lib = Library::new();
        lib.add(Track::new("t1", "Clair de Lune", "Debussy", "Suite Bergamasque", 300, ProviderId::Local));
        lib.add(Track::new("t2", "Morning Light", "Ludovico", "Mornings", 240, ProviderId::Streaming));
        lib.add(Track::new("t3", "Reverie", "Debussy", "Solo Piano", 280, ProviderId::Local));
        lib
    }

    #[test]
    fn add_and_len() {
        let lib = sample();
        assert_eq!(lib.len(), 3);
        assert!(!lib.is_empty());
    }

    #[test]
    fn re_adding_same_id_replaces_keeps_position() {
        let mut lib = sample();
        lib.add(Track::new("t1", "Clair de Lune (Remaster)", "Debussy", "Suite Bergamasque", 305, ProviderId::Local));
        assert_eq!(lib.len(), 3);
        assert_eq!(lib.get(&TrackId::new("t1")).map(|t| t.title.as_str()), Some("Clair de Lune (Remaster)"));
        // Still first in catalog order.
        assert_eq!(lib.tracks()[0].id, TrackId::new("t1"));
    }

    #[test]
    fn search_by_artist_substring_case_insensitive() {
        let lib = sample();
        let hits = lib.search("deb", SearchFields::all());
        assert_eq!(hits.len(), 2, "Debussy appears on two tracks");
        assert_eq!(hits[0].id, TrackId::new("t1"));
        assert_eq!(hits[1].id, TrackId::new("t3"));
    }

    #[test]
    fn search_by_title_substring() {
        let lib = sample();
        let hits = lib.search("MORNING", SearchFields::all());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, TrackId::new("t2"));
    }

    #[test]
    fn search_by_album() {
        let lib = sample();
        let hits = lib.search("piano", SearchFields::all());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, TrackId::new("t3"));
    }

    #[test]
    fn search_field_restriction() {
        let lib = sample();
        // "morning" is a title and an album word, but artist_only should miss it.
        let hits = lib.search("morning", SearchFields::artist_only());
        assert!(hits.is_empty());
    }

    #[test]
    fn empty_query_matches_nothing() {
        let lib = sample();
        assert!(lib.search("   ", SearchFields::all()).is_empty());
        assert!(lib.search("", SearchFields::all()).is_empty());
    }

    #[test]
    fn lookup_by_id() {
        let lib = sample();
        assert_eq!(lib.get(&TrackId::new("t2")).map(|t| t.artist.as_str()), Some("Ludovico"));
        assert!(lib.get(&TrackId::new("missing")).is_none());
    }
}
