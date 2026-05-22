// SPDX-License-Identifier: Apache-2.0
//! cave-home-containerd-rs — line-by-line port of containerd v2.3.0
//! (Orchestration Phase 1, ADR-004). See parity.manifest.toml for the
//! mapped/unmapped manifest and ROADMAP M2.5.
//!
//! Module layout mirrors the upstream Go package boundaries as closely as
//! Rust allows:
//!
//!   * [`cri`]         — CRI v1 gRPC server skeleton + in-memory stores.
//!   * [`content`]     — content-addressable blob store (sha256).
//!   * [`snapshots`]   — overlayfs snapshotter (metadata + fs layout).
//!   * [`image`]       — OCI distribution-spec resolver, bearer auth, unpack.
//!   * [`server`]      — tonic Server::builder bootstrap (lib-side; no main).

pub mod content;
pub mod cri;
pub mod image;
pub mod server;
pub mod snapshots;

/// Generated CRI v1 gRPC types — `runtime.v1` package.
///
/// Emitted by `build.rs` via `tonic-build` from `proto/runtime/v1/api.proto`,
/// vendored verbatim from containerd v2.3.0's
/// `vendor/k8s.io/cri-api/pkg/apis/runtime/v1/api.proto`.
#[allow(clippy::all, clippy::pedantic, clippy::nursery)]
pub mod runtime_v1 {
    tonic::include_proto!("runtime.v1");
}
