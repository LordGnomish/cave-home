// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Attribute reporting — ZCL §2.4.11 (Report Attributes).
//!
//! A Zigbee device tells its bound peer "attribute X changed to Y" by
//! sending a Report Attributes (0x0a) frame inside the cluster. Phase 1
//! decodes the inbound report, dedupes it (so repeated 0.5 °C samples
//! from a thermostat don't spam the bus), and emits a [`Reported`]
//! event the automation engine consumes.

use std::collections::HashMap;

use crate::error::{Result, ZigbeeError};
use crate::zcl::data_type::{AttributeValue, ZclDataType};

/// One attribute reported by a device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reported {
    /// Device IEEE address.
    pub device_ieee: u64,
    /// Cluster ID the report came from.
    pub cluster_id: u16,
    /// Attribute ID.
    pub attribute_id: u16,
    /// Reported value.
    pub value: AttributeValue,
}

/// Report Attributes payload — ZCL §2.4.11.1.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportAttributes {
    /// (attribute_id, value) tuples.
    pub records: Vec<(u16, AttributeValue)>,
}

impl ReportAttributes {
    /// Encode as ZCL payload.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (id, v) in &self.records {
            out.extend_from_slice(&id.to_le_bytes());
            out.push(v.data_type() as u8);
            out.extend_from_slice(&v.encode_value());
        }
        out
    }

    /// Decode a Report Attributes payload.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] or [`ZigbeeError::Zcl`].
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut i = 0;
        let mut records = Vec::new();
        while i < payload.len() {
            if payload.len() < i + 3 {
                return Err(ZigbeeError::Truncated {
                    need: i + 3,
                    have: payload.len(),
                });
            }
            let id = u16::from_le_bytes([payload[i], payload[i + 1]]);
            let dt = ZclDataType::from_byte(payload[i + 2])?;
            i += 3;
            let (v, used) = AttributeValue::decode(dt, &payload[i..])?;
            i += used;
            records.push((id, v));
        }
        Ok(Self { records })
    }
}

/// In-process attribute-report deduplicator.
///
/// Tracks the latest (device, cluster, attribute) → value tuple; a new
/// report with the same value is dropped (returns `None` from
/// [`ReportDeduper::observe`]).
#[derive(Clone, Debug, Default)]
pub struct ReportDeduper {
    last: HashMap<(u64, u16, u16), AttributeValue>,
}

impl ReportDeduper {
    /// Construct an empty dedup table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Observe a report. Returns `Some(report)` if it's new (value
    /// changed since the last report, or attribute was previously
    /// unseen). Returns `None` if it's a duplicate.
    pub fn observe(
        &mut self,
        device_ieee: u64,
        cluster_id: u16,
        attribute_id: u16,
        value: AttributeValue,
    ) -> Option<Reported> {
        let key = (device_ieee, cluster_id, attribute_id);
        let changed = match self.last.get(&key) {
            Some(prev) => prev != &value,
            None => true,
        };
        if changed {
            self.last.insert(key, value.clone());
            Some(Reported {
                device_ieee,
                cluster_id,
                attribute_id,
                value,
            })
        } else {
            None
        }
    }

    /// Number of distinct (device, cluster, attribute) triples tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.last.len()
    }

    /// `true` ⇔ nothing has been observed yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.last.is_empty()
    }

    /// Clear the dedup state for a device (e.g. when it leaves the network).
    pub fn forget_device(&mut self, device_ieee: u64) {
        self.last.retain(|(ieee, _, _), _| *ieee != device_ieee);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_attributes_round_trip() {
        let r = ReportAttributes {
            records: vec![
                (0x0000, AttributeValue::Uint8(7)),
                (0x0010, AttributeValue::Int16(-100)),
            ],
        };
        let bytes = r.encode();
        let decoded = ReportAttributes::decode(&bytes).unwrap();
        assert_eq!(decoded, r);
    }

    #[test]
    fn dedup_emits_first_value() {
        let mut d = ReportDeduper::new();
        let v = d.observe(0xaaaa, 0x0402, 0x0000, AttributeValue::Int16(2300));
        assert!(v.is_some());
    }

    #[test]
    fn dedup_drops_repeated_value() {
        let mut d = ReportDeduper::new();
        d.observe(0xaaaa, 0x0402, 0x0000, AttributeValue::Int16(2300));
        let second = d.observe(0xaaaa, 0x0402, 0x0000, AttributeValue::Int16(2300));
        assert!(second.is_none());
    }

    #[test]
    fn dedup_emits_changed_value() {
        let mut d = ReportDeduper::new();
        d.observe(0xaaaa, 0x0402, 0x0000, AttributeValue::Int16(2300));
        let v = d.observe(0xaaaa, 0x0402, 0x0000, AttributeValue::Int16(2400));
        assert!(v.is_some());
    }

    #[test]
    fn dedup_tracks_per_attribute() {
        let mut d = ReportDeduper::new();
        d.observe(0xaaaa, 0x0402, 0x0000, AttributeValue::Int16(2300));
        let v = d.observe(0xaaaa, 0x0402, 0x0001, AttributeValue::Int16(0));
        assert!(v.is_some(), "different attribute = not duplicate");
    }

    #[test]
    fn forget_device_drops_only_that_device() {
        let mut d = ReportDeduper::new();
        d.observe(0xaaaa, 0x0402, 0x0000, AttributeValue::Int16(2300));
        d.observe(0xbbbb, 0x0402, 0x0000, AttributeValue::Int16(2300));
        d.forget_device(0xaaaa);
        // Re-observing aaaa should yield a fresh entry.
        let v = d.observe(0xaaaa, 0x0402, 0x0000, AttributeValue::Int16(2300));
        assert!(v.is_some());
        assert_eq!(d.len(), 2);
    }
}
