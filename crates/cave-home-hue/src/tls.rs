// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Bridge TLS trust — re-exported from [`cave_home_tls_pin`] so callers
//! configure pinning without a second dependency. See
//! [`crate::v2::transport::signify_bridge_trust`] for the Signify-root path.

pub use cave_home_tls_pin::{
    capture_presented_certificate, CaptureError, Pin, PinParseError, PinSet, PresentedCertificate,
    Sha256Fingerprint, TlsTrust, TrustError,
};
