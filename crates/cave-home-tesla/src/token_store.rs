// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Credential persistence model.
//!
//! [`Secret`] wraps token material so it serialises transparently (for the
//! on-disk credential file) but **redacts in `Debug`/`Display`** — tracing a
//! [`Credentials`] can never leak a token. The actual file read/write (mode
//! `0600` under `~/.cave-home`) is the operational layer's I/O; this module
//! supplies the serialisation, the path and the mode constant.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// The owner-only file mode for the credential file.
pub const FILE_MODE: u32 = 0o600;

/// Secret string material that redacts in `Debug`/`Display` but serialises
/// transparently to its underlying value (so the credential file is plain).
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Wrap a secret value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Explicitly read the secret. The verbose name is the point: every call
    /// site that touches the raw token is greppable.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***redacted***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***redacted***")
    }
}

/// The persisted OAuth credentials for one Tesla account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// The bearer access token.
    pub access_token: Secret,
    /// The refresh token.
    pub refresh_token: Secret,
    /// Unix second at which the access token expires.
    pub expires_at_unix: u64,
    /// The Fleet API region key (`na` / `eu` / `cn`).
    pub region: String,
}

impl Credentials {
    /// Serialise to pretty JSON for the credential file.
    ///
    /// # Errors
    /// [`crate::TeslaError::Decode`] if serialisation fails (practically never).
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Parse from the credential-file JSON.
    ///
    /// # Errors
    /// [`crate::TeslaError::Decode`] on malformed JSON.
    pub fn from_json(s: &str) -> Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// Whether the access token is expired at `now_unix`, treating anything
    /// within `skew_secs` of expiry as expired.
    #[must_use]
    pub const fn is_expired(&self, now_unix: u64, skew_secs: u64) -> bool {
        now_unix.saturating_add(skew_secs) >= self.expires_at_unix
    }
}

/// The credential-file path under a home directory:
/// `<home>/.cave-home/tesla-credentials.json`.
#[must_use]
pub fn credentials_path(home: &Path) -> PathBuf {
    home.join(".cave-home").join("tesla-credentials.json")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn secret_debug_is_redacted() {
        let s = Secret::new("super-secret-token");
        assert!(!format!("{s:?}").contains("super-secret-token"));
        assert!(format!("{s:?}").contains("redacted"));
    }

    #[test]
    fn secret_display_is_redacted() {
        let s = Secret::new("super-secret-token");
        assert!(!format!("{s}").contains("super-secret-token"));
    }

    #[test]
    fn secret_exposes_value_explicitly() {
        let s = Secret::new("tok");
        assert_eq!(s.expose(), "tok");
    }

    #[test]
    fn secret_serialises_transparently() {
        let s = Secret::new("tok");
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"tok\"");
        let back: Secret = serde_json::from_str("\"tok\"").unwrap();
        assert_eq!(back.expose(), "tok");
    }

    #[test]
    fn credentials_roundtrip_json() {
        let c = Credentials {
            access_token: Secret::new("AT"),
            refresh_token: Secret::new("RT"),
            expires_at_unix: 4_600,
            region: "eu".into(),
        };
        let json = c.to_json().unwrap();
        let back = Credentials::from_json(&json).unwrap();
        assert_eq!(back.access_token.expose(), "AT");
        assert_eq!(back.expires_at_unix, 4_600);
        assert_eq!(back.region, "eu");
    }

    #[test]
    fn credentials_debug_never_leaks_tokens() {
        let c = Credentials {
            access_token: Secret::new("LEAKY-ACCESS"),
            refresh_token: Secret::new("LEAKY-REFRESH"),
            expires_at_unix: 0,
            region: "na".into(),
        };
        let dbg = format!("{c:?}");
        assert!(!dbg.contains("LEAKY-ACCESS"));
        assert!(!dbg.contains("LEAKY-REFRESH"));
    }

    #[test]
    fn credentials_expiry_uses_skew() {
        let c = Credentials {
            access_token: Secret::new("AT"),
            refresh_token: Secret::new("RT"),
            expires_at_unix: 1_000,
            region: "na".into(),
        };
        assert!(!c.is_expired(900, 60));
        assert!(c.is_expired(950, 60));
    }

    #[test]
    fn path_is_under_dot_cave_home() {
        let p = credentials_path(Path::new("/home/burak"));
        assert_eq!(
            p,
            Path::new("/home/burak/.cave-home/tesla-credentials.json")
        );
    }

    #[test]
    fn file_mode_is_owner_only() {
        assert_eq!(FILE_MODE, 0o600);
    }
}
