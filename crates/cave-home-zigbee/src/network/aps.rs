// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Application Support Sub-layer (APS) primitives — Zigbee 3.0 §2.4.
//!
//! Phase 1 models:
//! - [`ApsDataRequest`] (§2.4.3.1) — the principal "send a ZCL frame"
//!   primitive used by every cluster operation; encoded into either an
//!   EZSP `sendUnicast`/`sendMulticast` parameter blob or a deCONZ
//!   APSDE-DATA.request frame upstream of this module.
//! - [`ApsmePrimitive`] (§2.4.4) — APSME GET / SET / BIND / UNBIND used
//!   for binding-table management.

use crate::error::{Result, ZigbeeError};

/// APS addressing mode — §2.4.3.1.2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ApsAddressMode {
    /// 16-bit group address.
    Group = 0x01,
    /// 16-bit short network address + destination endpoint.
    Short = 0x02,
    /// 64-bit IEEE long address + destination endpoint.
    Ieee = 0x03,
}

/// 64-bit IEEE address — exposed to UI as a hidden detail; the
/// grandma-friendly Portal terms wrap this as "Cihaz" (device).
pub type IeeeAddress = u64;

/// APSDE-DATA.request — §2.4.3.1. The principal outbound APS primitive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApsDataRequest {
    /// Destination addressing mode.
    pub dest_address_mode: ApsAddressMode,
    /// Destination address (16-bit short or 64-bit IEEE, both stored as u64).
    pub dest_address: u64,
    /// Destination endpoint (1..=240; ignored for group multicast).
    pub dest_endpoint: u8,
    /// 16-bit ZCL profile identifier (0x0104 = Home Automation; 0xc05e = ZLL).
    pub profile_id: u16,
    /// Cluster identifier (e.g. 0x0006 = OnOff).
    pub cluster_id: u16,
    /// Source endpoint on the coordinator (1..=240; 0 reserved for ZDO).
    pub source_endpoint: u8,
    /// ASDU — the serialised ZCL frame body.
    pub asdu: Vec<u8>,
    /// TX options bitmap — §2.4.3.1.4.
    pub tx_options: u8,
    /// Maximum number of hops (0 = use NIB nwkMaxDepth).
    pub radius: u8,
}

impl ApsDataRequest {
    /// Construct a unicast APSDE-DATA.request to a 16-bit short address.
    #[must_use]
    pub fn unicast(
        short_addr: u16,
        dest_endpoint: u8,
        profile_id: u16,
        cluster_id: u16,
        source_endpoint: u8,
        asdu: Vec<u8>,
    ) -> Self {
        Self {
            dest_address_mode: ApsAddressMode::Short,
            dest_address: u64::from(short_addr),
            dest_endpoint,
            profile_id,
            cluster_id,
            source_endpoint,
            asdu,
            tx_options: 0,
            radius: 0,
        }
    }

    /// Construct a group-multicast APSDE-DATA.request.
    #[must_use]
    pub fn group(
        group_id: u16,
        profile_id: u16,
        cluster_id: u16,
        source_endpoint: u8,
        asdu: Vec<u8>,
    ) -> Self {
        Self {
            dest_address_mode: ApsAddressMode::Group,
            dest_address: u64::from(group_id),
            // Group casts ignore endpoint per §2.4.3.1.
            dest_endpoint: 0xff,
            profile_id,
            cluster_id,
            source_endpoint,
            asdu,
            tx_options: 0,
            radius: 0,
        }
    }

    /// Validate constraints documented in §2.4.3.1 (returns a list of
    /// failing checks, empty ⇒ valid).
    #[must_use]
    pub fn validate(&self) -> Vec<&'static str> {
        let mut errs = Vec::new();
        if self.dest_address_mode != ApsAddressMode::Group
            && !(1..=240).contains(&self.dest_endpoint)
        {
            errs.push("dest_endpoint must be 1..=240 for unicast");
        }
        if !(1..=240).contains(&self.source_endpoint) {
            errs.push("source_endpoint must be 1..=240");
        }
        if self.asdu.is_empty() {
            errs.push("asdu must be non-empty");
        }
        if self.asdu.len() > 128 {
            errs.push("asdu exceeds Zigbee 3.0 §2.4.3.1 max payload");
        }
        errs
    }
}

/// APSME primitive — §2.4.4.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApsmePrimitive {
    /// APSME-GET.request — read one APIB attribute.
    Get { attribute_id: u16 },
    /// APSME-SET.request — write one APIB attribute (raw value bytes).
    Set { attribute_id: u16, value: Vec<u8> },
    /// APSME-BIND.request — add a (src-ep, cluster, dst-ep) binding.
    Bind {
        src_ieee: IeeeAddress,
        src_endpoint: u8,
        cluster_id: u16,
        dst_ieee: IeeeAddress,
        dst_endpoint: u8,
    },
    /// APSME-UNBIND.request — drop the same binding.
    Unbind {
        src_ieee: IeeeAddress,
        src_endpoint: u8,
        cluster_id: u16,
        dst_ieee: IeeeAddress,
        dst_endpoint: u8,
    },
}

impl ApsmePrimitive {
    /// Validate the primitive's invariants per §2.4.4.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Network`] for an obviously bogus value
    /// (e.g. cluster ID 0xffff which is reserved).
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Bind {
                src_endpoint,
                dst_endpoint,
                cluster_id,
                ..
            }
            | Self::Unbind {
                src_endpoint,
                dst_endpoint,
                cluster_id,
                ..
            } => {
                if !(1..=240).contains(src_endpoint) {
                    return Err(ZigbeeError::Network(
                        "src_endpoint must be 1..=240".into(),
                    ));
                }
                if !(1..=240).contains(dst_endpoint) {
                    return Err(ZigbeeError::Network(
                        "dst_endpoint must be 1..=240".into(),
                    ));
                }
                if *cluster_id == 0xffff {
                    return Err(ZigbeeError::Network("cluster_id 0xffff reserved".into()));
                }
                Ok(())
            }
            Self::Get { .. } | Self::Set { .. } => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicast_constructor_sets_short_addressing() {
        let req = ApsDataRequest::unicast(0x1234, 1, 0x0104, 0x0006, 1, vec![0x01, 0x02]);
        assert_eq!(req.dest_address_mode, ApsAddressMode::Short);
        assert_eq!(req.dest_address, 0x1234);
        assert!(req.validate().is_empty());
    }

    #[test]
    fn group_constructor_sets_group_addressing() {
        let req = ApsDataRequest::group(0x0001, 0x0104, 0x0006, 1, vec![0x01]);
        assert_eq!(req.dest_address_mode, ApsAddressMode::Group);
        assert!(req.validate().is_empty());
    }

    #[test]
    fn empty_asdu_fails_validation() {
        let req = ApsDataRequest::unicast(0x1234, 1, 0x0104, 0x0006, 1, vec![]);
        let errs = req.validate();
        assert!(errs.iter().any(|e| e.contains("asdu must be non-empty")));
    }

    #[test]
    fn out_of_range_endpoint_fails_validation() {
        let req = ApsDataRequest::unicast(0x1234, 0, 0x0104, 0x0006, 1, vec![0x01]);
        let errs = req.validate();
        assert!(errs.iter().any(|e| e.contains("dest_endpoint")));
    }

    #[test]
    fn bind_validates_endpoints() {
        let p = ApsmePrimitive::Bind {
            src_ieee: 0,
            src_endpoint: 0,
            cluster_id: 0x0006,
            dst_ieee: 0,
            dst_endpoint: 1,
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn bind_with_valid_endpoints_ok() {
        let p = ApsmePrimitive::Bind {
            src_ieee: 0xabcd,
            src_endpoint: 1,
            cluster_id: 0x0006,
            dst_ieee: 0xef01,
            dst_endpoint: 1,
        };
        assert!(p.validate().is_ok());
    }

    #[test]
    fn get_set_pass_validation() {
        let g = ApsmePrimitive::Get { attribute_id: 0x01 };
        let s = ApsmePrimitive::Set {
            attribute_id: 0x01,
            value: vec![0xff],
        };
        assert!(g.validate().is_ok());
        assert!(s.validate().is_ok());
    }
}
