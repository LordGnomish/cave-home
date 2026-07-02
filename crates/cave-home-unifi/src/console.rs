// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The console abstraction: where the APIs live and how their URLs are built.
//!
//! A household's UniFi stack is reached one of two ways, and they differ only
//! in URL prefix and login path:
//!
//! - **UniFi OS console** ([`ConsoleKind::UnifiOs`]) — a Dream Machine /
//!   Router, a UNVR, or a Cloud Key Gen2+. One HTTPS port (443) fronts every
//!   application behind a `/proxy/<app>` prefix: Network at `/proxy/network`,
//!   Protect at `/proxy/protect`. Login is `/api/auth/login`.
//! - **Legacy controller** ([`ConsoleKind::Legacy`]) — a standalone Network
//!   application (the old self-hosted controller / Cloud Key Gen1) on port
//!   8443, with the Network API mounted at the root and login at `/api/login`.
//!
//! [`Console`] turns a `(scheme, host, port, kind)` tuple into the concrete
//! absolute URLs every API call needs, so no other module has to know the
//! prefix rules. UniFi **Access** runs its own developer API on a dedicated
//! port with bearer auth and is modelled separately (see [`crate::access`]),
//! because it is not proxied behind the console session.

/// Which flavour of console we are talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleKind {
    /// A UniFi OS console (Dream Machine, UNVR, Cloud Key Gen2+): every app is
    /// behind a `/proxy/<app>` prefix on port 443; login at `/api/auth/login`.
    UnifiOs,
    /// A legacy standalone Network controller (Cloud Key Gen1 / self-hosted):
    /// Network API at the root on port 8443; login at `/api/login`.
    Legacy,
}

impl ConsoleKind {
    /// The default management port for this console kind.
    #[must_use]
    pub const fn default_port(self) -> u16 {
        match self {
            Self::UnifiOs => 443,
            Self::Legacy => 8443,
        }
    }

    /// The login path for this console kind.
    #[must_use]
    pub const fn login_path(self) -> &'static str {
        match self {
            Self::UnifiOs => "/api/auth/login",
            Self::Legacy => "/api/login",
        }
    }

    /// The logout path for this console kind.
    #[must_use]
    pub const fn logout_path(self) -> &'static str {
        match self {
            Self::UnifiOs => "/api/auth/logout",
            Self::Legacy => "/api/logout",
        }
    }

    /// The path prefix the Network application is mounted under (empty for
    /// legacy, `/proxy/network` on UniFi OS).
    #[must_use]
    pub const fn network_prefix(self) -> &'static str {
        match self {
            Self::UnifiOs => "/proxy/network",
            Self::Legacy => "",
        }
    }

    /// The path prefix the Protect application is mounted under. Protect only
    /// exists on UniFi OS consoles (a UNVR / Dream Machine), so legacy maps to
    /// the same prefix for URL-building completeness but is never used.
    #[must_use]
    pub const fn protect_prefix(self) -> &'static str {
        "/proxy/protect"
    }
}

/// A reachable UniFi console: the host, the port, the TLS scheme and the kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Console {
    host: String,
    port: u16,
    kind: ConsoleKind,
    /// `true` for `https`/`wss` (the only real-world case); kept configurable so
    /// the `wiremock` e2e can point the same code at a plain-`http` mock.
    tls: bool,
}

impl Console {
    /// A UniFi OS console at `host` on the default port 443 over TLS.
    #[must_use]
    pub fn unifi_os(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: ConsoleKind::UnifiOs.default_port(),
            kind: ConsoleKind::UnifiOs,
            tls: true,
        }
    }

    /// A legacy controller at `host` on the default port 8443 over TLS.
    #[must_use]
    pub fn legacy(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: ConsoleKind::Legacy.default_port(),
            kind: ConsoleKind::Legacy,
            tls: true,
        }
    }

    /// Builder: override the port.
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Builder: choose `http`/`ws` instead of `https`/`wss` (e2e / mock only).
    #[must_use]
    pub fn with_tls(mut self, tls: bool) -> Self {
        self.tls = tls;
        self
    }

    /// The console kind.
    #[must_use]
    pub fn kind(&self) -> ConsoleKind {
        self.kind
    }

    /// The host.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The HTTP origin, e.g. `https://10.0.0.1:443`.
    #[must_use]
    pub fn base_url(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{scheme}://{}:{}", self.host, self.port)
    }

    /// The WebSocket origin, e.g. `wss://10.0.0.1:443`.
    #[must_use]
    pub fn ws_base(&self) -> String {
        let scheme = if self.tls { "wss" } else { "ws" };
        format!("{scheme}://{}:{}", self.host, self.port)
    }

    /// An absolute URL for a console-relative `path` (must start with `/`).
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url(), path)
    }

    /// The absolute login URL.
    #[must_use]
    pub fn login_url(&self) -> String {
        self.url(self.kind.login_path())
    }

    /// The absolute logout URL.
    #[must_use]
    pub fn logout_url(&self) -> String {
        self.url(self.kind.logout_path())
    }

    /// An absolute Network-API URL for a site-scoped sub-path.
    ///
    /// `sub` is the part after `/api/s/{site}/`, e.g. `stat/sta`. The
    /// `/proxy/network` prefix is applied automatically on UniFi OS.
    #[must_use]
    pub fn network_site_url(&self, site: &str, sub: &str) -> String {
        self.url(&format!(
            "{}/api/s/{site}/{sub}",
            self.kind.network_prefix()
        ))
    }

    /// An absolute Network-API URL for a non-site path, e.g. `self/sites`.
    #[must_use]
    pub fn network_url(&self, sub: &str) -> String {
        self.url(&format!("{}/api/{sub}", self.kind.network_prefix()))
    }

    /// The WebSocket URL for the Network site event stream.
    #[must_use]
    pub fn network_events_ws_url(&self, site: &str) -> String {
        format!(
            "{}{}/wss/s/{site}/events",
            self.ws_base(),
            self.kind.network_prefix()
        )
    }

    /// An absolute Protect-API URL, e.g. `bootstrap` -> `/proxy/protect/api/bootstrap`.
    #[must_use]
    pub fn protect_url(&self, sub: &str) -> String {
        self.url(&format!("{}/api/{sub}", self.kind.protect_prefix()))
    }

    /// The Protect binary-update WebSocket URL.
    #[must_use]
    pub fn protect_updates_ws_url(&self) -> String {
        format!("{}{}/ws/updates", self.ws_base(), self.kind.protect_prefix())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_console_kind() {
        assert_eq!(ConsoleKind::UnifiOs.default_port(), 443);
        assert_eq!(ConsoleKind::Legacy.default_port(), 8443);
        assert_eq!(ConsoleKind::UnifiOs.login_path(), "/api/auth/login");
        assert_eq!(ConsoleKind::Legacy.login_path(), "/api/login");
    }

    #[test]
    fn unifi_os_network_urls_carry_proxy_prefix() {
        let c = Console::unifi_os("10.0.0.1");
        assert_eq!(c.base_url(), "https://10.0.0.1:443");
        assert_eq!(
            c.network_site_url("default", "stat/sta"),
            "https://10.0.0.1:443/proxy/network/api/s/default/stat/sta"
        );
        assert_eq!(
            c.network_url("self/sites"),
            "https://10.0.0.1:443/proxy/network/api/self/sites"
        );
        assert_eq!(c.login_url(), "https://10.0.0.1:443/api/auth/login");
    }

    #[test]
    fn legacy_network_urls_have_no_prefix_and_8443() {
        let c = Console::legacy("nas.lan");
        assert_eq!(c.base_url(), "https://nas.lan:8443");
        assert_eq!(
            c.network_site_url("default", "stat/device"),
            "https://nas.lan:8443/api/s/default/stat/device"
        );
        assert_eq!(c.login_url(), "https://nas.lan:8443/api/login");
    }

    #[test]
    fn protect_urls_are_proxy_prefixed() {
        let c = Console::unifi_os("nvr.lan");
        assert_eq!(
            c.protect_url("bootstrap"),
            "https://nvr.lan:443/proxy/protect/api/bootstrap"
        );
        assert_eq!(
            c.protect_updates_ws_url(),
            "wss://nvr.lan:443/proxy/protect/ws/updates"
        );
    }

    #[test]
    fn websocket_urls_use_ws_scheme() {
        let c = Console::unifi_os("h");
        assert_eq!(
            c.network_events_ws_url("default"),
            "wss://h:443/proxy/network/wss/s/default/events"
        );
        let legacy = Console::legacy("h");
        assert_eq!(
            legacy.network_events_ws_url("s1"),
            "wss://h:8443/wss/s/s1/events"
        );
    }

    #[test]
    fn with_tls_false_downgrades_to_http_for_mock() {
        let c = Console::unifi_os("127.0.0.1").with_port(18080).with_tls(false);
        assert_eq!(c.base_url(), "http://127.0.0.1:18080");
        assert_eq!(c.ws_base(), "ws://127.0.0.1:18080");
        assert_eq!(
            c.network_url("self/sites"),
            "http://127.0.0.1:18080/proxy/network/api/self/sites"
        );
    }

    #[test]
    fn accessors_expose_config() {
        let c = Console::legacy("box").with_port(9000);
        assert_eq!(c.host(), "box");
        assert_eq!(c.port(), 9000);
        assert_eq!(c.kind(), ConsoleKind::Legacy);
    }
}
