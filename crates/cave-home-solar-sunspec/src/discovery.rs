// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec device model-chain discovery.
//!
//! Source: SunSpec Modbus Specification §A.3 — "Model Discovery".
//!
//! Layout on the wire (starting at one of `SUNSPEC_BASE_REGISTERS`):
//!
//! ```text
//!   word 0..1  SunSpec marker = 0x53756e53 ("SunS")
//!   word 2     Model ID (uint16)
//!   word 3     Model length (excluding header) (uint16)
//!   word 4..   Model payload of `length` words
//!   word 4+L   Next model ID …
//!   …
//!   word X     0xFFFF (end-of-chain sentinel)
//! ```

use crate::error::Result;
use serde::{Deserialize, Serialize};

/// Detected model header within a SunSpec device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelHeader {
    /// Model ID (e.g. 1, 101, 124).
    pub model_id: u16,
    /// Number of register words in the payload, excluding the 2-word
    /// header.
    pub length_regs: u16,
    /// Absolute base register address where this model's *payload*
    /// starts (i.e. immediately after its 2-word header).
    pub payload_base: u16,
}

/// A discovered model with header + raw payload registers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredModel {
    pub header: ModelHeader,
    pub payload: Vec<u16>,
}

/// Walk the SunSpec model chain, starting at the first register
/// after the marker, until either the end-of-chain sentinel
/// (`0xFFFF`) or the buffer runs out.
///
/// `chain_after_marker` must contain the register slice immediately
/// after the 32-bit `"SunS"` marker (i.e. starting at the first model
/// ID). `chain_base_address` is the absolute Modbus register address
/// of the first byte of `chain_after_marker[0]`, used to compute
/// payload-base addresses for downstream reads.
///
/// # Errors
///
/// [`crate::Error::LengthMismatch`] if a model declares more registers
/// than remain in the buffer.
pub fn discover_models(chain_after_marker: &[u16], chain_base_address: u16) -> Result<Vec<DiscoveredModel>> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    let mut absolute_base = chain_base_address;
    while idx + 1 < chain_after_marker.len() {
        let model_id = chain_after_marker[idx];
        if model_id == crate::SUNSPEC_END_MODEL_ID {
            break;
        }
        let length_regs = chain_after_marker[idx + 1];
        let payload_start = idx + 2;
        let payload_end = payload_start + length_regs as usize;
        if payload_end > chain_after_marker.len() {
            return Err(crate::Error::LengthMismatch {
                model_id,
                declared: length_regs,
                actual: (chain_after_marker.len() - payload_start) as u16,
            });
        }
        let payload_base = absolute_base + 2;
        let payload = chain_after_marker[payload_start..payload_end].to_vec();
        out.push(DiscoveredModel {
            header: ModelHeader {
                model_id,
                length_regs,
                payload_base,
            },
            payload,
        });
        idx = payload_end;
        absolute_base = absolute_base.saturating_add(2 + length_regs);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_chain_yields_empty_vec() {
        let m = discover_models(&[crate::SUNSPEC_END_MODEL_ID], 40_002).unwrap();
        assert!(m.is_empty());
    }

    #[test]
    fn one_model_one_register_payload() {
        // model 1, length 65, then end-of-chain
        let mut chain = vec![1u16, 65u16];
        chain.extend(std::iter::repeat(0u16).take(65));
        chain.push(crate::SUNSPEC_END_MODEL_ID);
        let m = discover_models(&chain, 40_002).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].header.model_id, 1);
        assert_eq!(m[0].header.length_regs, 65);
        assert_eq!(m[0].header.payload_base, 40_004);
        assert_eq!(m[0].payload.len(), 65);
    }

    #[test]
    fn two_models_then_end() {
        let mut chain = vec![1u16, 4u16, 0, 0, 0, 0]; // model 1, len 4
        chain.extend_from_slice(&[103u16, 50u16]);
        chain.extend(std::iter::repeat(0u16).take(50));
        chain.push(crate::SUNSPEC_END_MODEL_ID);
        let m = discover_models(&chain, 40_002).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].header.model_id, 1);
        assert_eq!(m[1].header.model_id, 103);
        assert_eq!(m[1].header.length_regs, 50);
        // m[0] header at base 40_002 ⇒ payload base 40_004
        assert_eq!(m[0].header.payload_base, 40_004);
        // m[1] header at base 40_002 + 2 + 4 = 40_008 ⇒ payload base 40_010
        assert_eq!(m[1].header.payload_base, 40_010);
    }

    #[test]
    fn length_mismatch_when_payload_truncated() {
        // model 1 declares 65 registers but only 2 follow.
        let chain = vec![1u16, 65u16, 0, 0];
        let r = discover_models(&chain, 40_002);
        assert!(matches!(r, Err(crate::Error::LengthMismatch { .. })));
    }

    #[test]
    fn marker_value_constant_is_suns() {
        assert_eq!(crate::SUNSPEC_MARKER, 0x5375_6e53);
        assert_eq!(crate::SUNSPEC_MARKER.to_be_bytes(), *b"SunS");
    }
}
