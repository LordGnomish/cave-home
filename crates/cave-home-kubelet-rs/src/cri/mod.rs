// SPDX-License-Identifier: Apache-2.0
//! CRI v1 client trait + in-memory mock.
//!
//! Phase 1 wires the kubelet against an injected `CriClient` trait.
//! `MockCriClient` provides a deterministic in-memory implementation for
//! tests. Real gRPC wiring lives in workspace-level integration code
//! (Phase 1b — recorded in `parity.manifest.toml`).

pub mod client;
pub mod mock;
pub mod types;

pub use client::{CriClient, CriError, CriResult};
pub use mock::MockCriClient;
