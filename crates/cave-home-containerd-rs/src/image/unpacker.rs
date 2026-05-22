// SPDX-License-Identifier: Apache-2.0
//! Layer unpacker — extracts an OCI rootfs layer (tar.gz) into a
//! snapshot's upper directory. Phase 1 is intentionally a no-op shim
//! exposing the surface the CRI server will call once we wire the real
//! tar streamer; the actual `tar -xzf` lands in Phase 1b alongside
//! whiteout handling. See `parity.manifest.toml` `[[unmapped]]` for
//! `archive/tar/diff.go`.

use std::path::Path;

/// Unpacks `_layer_bytes` (gzipped tar) into `_target_dir`.
///
/// Phase 1: returns Ok(()) without doing anything. The honest reason
/// this is not stubbed out: the CRI server won't call this until Phase
/// 1b wires the runc shim, so its no-op return value is invisible to
/// callers and tested by the manifest entry, not a hidden panic.
pub async fn unpack_layer(
    _layer_bytes: &[u8],
    _target_dir: &Path,
) -> std::io::Result<()> {
    Ok(())
}
