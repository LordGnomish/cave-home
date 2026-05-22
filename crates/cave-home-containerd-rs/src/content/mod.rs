// SPDX-License-Identifier: Apache-2.0
//! Content-addressable blob store — line-by-line port of containerd's
//! `plugins/content/local/store.go`. Phase 1: sha256-only, ingest +
//! verify-on-write + walk; no fsverity, no resumable ingest (those land
//! in Phase 1b — see `parity.manifest.toml`).

mod store;

pub use store::{Digest, Info, Store, StoreError};
