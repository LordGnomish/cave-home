//! Port of `homeassistant.components.zone`.
//!
//! A zone is a circle on the globe (centre lat/long + radius in metres). A
//! point is *in* a zone when the great-circle distance to its centre is within
//! the radius. The registry's [`active_zone`](ZoneRegistry::active_zone) ports
//! HA's `async_active_zone`: of the non-`passive` zones containing a point, the
//! one with the smallest radius (the most specific) wins.

use crate::util::{ensure_unique_string, slugify};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ZoneError {
    #[error("zone name must not be empty")]
    EmptyName,
}

/// Port of a `zone` config entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    /// Radius in metres.
    pub radius: f64,
    /// A passive zone is used for naming a location but never claims presence
    /// (excluded from [`ZoneRegistry::active_zone`]).
    #[serde(default)]
    pub passive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl Zone {
    /// Great-circle distance in metres from this zone's centre to a point.
    #[must_use]
    pub fn distance(&self, latitude: f64, longitude: f64) -> f64 {
        haversine_m(self.latitude, self.longitude, latitude, longitude)
    }

    /// Whether a point lies within the zone's radius.
    #[must_use]
    pub fn contains(&self, latitude: f64, longitude: f64) -> bool {
        self.distance(latitude, longitude) <= self.radius
    }
}

/// Great-circle distance between two lat/long points, in metres.
fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;
    let phi1 = lat1.to_radians();
    let phi2 = lat2.to_radians();
    let d_phi = (lat2 - lat1).to_radians();
    let d_lambda = (lon2 - lon1).to_radians();
    let hav_lat = (d_phi / 2.0).sin().powi(2);
    let hav_lon = (d_lambda / 2.0).sin().powi(2);
    let a = (phi1.cos() * phi2.cos()).mul_add(hav_lon, hav_lat);
    2.0 * EARTH_RADIUS_M * a.sqrt().atan2((1.0 - a).sqrt())
}

#[derive(Default)]
struct ZoneInner {
    zones: BTreeMap<String, Zone>,
}

/// Registry of [`Zone`]s.
#[derive(Clone, Default)]
pub struct ZoneRegistry {
    inner: Arc<RwLock<ZoneInner>>,
}

impl ZoneRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a zone from a name (slug id, de-duplicated) and geometry.
    ///
    /// # Errors
    /// [`ZoneError::EmptyName`] if `name` slugs to nothing.
    pub fn create(
        &self,
        name: impl Into<String>,
        latitude: f64,
        longitude: f64,
        radius: f64,
    ) -> Result<Zone, ZoneError> {
        let name = name.into();
        let slug = slugify(&name);
        if slug.is_empty() {
            return Err(ZoneError::EmptyName);
        }
        let mut guard = self.inner.write();
        let existing: HashSet<String> = guard.zones.keys().cloned().collect();
        let id = ensure_unique_string(&slug, &existing);
        let zone = Zone {
            id: id.clone(),
            name,
            latitude,
            longitude,
            radius,
            passive: false,
            icon: None,
        };
        guard.zones.insert(id, zone.clone());
        Ok(zone)
    }

    /// Insert or replace a fully-specified zone (lets callers set `passive`).
    pub fn upsert(&self, zone: Zone) {
        self.inner.write().zones.insert(zone.id.clone(), zone);
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<Zone> {
        self.inner.read().zones.get(id).cloned()
    }

    #[must_use]
    pub fn delete(&self, id: &str) -> Option<Zone> {
        self.inner.write().zones.remove(id)
    }

    #[must_use]
    pub fn list(&self) -> Vec<Zone> {
        self.inner.read().zones.values().cloned().collect()
    }

    /// Port of `async_active_zone`: the most specific (smallest-radius)
    /// non-passive zone that contains the point, or `None`.
    #[must_use]
    pub fn active_zone(&self, latitude: f64, longitude: f64) -> Option<Zone> {
        self.inner
            .read()
            .zones
            .values()
            .filter(|z| !z.passive && z.contains(latitude, longitude))
            // Smallest radius = most specific zone. `total_cmp` orders the
            // f64 radii without an Ord/NaN hazard.
            .min_by(|a, b| a.radius.total_cmp(&b.radius))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_and_contains() {
        let z = Zone {
            id: "home".into(),
            name: "Home".into(),
            latitude: 52.0,
            longitude: 4.0,
            radius: 100.0,
            passive: false,
            icon: None,
        };
        // centre distance is zero
        assert!(z.distance(52.0, 4.0) < 1.0);
        assert!(z.contains(52.0, 4.0));
        // ~0.001 deg latitude north ≈ 111 m — outside a 100 m radius
        let d = z.distance(52.001, 4.0);
        assert!((d - 111.0).abs() < 5.0, "expected ~111 m, got {d}");
        assert!(!z.contains(52.001, 4.0));
        // a 150 m radius would include it
        let mut big = z.clone();
        big.radius = 150.0;
        assert!(big.contains(52.001, 4.0));
    }

    #[test]
    fn create_slugs_and_dedupes() {
        let reg = ZoneRegistry::new();
        let a = reg.create("Home Base", 52.0, 4.0, 100.0).expect("a");
        assert_eq!(a.id, "home_base");
        let b = reg.create("Home Base", 1.0, 1.0, 50.0).expect("b");
        assert_eq!(b.id, "home_base_2");
        assert_eq!(reg.list().len(), 2);
        assert!(reg.delete("home_base").is_some());
        assert!(reg.get("home_base").is_none());
    }

    #[test]
    fn empty_name_rejected() {
        let reg = ZoneRegistry::new();
        assert_eq!(reg.create("  ", 0.0, 0.0, 1.0).unwrap_err(), ZoneError::EmptyName);
    }

    #[test]
    fn active_zone_picks_smallest_containing_non_passive() {
        let reg = ZoneRegistry::new();
        // a big city zone and a small home zone share the same centre
        reg.upsert(Zone {
            id: "city".into(),
            name: "City".into(),
            latitude: 52.0,
            longitude: 4.0,
            radius: 5000.0,
            passive: false,
            icon: None,
        });
        reg.upsert(Zone {
            id: "home".into(),
            name: "Home".into(),
            latitude: 52.0,
            longitude: 4.0,
            radius: 100.0,
            passive: false,
            icon: None,
        });
        // a passive zone that also contains the point must be ignored
        reg.upsert(Zone {
            id: "region".into(),
            name: "Region".into(),
            latitude: 52.0,
            longitude: 4.0,
            radius: 10.0,
            passive: true,
            icon: None,
        });

        // standing at the centre: smallest *active* zone is home (100 m),
        // not the passive 10 m region
        let active = reg.active_zone(52.0, 4.0).expect("active");
        assert_eq!(active.id, "home");

        // far away: no zone contains the point
        assert!(reg.active_zone(0.0, 0.0).is_none());

        // 200 m north-ish: outside home, inside city → city wins
        let active2 = reg.active_zone(52.0018, 4.0).expect("active2");
        assert_eq!(active2.id, "city");
    }
}
