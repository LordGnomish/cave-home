// SPDX-License-Identifier: Apache-2.0
//! Per-scheduling-cycle state bag passed between plugin extension points.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         pkg/scheduler/framework/cycle_state.go

use std::any::Any;
use std::collections::HashMap;

/// Opaque per-cycle key. Upstream defines `framework.StateKey` (alias for
/// `string`); we mirror the alias so plugin authors get the same look.
pub type StateKey = String;

/// Upstream: `pkg/scheduler/framework/cycle_state.go::CycleState`.
///
/// Plugin extension points share state via this map. Only one cycle
/// runs at a time per scheduling thread, so a plain hash map is enough
/// (upstream's sync.Map is for the same reason kept thread-local).
#[derive(Default)]
pub struct CycleState {
    storage: HashMap<StateKey, Box<dyn Any + Send + Sync>>,
}

impl CycleState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Upstream: `CycleState.Write`.
    pub fn write<T: 'static + Send + Sync>(&mut self, key: impl Into<StateKey>, value: T) {
        self.storage.insert(key.into(), Box::new(value));
    }

    /// Upstream: `CycleState.Read`.
    #[must_use]
    pub fn read<T: 'static>(&self, key: &str) -> Option<&T> {
        self.storage.get(key).and_then(|b| b.downcast_ref::<T>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read_returns_value() {
        let mut s = CycleState::new();
        s.write("k", 42_i64);
        assert_eq!(s.read::<i64>("k"), Some(&42));
    }

    #[test]
    fn read_missing_returns_none() {
        let s = CycleState::new();
        assert!(s.read::<i64>("nope").is_none());
    }

    #[test]
    fn read_with_wrong_type_returns_none() {
        let mut s = CycleState::new();
        s.write("k", "hello".to_string());
        assert!(s.read::<i64>("k").is_none());
    }
}
