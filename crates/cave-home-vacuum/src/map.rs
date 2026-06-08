//! Room/segment and zone cleaning model.
//!
//! Valetudo exposes a vacuum's saved map as a set of numbered **segments**
//! (rooms) the user can name, plus arbitrary rectangular **zones** for one-off
//! "clean just here" requests. cave-home models the *request and its
//! validation* — the household asks to clean rooms by name, and a clean-segments
//! request is checked against the known map before anything is dispatched.
//!
//! Persisting the map itself (lidar scans, pixel grids) is network/hardware
//! bound and deferred to Phase-1b (see `parity.manifest.toml`, ADR-017). What is
//! real here is the typed, validated request the engine accepts.

use std::collections::BTreeSet;

/// One mapped room: a stable numeric id plus the household name for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    id: u16,
    name: String,
}

impl Segment {
    /// Build a segment from its map id and household name.
    #[must_use]
    pub fn new(id: u16, name: impl Into<String>) -> Self {
        Self { id, name: name.into() }
    }

    #[must_use]
    pub const fn id(&self) -> u16 {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A rectangular cleaning zone in the vacuum's map coordinate space.
///
/// Coordinates are stored normalised so `x1 <= x2` and `y1 <= y2`, and a zone
/// must enclose real area (no zero-width / zero-height "lines").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

/// Why a [`Zone`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneError {
    /// The two corners describe a line or a point, not a rectangle.
    DegenerateRectangle,
}

impl core::fmt::Display for ZoneError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DegenerateRectangle => f.write_str("the cleaning area has no size"),
        }
    }
}

impl std::error::Error for ZoneError {}

impl Zone {
    /// Build a zone from two opposite corners. The corners are normalised, so
    /// the order they are given in does not matter.
    ///
    /// # Errors
    /// [`ZoneError::DegenerateRectangle`] if the corners do not enclose area.
    pub fn new(ax: i32, ay: i32, bx: i32, by: i32) -> Result<Self, ZoneError> {
        let (x1, x2) = (ax.min(bx), ax.max(bx));
        let (y1, y2) = (ay.min(by), ay.max(by));
        if x1 == x2 || y1 == y2 {
            return Err(ZoneError::DegenerateRectangle);
        }
        Ok(Self { x1, y1, x2, y2 })
    }

    #[must_use]
    pub const fn corners(self) -> (i32, i32, i32, i32) {
        (self.x1, self.y1, self.x2, self.y2)
    }

    /// The enclosed area in map units squared.
    #[must_use]
    pub const fn area(self) -> i64 {
        let w = (self.x2 - self.x1) as i64;
        let h = (self.y2 - self.y1) as i64;
        w * h
    }
}

/// The set of rooms a particular vacuum has on its saved map.
///
/// This is the source of truth a clean-segments request is validated against:
/// asking to clean a room id the vacuum has never mapped is rejected, rather
/// than silently dropped, so the household learns the room is not set up yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VacuumMap {
    segments: Vec<Segment>,
}

/// Why a clean-segments request was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentRequestError {
    /// The request named no rooms at all.
    Empty,
    /// One or more requested room ids are not on the saved map. Carries the
    /// unknown ids, sorted, so the caller can tell the user which room is the
    /// problem.
    UnknownSegments(Vec<u16>),
}

impl core::fmt::Display for SegmentRequestError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => f.write_str("no rooms were chosen to clean"),
            Self::UnknownSegments(_) => {
                f.write_str("one of the chosen rooms is not on the vacuum's map")
            }
        }
    }
}

impl std::error::Error for SegmentRequestError {}

impl VacuumMap {
    /// An empty map — a vacuum that has not learned its home yet.
    #[must_use]
    pub const fn empty() -> Self {
        Self { segments: Vec::new() }
    }

    /// Build a map from a set of known rooms.
    #[must_use]
    pub const fn new(segments: Vec<Segment>) -> Self {
        Self { segments }
    }

    /// The rooms on this map.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Whether a room id is on the map.
    #[must_use]
    pub fn contains(&self, id: u16) -> bool {
        self.segments.iter().any(|s| s.id == id)
    }

    /// Look up a room by id.
    #[must_use]
    pub fn segment(&self, id: u16) -> Option<&Segment> {
        self.segments.iter().find(|s| s.id == id)
    }

    /// Validate a clean-segments request against this map.
    ///
    /// On success returns the requested ids de-duplicated and in a stable order.
    ///
    /// # Errors
    /// - [`SegmentRequestError::Empty`] if `requested` is empty.
    /// - [`SegmentRequestError::UnknownSegments`] if any requested id is not on
    ///   the map; the error lists every unknown id.
    pub fn validate_segments(
        &self,
        requested: &[u16],
    ) -> Result<Vec<u16>, SegmentRequestError> {
        if requested.is_empty() {
            return Err(SegmentRequestError::Empty);
        }
        let unknown: BTreeSet<u16> =
            requested.iter().copied().filter(|id| !self.contains(*id)).collect();
        if !unknown.is_empty() {
            return Err(SegmentRequestError::UnknownSegments(unknown.into_iter().collect()));
        }
        let known: BTreeSet<u16> = requested.iter().copied().collect();
        Ok(known.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> VacuumMap {
        VacuumMap::new(vec![
            Segment::new(1, "Kitchen"),
            Segment::new(2, "Living room"),
            Segment::new(3, "Hallway"),
        ])
    }

    #[test]
    fn segment_carries_id_and_name() {
        let s = Segment::new(1, "Kitchen");
        assert_eq!(s.id(), 1);
        assert_eq!(s.name(), "Kitchen");
    }

    #[test]
    fn zone_normalises_corners() {
        let z = Zone::new(10, 20, 0, 5).expect("valid");
        assert_eq!(z.corners(), (0, 5, 10, 20));
        assert_eq!(z.area(), 150);
    }

    #[test]
    fn zone_rejects_zero_size() {
        assert_eq!(Zone::new(5, 5, 5, 10), Err(ZoneError::DegenerateRectangle));
        assert_eq!(Zone::new(0, 0, 10, 0), Err(ZoneError::DegenerateRectangle));
    }

    #[test]
    fn map_lookup_and_contains() {
        let m = home();
        assert!(m.contains(1));
        assert!(!m.contains(9));
        assert_eq!(m.segment(2).map(Segment::name), Some("Living room"));
        assert_eq!(m.segment(9), None);
    }

    #[test]
    fn validate_accepts_known_rooms() {
        let m = home();
        assert_eq!(m.validate_segments(&[1, 3]), Ok(vec![1, 3]));
    }

    #[test]
    fn validate_dedupes_and_sorts() {
        let m = home();
        assert_eq!(m.validate_segments(&[3, 1, 3, 1]), Ok(vec![1, 3]));
    }

    #[test]
    fn validate_rejects_empty_request() {
        let m = home();
        assert_eq!(m.validate_segments(&[]), Err(SegmentRequestError::Empty));
    }

    #[test]
    fn validate_reports_unknown_room_ids() {
        let m = home();
        assert_eq!(
            m.validate_segments(&[1, 9, 7]),
            Err(SegmentRequestError::UnknownSegments(vec![7, 9]))
        );
    }

    #[test]
    fn empty_map_rejects_any_room() {
        let m = VacuumMap::empty();
        assert_eq!(
            m.validate_segments(&[1]),
            Err(SegmentRequestError::UnknownSegments(vec![1]))
        );
    }
}
