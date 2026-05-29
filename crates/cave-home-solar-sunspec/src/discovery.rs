// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! SunSpec device model-chain discovery.
//!
//! Source: SunSpec Information Model Specification — "Model Discovery".
//!
//! A SunSpec device lays its models out as one contiguous register block:
//!
//! ```text
//!   word 0..1  SunSpec marker = 0x53756e53 ("SunS")
//!   word 2     Model ID (uint16)
//!   word 3     Model length, in registers, excluding this 2-word header
//!   word 4..   Model payload of `length` words
//!   word 4+L   Next model ID …
//!   …
//!   word X     0xFFFF (end-of-chain sentinel)
//! ```
//!
//! cave-home reads the whole block once, then walks the `(id, length)`
//! headers to slice out each model's payload. The walk is pure index
//! arithmetic over a `&[u16]` — no hardware, no network.

use crate::fault::DecodeError;

/// The 32-bit SunSpec identifier marker — ASCII `"SunS"`.
pub const SUNSPEC_MARKER: u32 = 0x5375_6e53;

/// End-of-chain sentinel: a model id of `0xFFFF` means "no more models".
pub const SUNSPEC_END_MODEL_ID: u16 = 0xFFFF;

/// One discovered model: its id, its declared register length, and a slice
/// of the underlying block holding just its payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveredModel<'a> {
    /// Model id (e.g. 1, 101, 103, 111).
    pub model_id: u16,
    /// Payload length in registers, excluding the 2-word header.
    pub length: u16,
    /// The payload registers (length == `length`).
    pub payload: &'a [u16],
}

/// Verify the `"SunS"` marker sits at the start of `block` and return the
/// register slice that begins at the first model header (i.e. `block[2..]`).
///
/// # Errors
/// [`DecodeError::OutOfBounds`] if `block` is shorter than the 2-word marker.
/// [`DecodeError::MissingMarker`] if the marker bytes do not match.
pub fn check_marker(block: &[u16]) -> Result<&[u16], DecodeError> {
    if block.len() < 2 {
        return Err(DecodeError::OutOfBounds { offset: 2, len: block.len() });
    }
    let marker = (u32::from(block[0]) << 16) | u32::from(block[1]);
    if marker != SUNSPEC_MARKER {
        return Err(DecodeError::MissingMarker);
    }
    Ok(&block[2..])
}

/// Walk the model chain that follows the marker.
///
/// `chain` is the register slice that begins at the first model id (i.e. the
/// output of [`check_marker`]). Walking stops at the end-of-chain sentinel or
/// when the buffer is exhausted.
///
/// # Errors
/// [`DecodeError::LengthMismatch`] if a model declares more registers than
/// remain in the buffer — we reject rather than read past the block.
pub fn walk_chain(chain: &[u16]) -> Result<Vec<DiscoveredModel<'_>>, DecodeError> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    // Need at least id + length to read a header.
    while idx + 1 < chain.len() {
        let model_id = chain[idx];
        if model_id == SUNSPEC_END_MODEL_ID {
            break;
        }
        let length = chain[idx + 1];
        let payload_start = idx + 2;
        let payload_end = payload_start + length as usize;
        if payload_end > chain.len() {
            return Err(DecodeError::LengthMismatch {
                model_id,
                declared: length,
                available: (chain.len() - payload_start) as u16,
            });
        }
        out.push(DiscoveredModel {
            model_id,
            length,
            payload: &chain[payload_start..payload_end],
        });
        idx = payload_end;
    }
    Ok(out)
}

/// Discover all models in a full SunSpec block (marker included). Convenience
/// wrapper combining [`check_marker`] and [`walk_chain`].
///
/// # Errors
/// Propagates [`check_marker`] and [`walk_chain`] errors.
pub fn discover(block: &[u16]) -> Result<Vec<DiscoveredModel<'_>>, DecodeError> {
    let chain = check_marker(block)?;
    walk_chain(chain)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `[u16]` block: marker, then the given `(id, payload)` models,
    /// then the end sentinel.
    fn build_block(models: &[(u16, &[u16])]) -> Vec<u16> {
        let mut b = vec![(SUNSPEC_MARKER >> 16) as u16, (SUNSPEC_MARKER & 0xFFFF) as u16];
        for (id, payload) in models {
            b.push(*id);
            b.push(payload.len() as u16);
            b.extend_from_slice(payload);
        }
        b.push(SUNSPEC_END_MODEL_ID);
        b
    }

    #[test]
    fn marker_constant_is_suns() {
        assert_eq!(SUNSPEC_MARKER, 0x5375_6e53);
        assert_eq!(SUNSPEC_MARKER.to_be_bytes(), *b"SunS");
    }

    #[test]
    fn check_marker_accepts_and_rejects() {
        let good = build_block(&[]);
        assert!(check_marker(&good).is_ok());
        let bad = [0x0000u16, 0x0000, SUNSPEC_END_MODEL_ID];
        assert_eq!(check_marker(&bad), Err(DecodeError::MissingMarker));
        let tiny = [0x5375u16];
        assert!(matches!(check_marker(&tiny), Err(DecodeError::OutOfBounds { .. })));
    }

    #[test]
    fn empty_chain_yields_no_models() {
        let block = build_block(&[]);
        assert!(discover(&block).unwrap().is_empty());
    }

    #[test]
    fn single_model_walk() {
        let payload = [0u16; 65];
        let block = build_block(&[(1, &payload)]);
        let models = discover(&block).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_id, 1);
        assert_eq!(models[0].length, 65);
        assert_eq!(models[0].payload.len(), 65);
    }

    #[test]
    fn two_model_chain_in_order() {
        let common = [0u16; 4];
        let inv = [0u16; 50];
        let block = build_block(&[(1, &common), (103, &inv)]);
        let models = discover(&block).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].model_id, 1);
        assert_eq!(models[1].model_id, 103);
        assert_eq!(models[1].length, 50);
    }

    #[test]
    fn declared_length_past_buffer_is_rejected() {
        // model 1 claims 65 registers but only 2 follow before the buffer ends.
        let chain = [1u16, 65, 0, 0];
        assert!(matches!(
            walk_chain(&chain),
            Err(DecodeError::LengthMismatch { model_id: 1, declared: 65, .. })
        ));
    }
}
