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
