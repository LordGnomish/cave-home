// SPDX-License-Identifier: Apache-2.0
//! Service + load-balancer model.
//!
//! A [`Service`] owns a pool of backend [`Server`]s and a [`LoadBalancer`]
//! policy that picks one server per request. Selection is *deterministic*
//! given a caller-supplied counter, so it is fully testable without global
//! mutable state or a clock.
//!
//! Spec basis (public Traefik services docs):
//! * Weighted round-robin (WRR) distributes requests across servers in
//!   proportion to their integer weights.
//! * Sticky sessions pin a client to a server via a cookie value.
//! * Unhealthy servers (failed health checks) are removed from the pool.
//!
//! The health-check *transport* (active HTTP probes) is deferred to phase-1b;
//! here health is an explicit flag the caller maintains, and the selection
//! logic that *uses* it is implemented and tested.

/// A single backend server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    /// Opaque backend identifier (e.g. `http://10.0.0.2:8080`).
    pub url: String,
    /// Relative weight for weighted selection (must be ≥ 1 to receive traffic).
    pub weight: u32,
    /// Whether the server currently passes health checks.
    pub healthy: bool,
}

impl Server {
    /// A healthy server with weight 1.
    #[must_use]
    pub fn new(url: &str) -> Self {
        Self { url: url.to_string(), weight: 1, healthy: true }
    }

    /// Builder: set the weight.
    #[must_use]
    pub const fn with_weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }

    /// Builder: set the health flag.
    #[must_use]
    pub const fn with_healthy(mut self, healthy: bool) -> Self {
        self.healthy = healthy;
        self
    }
}

/// The load-balancing policy for a service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadBalancer {
    /// Weighted round-robin over the healthy servers (weight 0 = no traffic).
    WeightedRoundRobin,
    /// Sticky sessions: clients are pinned by the value of `cookie_name`.
    Sticky {
        /// Cookie carrying the pinned server's identity.
        cookie_name: String,
    },
}

/// A routable service: a server pool plus a balancing policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// Service name (referenced by routers).
    pub name: String,
    /// Backend servers.
    pub servers: Vec<Server>,
    /// Load-balancing policy.
    pub policy: LoadBalancer,
}

/// The outcome of a sticky selection: which server, and whether the client
/// needs a fresh sticky cookie set (because none was supplied or it was stale).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StickyPick<'a> {
    /// The chosen server.
    pub server: &'a Server,
    /// `Some(value)` if the caller should set a sticky cookie to this value.
    pub set_cookie: Option<String>,
}

impl Service {
    /// Build a service.
    #[must_use]
    pub fn new(name: &str, servers: Vec<Server>, policy: LoadBalancer) -> Self {
        Self { name: name.to_string(), servers, policy }
    }

    /// The healthy servers, in declaration order.
    #[must_use]
    pub fn healthy(&self) -> Vec<&Server> {
        self.servers.iter().filter(|s| s.healthy).collect()
    }

    /// Pick a server using weighted round-robin, advancing `counter` worth of
    /// virtual slots. `counter` is the request ordinal supplied by the caller
    /// (0 for the first request, 1 for the next, …). Returns `None` if there is
    /// no healthy server with positive weight.
    ///
    /// The algorithm expands the healthy pool into a weight-proportional slot
    /// ring and indexes it by `counter % total_weight`, which is the standard,
    /// deterministic WRR distribution.
    #[must_use]
    pub fn pick_round_robin(&self, counter: u64) -> Option<&Server> {
        let pool: Vec<&Server> = self
            .servers
            .iter()
            .filter(|s| s.healthy && s.weight > 0)
            .collect();
        if pool.is_empty() {
            return None;
        }
        let total: u64 = pool.iter().map(|s| u64::from(s.weight)).sum();
        if total == 0 {
            return None;
        }
        let mut slot = counter % total;
        for s in pool {
            let w = u64::from(s.weight);
            if slot < w {
                return Some(s);
            }
            slot -= w;
        }
        None
    }

    /// Sticky-session selection. If `cookie_value` names a currently-healthy
    /// server, return it with no `set_cookie`. Otherwise fall back to a
    /// round-robin pick keyed by `counter` and signal that a fresh sticky
    /// cookie (the chosen server's `url`) should be set.
    #[must_use]
    pub fn pick_sticky(&self, cookie_value: Option<&str>, counter: u64) -> Option<StickyPick<'_>> {
        if let Some(want) = cookie_value
            && let Some(s) = self.servers.iter().find(|s| s.url == want && s.healthy)
        {
            return Some(StickyPick { server: s, set_cookie: None });
        }
        let s = self.pick_round_robin(counter)?;
        Some(StickyPick { server: s, set_cookie: Some(s.url.clone()) })
    }

    /// Select a server according to this service's configured policy. For
    /// sticky services the caller passes the inbound cookie value; for WRR it
    /// is ignored. Returns the chosen server and an optional sticky cookie to
    /// set on the response.
    #[must_use]
    pub fn select(&self, cookie_value: Option<&str>, counter: u64) -> Option<StickyPick<'_>> {
        match &self.policy {
            LoadBalancer::WeightedRoundRobin => self
                .pick_round_robin(counter)
                .map(|s| StickyPick { server: s, set_cookie: None }),
            LoadBalancer::Sticky { .. } => self.pick_sticky(cookie_value, counter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrr_service(servers: Vec<Server>) -> Service {
        Service::new("svc", servers, LoadBalancer::WeightedRoundRobin)
    }

    #[test]
    fn round_robin_cycles_equal_weights() {
        let svc = wrr_service(vec![Server::new("a"), Server::new("b"), Server::new("c")]);
        let seq: Vec<&str> = (0..6).map(|i| svc.pick_round_robin(i).unwrap().url.as_str()).collect();
        assert_eq!(seq, vec!["a", "b", "c", "a", "b", "c"]);
    }

    #[test]
    fn weighted_distribution_is_proportional() {
        // a:3, b:1 -> over a window of 4 slots: a,a,a,b
        let svc = wrr_service(vec![
            Server::new("a").with_weight(3),
            Server::new("b").with_weight(1),
        ]);
        let seq: Vec<&str> = (0..4).map(|i| svc.pick_round_robin(i).unwrap().url.as_str()).collect();
        assert_eq!(seq, vec!["a", "a", "a", "b"]);
        // wraps deterministically
        assert_eq!(svc.pick_round_robin(4).unwrap().url, "a");
    }

    #[test]
    fn skips_unhealthy_servers() {
        let svc = wrr_service(vec![
            Server::new("a"),
            Server::new("b").with_healthy(false),
            Server::new("c"),
        ]);
        let seq: Vec<&str> = (0..4).map(|i| svc.pick_round_robin(i).unwrap().url.as_str()).collect();
        assert_eq!(seq, vec!["a", "c", "a", "c"]);
    }

    #[test]
    fn skips_zero_weight_servers() {
        let svc = wrr_service(vec![
            Server::new("a").with_weight(0),
            Server::new("b").with_weight(2),
        ]);
        let seq: Vec<&str> = (0..2).map(|i| svc.pick_round_robin(i).unwrap().url.as_str()).collect();
        assert_eq!(seq, vec!["b", "b"]);
    }

    #[test]
    fn no_healthy_servers_returns_none() {
        let svc = wrr_service(vec![Server::new("a").with_healthy(false)]);
        assert!(svc.pick_round_robin(0).is_none());
    }

    #[test]
    fn sticky_pins_to_named_healthy_server() {
        let svc = Service::new(
            "svc",
            vec![Server::new("a"), Server::new("b")],
            LoadBalancer::Sticky { cookie_name: "srv".to_string() },
        );
        let pick = svc.pick_sticky(Some("b"), 0).unwrap();
        assert_eq!(pick.server.url, "b");
        assert_eq!(pick.set_cookie, None);
    }

    #[test]
    fn sticky_falls_back_when_cookie_server_unhealthy() {
        let svc = Service::new(
            "svc",
            vec![Server::new("a"), Server::new("b").with_healthy(false)],
            LoadBalancer::Sticky { cookie_name: "srv".to_string() },
        );
        // cookie points at the down server -> fall back, and set a new cookie
        let pick = svc.pick_sticky(Some("b"), 0).unwrap();
        assert_eq!(pick.server.url, "a");
        assert_eq!(pick.set_cookie.as_deref(), Some("a"));
    }

    #[test]
    fn sticky_assigns_cookie_when_absent() {
        let svc = Service::new(
            "svc",
            vec![Server::new("a"), Server::new("b")],
            LoadBalancer::Sticky { cookie_name: "srv".to_string() },
        );
        let pick = svc.pick_sticky(None, 0).unwrap();
        assert_eq!(pick.server.url, "a");
        assert_eq!(pick.set_cookie.as_deref(), Some("a"));
    }

    #[test]
    fn select_dispatches_on_policy() {
        let wrr = wrr_service(vec![Server::new("a"), Server::new("b")]);
        assert_eq!(wrr.select(None, 1).unwrap().server.url, "b");

        let sticky = Service::new(
            "svc",
            vec![Server::new("a"), Server::new("b")],
            LoadBalancer::Sticky { cookie_name: "srv".to_string() },
        );
        assert_eq!(sticky.select(Some("b"), 0).unwrap().server.url, "b");
    }

    #[test]
    fn healthy_filters_pool() {
        let svc = wrr_service(vec![
            Server::new("a"),
            Server::new("b").with_healthy(false),
        ]);
        assert_eq!(svc.healthy().len(), 1);
    }
}
