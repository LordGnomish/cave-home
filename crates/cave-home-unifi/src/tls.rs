// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! TLS trust for the console transports — re-exported from
//! [`cave_home_tls_pin`] so callers (the CLI) configure pinning without a
//! second dependency.

pub use cave_home_tls_pin::{
    capture_presented_certificate, CaptureError, Pin, PinParseError, PinSet, PresentedCertificate,
    Sha256Fingerprint, TlsTrust, TrustError,
};
