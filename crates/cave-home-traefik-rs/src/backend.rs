// SPDX-License-Identifier: Apache-2.0
//! Reverse-proxy backend: building the upstream request line.
//!
//! Given a chosen backend [`crate::loadbalancer::Server`] URL, the
//! (post-middleware) request path and the original query string, this produces
//! the absolute URI the proxy dials. The async forwarding engine (retries,
//! circuit breaking, connection pooling) lives in [`crate::server`] and builds
//! on [`crate::retry`] / [`crate::circuit`]; the URI assembly here is pure and
//! independently testable.

use http::Uri;

/// An error assembling the upstream request URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    /// The backend server URL could not be parsed into scheme + authority.
    InvalidServer(String),
    /// The assembled target URI was not a valid URI.
    InvalidTarget(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidServer(s) => write!(f, "invalid backend server url: {s}"),
            Self::InvalidTarget(s) => write!(f, "invalid upstream target uri: {s}"),
        }
    }
}

impl std::error::Error for BackendError {}

/// Assemble the absolute upstream URI from a backend server URL, a request path
/// and an optional query string.
///
/// A server URL without a scheme is assumed to be `http://`. The backend's own
/// path component (rare) is ignored; the router/middleware-chosen `path` wins,
/// matching Traefik's behaviour of routing to `scheme://authority` + the
/// rewritten request path.
///
/// # Errors
/// Returns [`BackendError`] if the server URL has no authority or the assembled
/// URI does not parse.
pub fn upstream_uri(
    server_url: &str,
    path: &str,
    query: Option<&str>,
) -> Result<Uri, BackendError> {
    let normalized = if server_url.contains("://") {
        server_url.to_string()
    } else {
        format!("http://{server_url}")
    };
    let base: Uri = normalized
        .parse()
        .map_err(|_| BackendError::InvalidServer(server_url.to_string()))?;
    let scheme = base.scheme_str().unwrap_or("http");
    let authority = base
        .authority()
        .ok_or_else(|| BackendError::InvalidServer(server_url.to_string()))?
        .as_str();

    let path_and_query = match query {
        Some(q) if !q.is_empty() => format!("{path}?{q}"),
        _ => path.to_string(),
    };
    let target = format!("{scheme}://{authority}{path_and_query}");
    target
        .parse()
        .map_err(|_| BackendError::InvalidTarget(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn assembles_scheme_authority_path_query() -> TestResult {
        let uri = upstream_uri("http://10.0.0.2:8080", "/users", Some("q=1"))?;
        assert_eq!(uri.to_string(), "http://10.0.0.2:8080/users?q=1");
        Ok(())
    }

    #[test]
    fn root_path_no_query() -> TestResult {
        let uri = upstream_uri("http://h:80", "/", None)?;
        assert_eq!(uri.to_string(), "http://h:80/");
        Ok(())
    }

    #[test]
    fn empty_query_is_omitted() -> TestResult {
        let uri = upstream_uri("https://api.svc", "/v1", Some(""))?;
        assert_eq!(uri.to_string(), "https://api.svc/v1");
        Ok(())
    }

    #[test]
    fn missing_scheme_defaults_to_http() -> TestResult {
        let uri = upstream_uri("10.0.0.9:3000", "/x", None)?;
        assert_eq!(uri.to_string(), "http://10.0.0.9:3000/x");
        Ok(())
    }

    #[test]
    fn preserves_https_scheme() -> TestResult {
        let uri = upstream_uri("https://secure:8443", "/a/b", None)?;
        assert_eq!(uri.scheme_str(), Some("https"));
        assert_eq!(
            uri.authority().map(http::uri::Authority::as_str),
            Some("secure:8443")
        );
        Ok(())
    }

    #[test]
    fn rejects_url_without_authority() {
        assert!(matches!(
            upstream_uri("http://", "/x", None),
            Err(BackendError::InvalidServer(_))
        ));
    }
}
