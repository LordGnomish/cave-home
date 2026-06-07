// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Error type for the free@home SysAP client.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_error_displays() {
        let e = FreeAtHomeError::Http("boom".into());
        assert_eq!(e.to_string(), "http transport error: boom");
    }

    #[test]
    fn auth_error_displays() {
        let e = FreeAtHomeError::Auth("bad credentials".into());
        assert_eq!(e.to_string(), "authentication failed: bad credentials");
    }

    #[test]
    fn is_a_std_error() {
        let e: Box<dyn std::error::Error> = Box::new(FreeAtHomeError::Config("x".into()));
        assert!(e.to_string().contains("configuration"));
    }
}
