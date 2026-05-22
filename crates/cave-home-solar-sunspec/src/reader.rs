// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Async Modbus-TCP read facade over a vendor-agnostic [`ModbusRead`]
//! trait. The trait abstracts the transport so cave-home can unit-test
//! the discovery + parse pipeline with an in-memory mock register
//! bank, and ship a real `tokio-modbus`-backed implementation in
//! `cave-home-binary` as a thin adapter.

use crate::common::CommonModel;
use crate::discovery::{DiscoveredModel, discover_models};
use crate::error::Result;
use async_trait::async_trait;
use std::collections::HashMap;

/// Async Modbus-TCP read trait. Each call returns `count` registers
/// starting at `address`. Source: SunSpec spec §B.1 — devices use
/// Modbus function 3 (read holding registers).
#[async_trait]
pub trait ModbusRead: Send + Sync {
    /// Read `count` 16-bit holding registers at `address`.
    async fn read_holding(&self, address: u16, count: u16) -> Result<Vec<u16>>;
}

/// SunSpec reader — orchestrates the marker probe + model-chain walk
/// against an arbitrary [`ModbusRead`] transport.
#[derive(Debug)]
pub struct SunSpecReader<T: ModbusRead> {
    pub transport: T,
}

impl<T: ModbusRead> SunSpecReader<T> {
    pub const fn new(transport: T) -> Self {
        Self { transport }
    }

    /// Probe the three well-known base addresses for the SunSpec
    /// marker. Returns the base address that succeeded, or
    /// [`crate::Error::MarkerNotFound`].
    pub async fn probe_marker(&self) -> Result<u16> {
        for base in crate::SUNSPEC_BASE_REGISTERS.iter().copied() {
            // Read the 2-register marker.
            let regs = match self.transport.read_holding(base, 2).await {
                Ok(v) => v,
                Err(_) => continue,
            };
            if regs.len() < 2 {
                continue;
            }
            let marker = (u32::from(regs[0]) << 16) | u32::from(regs[1]);
            if marker == crate::SUNSPEC_MARKER {
                return Ok(base);
            }
        }
        Err(crate::Error::MarkerNotFound)
    }

    /// Probe marker, then walk the full chain. Returns the
    /// discovered models in order.
    pub async fn discover_chain(&self, max_chain_regs: u16) -> Result<Vec<DiscoveredModel>> {
        let marker_base = self.probe_marker().await?;
        let chain_base = marker_base + 2;
        let regs = self.transport.read_holding(chain_base, max_chain_regs).await?;
        discover_models(&regs, chain_base)
    }

    /// Convenience: discover + parse Model 1 if present.
    pub async fn read_common(&self) -> Result<CommonModel> {
        let models = self.discover_chain(256).await?;
        let common = models
            .iter()
            .find(|m| m.header.model_id == CommonModel::MODEL_ID)
            .ok_or(crate::Error::UnsupportedModel(CommonModel::MODEL_ID))?;
        CommonModel::parse(&common.payload)
    }
}

/// In-memory `ModbusRead` mock — register bank addressable by absolute
/// register address. Useful for unit tests; not for production.
#[derive(Debug, Default, Clone)]
pub struct MockRegisters {
    pub bank: HashMap<u16, u16>,
}

impl MockRegisters {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, address: u16, regs: &[u16]) {
        for (i, v) in regs.iter().enumerate() {
            self.bank.insert(address + i as u16, *v);
        }
    }
}

#[async_trait]
impl ModbusRead for MockRegisters {
    async fn read_holding(&self, address: u16, count: u16) -> Result<Vec<u16>> {
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            let addr = address.checked_add(i).ok_or_else(|| {
                crate::Error::Transport(format!("register overflow at {address}+{i}"))
            })?;
            out.push(self.bank.get(&addr).copied().unwrap_or(0xFFFF));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::InverterFamily;

    fn build_marker_at(addr: u16) -> MockRegisters {
        let mut m = MockRegisters::new();
        m.set(addr, &[0x5375, 0x6e53]); // "SunS"
        m
    }

    #[tokio::test]
    async fn probe_marker_at_40000() {
        let m = build_marker_at(40_000);
        let r = SunSpecReader::new(m);
        assert_eq!(r.probe_marker().await.unwrap(), 40_000);
    }

    #[tokio::test]
    async fn probe_marker_at_50000() {
        let m = build_marker_at(50_000);
        let r = SunSpecReader::new(m);
        assert_eq!(r.probe_marker().await.unwrap(), 50_000);
    }

    #[tokio::test]
    async fn probe_marker_not_found_when_absent() {
        let m = MockRegisters::new();
        let r = SunSpecReader::new(m);
        assert!(matches!(r.probe_marker().await, Err(crate::Error::MarkerNotFound)));
    }

    #[tokio::test]
    async fn discover_chain_returns_models() {
        let mut m = build_marker_at(40_000);
        // After marker at 40_002, write model 1 (len=65) then end sentinel
        let mut chain = vec![1u16, 65u16];
        chain.extend(std::iter::repeat(0u16).take(65));
        chain.push(crate::SUNSPEC_END_MODEL_ID);
        m.set(40_002, &chain);
        let r = SunSpecReader::new(m);
        let models = r.discover_chain(128).await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].header.model_id, 1);
    }

    #[tokio::test]
    async fn read_common_full_pipeline() {
        let mut m = build_marker_at(40_000);
        // model 1 payload — 65 registers. Just put manufacturer at offset 0.
        let mut chain = vec![1u16, 65u16];
        // 16 registers of "Fronius" ASCII
        let mfr = b"Fronius\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
        let mfr_regs: Vec<u16> = mfr
            .chunks(2)
            .take(16)
            .map(|c| (u16::from(c[0]) << 8) | u16::from(c[1]))
            .collect();
        chain.extend(mfr_regs);
        // Pad payload to 65 registers
        while chain.len() < 2 + 65 {
            chain.push(0);
        }
        chain.push(crate::SUNSPEC_END_MODEL_ID);
        m.set(40_002, &chain);
        let r = SunSpecReader::new(m);
        let c = r.read_common().await.unwrap();
        assert_eq!(c.manufacturer, "Fronius");
        assert_eq!(c.family, InverterFamily::Fronius);
    }
}
